"""Pairing codes, bindings and node registration (AUTH §5.6, §9, §14.1)."""

import json
import time

import pytest

from dreamference.accounts import AccountsBindings, AccountsJwt, AccountsOauth, AccountsStore
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
    store.create_user("alice", "pw-alice-1")
    store.create_user("bob", "pw-bob-1")
    _, _, node = call(oauth, "POST", "/ragon/v1/nodes", fields={"node_id": NODE})
    return store.mint_key("alice", "k"), store.mint_key("bob", "k"), node["key"]


def pair(oauth, alice, service, kind="telegram", identity="42"):
    _, _, made = call(oauth, "POST", "/ragon/v1/bindings", alice, {"chat_kind": kind})
    return call(
        oauth,
        "POST",
        "/ragon/v1/bindings/redeem",
        service,
        {"code": made["code"], "chat_kind": kind, "chat_identity": identity},
    )


def test_bindings_a_code_binds_one_chat_and_the_bridge_key_works(oauth, world):
    alice, _, service = world
    status, _, made = call(oauth, "POST", "/ragon/v1/bindings", alice, {"chat_kind": "telegram"})
    assert status == 200 and len(made["code"]) == 6 and made["expires_in"] == 3600
    status, _, bound = call(
        oauth,
        "POST",
        "/ragon/v1/bindings/redeem",
        service,
        {"code": made["code"].lower(), "chat_kind": "telegram", "chat_identity": "42"},
    )
    assert status == 200 and bound["username"] == "alice"
    assert call(oauth, "GET", "/ragon/v1/users/me", bound["key"])[2]["username"] == "alice"
    listed = call(oauth, "GET", "/ragon/v1/bindings", alice)[2]["bindings"]
    assert [b["chat_identity"] for b in listed] == ["42"]


def test_bindings_a_code_is_consumed_by_its_first_use(oauth, world):
    alice, _, service = world
    _, _, made = call(oauth, "POST", "/ragon/v1/bindings", alice, {"chat_kind": "matrix"})
    fields = {"code": made["code"], "chat_kind": "matrix", "chat_identity": "@a:b"}
    assert call(oauth, "POST", "/ragon/v1/bindings/redeem", service, fields)[0] == 200
    assert call(oauth, "POST", "/ragon/v1/bindings/redeem", service, fields)[0] == 400


def test_bindings_a_code_is_for_its_chat_kind_and_expires(oauth, store, world):
    alice, _, service = world
    _, _, made = call(oauth, "POST", "/ragon/v1/bindings", alice, {"chat_kind": "matrix"})
    wrong = {"code": made["code"], "chat_kind": "telegram", "chat_identity": "1"}
    assert call(oauth, "POST", "/ragon/v1/bindings/redeem", service, wrong)[0] == 400
    store.db.execute("UPDATE pairing_codes SET expires = ?", (time.time() - 1,))
    right = {"code": made["code"], "chat_kind": "matrix", "chat_identity": "1"}
    assert call(oauth, "POST", "/ragon/v1/bindings/redeem", service, right)[0] == 400


def test_bindings_repairing_moves_the_identity_and_revokes_the_old_key(oauth, world):
    alice, bob, service = world
    old = pair(oauth, alice, service)[2]
    new = pair(oauth, bob, service)[2]
    assert new["username"] == "bob"
    assert call(oauth, "GET", "/ragon/v1/users/me", old["key"])[0] == 401
    assert call(oauth, "GET", "/ragon/v1/users/me", new["key"])[0] == 200
    assert call(oauth, "GET", "/ragon/v1/bindings", alice)[2]["bindings"] == []


def test_bindings_only_a_service_key_redeems(oauth, world):
    alice, _, _ = world
    _, _, made = call(oauth, "POST", "/ragon/v1/bindings", alice, {"chat_kind": "discord"})
    fields = {"code": made["code"], "chat_kind": "discord", "chat_identity": "9"}
    assert call(oauth, "POST", "/ragon/v1/bindings/redeem", alice, fields)[0] == 403
    assert call(oauth, "POST", "/ragon/v1/bindings/redeem", None, fields)[0] == 401


def test_bindings_remove_deletes_the_binding_and_its_key(oauth, world):
    alice, bob, service = world
    bound = pair(oauth, alice, service)[2]
    assert call(oauth, "DELETE", "/ragon/v1/bindings/" + bound["id"], bob)[0] == 404
    assert call(oauth, "DELETE", "/ragon/v1/bindings/" + bound["id"], alice)[0] == 200
    assert call(oauth, "GET", "/ragon/v1/bindings", alice)[2]["bindings"] == []
    assert call(oauth, "GET", "/ragon/v1/users/me", bound["key"])[0] == 401


def test_bindings_reject_an_unknown_chat_kind_and_anonymous_callers(oauth, world):
    alice, _, _ = world
    assert call(oauth, "POST", "/ragon/v1/bindings", alice, {"chat_kind": "irc"})[0] == 400
    assert call(oauth, "GET", "/ragon/v1/bindings")[0] == 401
    assert call(oauth, "PUT", "/ragon/v1/bindings", alice)[0] == 405


def test_bindings_removing_the_user_drops_bindings(oauth, store, world):
    _, bob, service = world
    _, _, made = call(oauth, "POST", "/ragon/v1/bindings", bob, {"chat_kind": "telegram"})
    fields = {"code": made["code"], "chat_kind": "telegram", "chat_identity": "7"}
    assert call(oauth, "POST", "/ragon/v1/bindings/redeem", service, fields)[0] == 200
    store.remove_user("bob")
    assert store.db.execute("SELECT COUNT(*) FROM bindings").fetchone()[0] == 0


def test_nodes_register_once_and_answer_the_key_only_the_first_time(oauth, store):
    status, _, first = call(oauth, "POST", "/ragon/v1/nodes", fields={"node_id": NODE})
    assert status == 201 and first["username"] == "ragon-node-0f8e1c52" and first["key"]
    status, _, again = call(oauth, "POST", "/ragon/v1/nodes", fields={"node_id": NODE})
    assert status == 200 and "key" not in again and again["created"] is False
    service = [u for u in store.list_users() if u["service"]]
    assert [u["username"] for u in service] == ["ragon-node-0f8e1c52"]


def test_nodes_reject_a_bad_node_id(oauth):
    assert call(oauth, "POST", "/ragon/v1/nodes", fields={"node_id": "x y"})[0] == 400
    assert call(oauth, "POST", "/ragon/v1/nodes", fields={})[0] == 400
    assert call(oauth, "GET", "/ragon/v1/nodes")[0] == 405


def test_bindings_codes_use_an_unambiguous_alphabet():
    assert not set("01OIL") & set(AccountsBindings.normalize("ab-c d").upper() + "")
    assert AccountsBindings.normalize("ab-c d") == "ABCD"
