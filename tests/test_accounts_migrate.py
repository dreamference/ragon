"""Tests for the primary/secondary choice and the migration (AUTH §7)."""

import argparse
from pathlib import Path

import pytest

from dreamference.accounts.accounts_error import AccountsError
from dreamference.accounts.accounts_migrate import AccountsMigrate
from dreamference.accounts.accounts_secondary import AccountsSecondary
from dreamference.node.node_service_file import NodeServiceFile
from dreamference.node.node_settings import NodeSettings


def _args(**kw):
    base = {"primary_id": None, "primary_self": False, "secondaries": []}
    base.update(kw)
    return argparse.Namespace(**base)


def test_render_and_update_carry_auth_primary(tmp_path):
    NodeServiceFile.service_path = tmp_path / "s.service"
    text = NodeServiceFile.render(8000, "n1", "1.0", auth_primary=True)
    assert "<txt-record>auth=primary</txt-record>" in text
    assert NodeServiceFile.parse(text)["auth"] == "primary"
    assert "auth=" not in NodeServiceFile.render(8000, "n1", "1.0")
    NodeServiceFile.service_path.write_text(text)
    NodeServiceFile.update(state="ready")
    assert (NodeServiceFile.read() or {}).get("auth") == "primary"
    NodeServiceFile.update(auth_primary=False)
    assert "auth" not in (NodeServiceFile.read() or {})


PRIMARY_ID = "6f1d2c3b-4a59-4e68-9f70-8a1b2c3d4e5f"
OTHER_ID = "11111111-2222-4333-8444-555555555555"
PRIMARY = {
    "node": PRIMARY_ID,
    "name": "alpha",
    "host": "alpha.local.",
    "address": "192.0.2.7",
    "auth": "primary",
}
PEMS: dict[str, bytes] = {}


@pytest.fixture(scope="module", autouse=True)
def _cas(tmp_path_factory):
    """Two real CA certificates: the primary's and an impostor's (the fingerprint parses them)."""
    from dreamference.vllm_server.model_gate_tls import CA_CERT, ModelGateTls

    for name in ("primary", "impostor"):
        folder = ModelGateTls.ensure(tmp_path_factory.mktemp(name), [f"{name}.example"])
        PEMS[name] = (folder / CA_CERT).read_bytes()


def _fingerprint(pem):
    from dreamference.vllm_server.model_gate_tls import ModelGateTls

    return ModelGateTls.fingerprint(pem)


def _browse(*entries):
    return lambda: [dict(entry) for entry in entries or (PRIMARY,)]


def _fetch(pem=None):
    seen: list[tuple] = []

    def fetch(address, port, node_id):
        seen.append((address, port, node_id))
        return pem or PEMS["primary"]

    fetch.seen = seen  # type: ignore[attr-defined]
    return fetch


def test_primary_self_and_pin():
    assert AccountsMigrate.run_primary(_args(primary_self=True)) == 0
    assert NodeSettings.auth_primary() and NodeSettings.pinned_primary() is None
    assert AccountsMigrate.run_primary(_args(primary_id=PRIMARY_ID), _browse(), _fetch()) == 0
    assert not NodeSettings.auth_primary() and NodeSettings.pinned_primary() == PRIMARY_ID
    assert AccountsMigrate.run_primary(_args(primary_id="x", primary_self=True)) == 2
    assert NodeSettings.load()["advertise"] is False  # the switches are untouched


def test_choose_secondary_lists_advertised_primaries():
    browse = lambda: [  # noqa: E731
        {"node": PRIMARY_ID, "name": "alpha", "auth": "primary", "address": "192.0.2.7"},
        {"node": "s1", "name": "beta"},
    ]
    answers = iter(["secondary", "1"])
    said: list[str] = []
    result = AccountsMigrate.choose(
        browse, ask=lambda _p: next(answers), say=said.append, fetch_ca=_fetch()
    )
    assert result == "secondary"
    assert NodeSettings.pinned_primary() == PRIMARY_ID
    assert AccountsSecondary.ca_path(PRIMARY_ID).read_bytes() == PEMS["primary"]
    assert "alpha" in said[0] and _fingerprint(PEMS["primary"]) in said[1]
    assert AccountsMigrate.choose(browse, ask=lambda _p: "x") is None  # already chosen


