"""Usage per user, day and client kind, and the unauthenticated counts (AUTH §9, §10)."""

import argparse
import json

import pytest

from dreamference.accounts import (
    AccountsAdmin,
    AccountsJwt,
    AccountsOauth,
    AccountsStore,
    AccountsUsage,
)
from dreamference.accounts.accounts_service import AccountsService

NODE = "0f8e1c52-aaaa-bbbb-cccc-123456789abc"


@pytest.fixture
def store(tmp_path):
    return AccountsStore(tmp_path / "a.db", primary_name="box", audit_sink=lambda _l: None)


@pytest.fixture
def oauth(store, tmp_path):
    signer = AccountsJwt(tmp_path / "k.pem", AccountsService.issuer("box"), "node-1", "box")
    return AccountsOauth(store, signer)


def call(oauth, method, path, key=None, fields=None):
    headers = {"authorization": f"Bearer {key}"} if key else {}
    body = b""
    if fields is not None:
        headers["content-type"] = "application/json"
        body = json.dumps(fields).encode()
    return oauth.handle(method, path, headers, body)


@pytest.fixture
def world(store, oauth):
    store.create_user("admin1", "pw-admin-1")
    store.create_user("bob", "pw-bob-12")
    _, _, node = call(oauth, "POST", "/ragon/v1/nodes", fields={"node_id": NODE})
    store.record_usage("admin1", "codex", 10, 5)
    store.record_usage("bob", "codex", 1, 1)
    store.record_usage("bob", "web", 2, 2)
    return store.mint_key("admin1", "k"), store.mint_key("bob", "k"), node["key"]


def test_usage_a_user_reads_their_own_rows_and_an_admin_everyone_s(oauth, world):
    admin, bob, _ = world
    own = call(oauth, "GET", "/ragon/v1/usage", bob)[2]["usage"]
    assert {r["username"] for r in own} == {"bob"} and {r["client_kind"] for r in own} == {
        "codex",
        "web",
    }
    everyone = call(oauth, "GET", "/ragon/v1/usage?days=3", admin)[2]["usage"]
    assert {r["username"] for r in everyone} == {"admin1", "bob"}
    assert call(oauth, "GET", "/ragon/v1/usage")[0] == 401


def test_usage_unauthenticated_counts_are_sent_by_a_node_and_read_by_an_admin(oauth, world):
    admin, bob, service = world
    day = AccountsUsage.today()
    counts = [{"client_kind": "codex", "source": "10.0.0.5", "requests": 3}]
    send = {"day": day, "counts": counts}
    path = "/ragon/v1/usage/unauthenticated"
    assert call(oauth, "POST", path, service, send)[2] == {"stored": 1}
    assert call(oauth, "POST", path, service, send)[2] == {"stored": 1}  # a repeat replaces
    rows = call(oauth, "GET", path, admin)[2]["unauthenticated"]
    assert rows == [
        {
            "day": day,
            "node_id": "ragon-node-0f8e1c52",
            "client_kind": "codex",
            "source": "10.0.0.5",
            "requests": 3,
        }
    ]
    assert call(oauth, "GET", path, bob)[0] == 403
    assert call(oauth, "POST", path, bob, send)[0] == 403
    assert call(oauth, "POST", path, None, send)[0] == 401


@pytest.mark.parametrize(
    "send",
    [
        {"day": "yesterday", "counts": []},
        {"day": "2026-11-19", "counts": [{"client_kind": "x", "source": "s", "requests": -1}]},
        {"day": "2026-11-19", "counts": [{"client_kind": "x", "source": "s", "requests": True}]},
        {"day": "2026-11-19", "counts": ["no"]},
        {"day": "2026-11-19"},
    ],
)
def test_usage_malformed_counts_are_refused(oauth, world, send):
    assert call(oauth, "POST", "/ragon/v1/usage/unauthenticated", world[2], send)[0] == 400


def test_usage_the_admin_command_prints_the_unauthenticated_days(store, capsys):
    usage = AccountsUsage(store)
    usage.add_unauthenticated(
        "ragon-node-1",
        AccountsUsage.today(),
        [{"client_kind": "codex", "source": "h", "requests": 4}],
    )
    args = argparse.Namespace(user_command="usage", days=7, unauthenticated=True)
    assert AccountsAdmin.run(args, store) == 0
    out = capsys.readouterr().out
    assert "ragon-node-1\tcodex\th\t4 requests" in out