def test_choose_primary():
    assert AccountsMigrate.choose(list, ask=lambda _p: "primary") == "primary"
    assert NodeSettings.auth_primary()


def test_migrate_registers_service_accounts(monkeypatch):
    class Store:
        def __init__(self):
            self.nodes = []

        def ensure_service_account(self, node):
            self.nodes.append(node)
            return {"username": f"node-{node}"}

    store = Store()
    result = AccountsMigrate.migrate(store, "me", ["a", "b", "me"])
    assert store.nodes == ["me", "a", "b"]
    assert result["accounts"] == ["node-me", "node-a", "node-b"]
    assert result["commands"] == ["ragon-admin node primary me"] * 2
    assert NodeSettings.auth_primary()


def test_accounts_migrate_migrate_rejects_a_node_without_id():
    class Store:
        def ensure_service_account(self, node):
            raise AssertionError("no account may be registered")

    with pytest.raises(AccountsError, match="no id"):
        AccountsMigrate.migrate(Store(), "", ["a"])


# ----- `node primary <name>` on a running node (the w48s test pair, 11-24) ----------------------


@pytest.mark.parametrize("wanted", [PRIMARY_ID, "alpha", "ALPHA", "alpha.local", "alpha.local."])
def test_pinning_by_name_stores_the_node_id_and_its_ca(wanted):
    """The pair pinned `seventh-puffin`, a name: the forwarder looked for a node with that id."""
    fetch = _fetch()
    assert AccountsMigrate.run_primary(_args(primary_id=wanted), _browse(), fetch) == 0
    assert NodeSettings.pinned_primary() == PRIMARY_ID
    assert fetch.seen == [("192.0.2.7", None, PRIMARY_ID)]
    assert AccountsSecondary.ca_path(PRIMARY_ID).read_bytes() == PEMS["primary"]


def test_pinning_a_node_not_advertised_changes_nothing(capsys):
    assert AccountsMigrate.run_primary(_args(primary_id="nobody"), _browse(), _fetch()) == 1
    assert NodeSettings.pinned_primary() is None
    assert "No node nobody is advertised" in capsys.readouterr().out


def test_pinning_fails_cleanly_when_the_primary_serves_no_ca(capsys):
    def down(address, port, node_id):
        raise OSError("its gate did not answer (refused)")

    assert AccountsMigrate.run_primary(_args(primary_id="alpha"), _browse(), down) == 1
    assert NodeSettings.pinned_primary() is None
    assert not AccountsSecondary.ca_path(PRIMARY_ID).exists()
    assert "could not be pinned" in capsys.readouterr().out


def test_a_changed_primary_certificate_is_refused(capsys):
    assert AccountsMigrate.run_primary(_args(primary_id="alpha"), _browse(), _fetch()) == 0
    other = PEMS["impostor"]
    assert AccountsMigrate.run_primary(_args(primary_id="alpha"), _browse(), _fetch(other)) == 1
    assert AccountsSecondary.ca_path(PRIMARY_ID).read_bytes() == PEMS["primary"]
    assert "has changed" in capsys.readouterr().out


@pytest.mark.parametrize("node", ["../../.ssh", "abc", ""])
def test_an_advertised_id_that_is_not_a_uuid_is_refused(node, capsys):
    odd = {**PRIMARY, "node": node}
    assert AccountsMigrate.run_primary(_args(primary_id="alpha"), _browse(odd), _fetch()) == 1
    assert NodeSettings.pinned_primary() is None
    assert not list(Path.home().glob("**/ca.pem"))


def test_pinning_this_node_is_refused(monkeypatch):
    from dreamference.node.node_identity import NodeIdentity

    monkeypatch.setattr(NodeIdentity, "read", classmethod(lambda cls: PRIMARY_ID))
    assert AccountsMigrate.run_primary(_args(primary_id="alpha"), _browse(), _fetch()) == 1
    assert NodeSettings.pinned_primary() is None


def test_pinning_makes_the_service_and_the_gate_info_follow(monkeypatch):
    from dreamference.accounts.accounts_service import AccountsService

    followed: list[bool] = []
    monkeypatch.setattr(
        AccountsService, "follow_role", classmethod(lambda cls: followed.append(True) or True)
    )
    assert AccountsMigrate.run_primary(_args(primary_id="alpha"), _browse(), _fetch()) == 0
    assert AccountsMigrate.run_primary(_args(primary_self=True)) == 0
    assert followed == [True, True]


# ----- the review's B1 and B3 (11-24): only one advertised primary is pinned ---------------------


def _nothing_pinned():
    assert NodeSettings.pinned_primary() is None
    assert not list(Path.home().glob("**/ca.pem"))


def test_a_node_that_keeps_no_accounts_is_not_pinned(capsys):
    """B1: another secondary (or a node with no accounts) pinned as the primary 401s every key."""
    secondary = {k: v for k, v in PRIMARY.items() if k != "auth"}
    for entry in (secondary, {**secondary, "auth": "secondary"}):
        fetch = _fetch()
        assert AccountsMigrate.run_primary(_args(primary_id="alpha"), _browse(entry), fetch) == 1
        assert fetch.seen == []
        _nothing_pinned()
        assert "alpha does not keep accounts (auth=primary not advertised)" in (
            capsys.readouterr().out
        )


ANOTHER_ID = {"node": OTHER_ID, "host": "beta.local.", "address": "192.0.2.9"}
ANOTHER_HOST = {"host": "beta.local.", "address": "192.0.2.9"}
ANOTHER_PORT = {"port": "8443"}


@pytest.mark.parametrize(
    ("wanted", "impostor"),
    [
        ("alpha", ANOTHER_ID),  # the primary's name, another node id
        ("alpha", ANOTHER_HOST),  # the primary's id from another host
        (PRIMARY_ID, ANOTHER_HOST),
        ("alpha", ANOTHER_PORT),  # the same host on another port
        (PRIMARY_ID, ANOTHER_PORT),
    ],
)
def test_a_name_or_id_that_several_nodes_advertise_is_refused(wanted, impostor, capsys):
    """B3: the first match won, and the success line named no UUID the operator could check."""
    fetch = _fetch()
    entries = (PRIMARY, {**PRIMARY, **impostor})
    assert AccountsMigrate.run_primary(_args(primary_id=wanted), _browse(*entries), fetch) == 1
    assert fetch.seen == []
    _nothing_pinned()
    out = capsys.readouterr().out
    assert "advertised by several nodes" in out and "Pin by UUID on a quiet network" in out


def test_one_node_seen_at_several_addresses_is_pinned_at_its_best():
    """One host on two interfaces is one node: mDNS keeps the host name unique."""
    v6 = {**PRIMARY, "address": "fe80::7%eth0"}
    fetch = _fetch()
    assert AccountsMigrate.run_primary(_args(primary_id="alpha"), _browse(v6, PRIMARY), fetch) == 0
    assert fetch.seen == [("192.0.2.7", None, PRIMARY_ID)]


def test_the_pin_and_the_primary_print_the_same_ca_fingerprint(capsys):
    """B3: the operator compares the two out of band; nothing else stops an impostor at first contact."""
    from dreamference.vllm_server.model_gate_tls import CA_CERT, ModelGateTls

    assert AccountsMigrate.run_primary(_args(primary_id="alpha"), _browse(), _fetch()) == 0
    out = capsys.readouterr().out
    fingerprint = _fingerprint(PEMS["primary"])
    assert PRIMARY_ID in out and f"SHA-256 {fingerprint}" in out
    assert AccountsMigrate.run_primary(_args()) == 0  # `node primary` on the secondary
    assert f"primary: {PRIMARY_ID}\nCA SHA-256: {fingerprint}" in capsys.readouterr().out
    # On the primary: its own gate's CA.
    NodeSettings.set_self_primary()
    assert "CA SHA-256: none yet" in AccountsMigrate.describe("me")
    folder = ModelGateTls.ensure(ModelGateTls.directory(), ["alpha.example"])
    own = _fingerprint((folder / CA_CERT).read_bytes())
    assert AccountsMigrate.describe("me") == f"primary: this node (me)\nCA SHA-256: {own}"
