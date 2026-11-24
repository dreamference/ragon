"""Tests for the primary/secondary choice and the migration (AUTH §7)."""

import argparse

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


CA = b"-----BEGIN CERTIFICATE-----\nprimary\n-----END CERTIFICATE-----\n"
PRIMARY = {"node": "abc", "name": "alpha", "host": "alpha.local.", "address": "192.0.2.7"}


def _browse(*entries):
    return lambda: [dict(entry) for entry in entries or (PRIMARY,)]


def _fetch(pem=CA):
    seen: list[tuple] = []

    def fetch(address, port):
        seen.append((address, port))
        return pem

    fetch.seen = seen  # type: ignore[attr-defined]
    return fetch


def test_primary_self_and_pin():
    assert AccountsMigrate.run_primary(_args(primary_self=True)) == 0
    assert NodeSettings.auth_primary() and NodeSettings.pinned_primary() is None
    assert AccountsMigrate.run_primary(_args(primary_id="abc"), _browse(), _fetch()) == 0
    assert not NodeSettings.auth_primary() and NodeSettings.pinned_primary() == "abc"
    assert AccountsMigrate.run_primary(_args(primary_id="x", primary_self=True)) == 2
    assert NodeSettings.load()["advertise"] is False  # the switches are untouched


def test_choose_secondary_lists_advertised_primaries():
    browse = lambda: [  # noqa: E731
        {"node": "p1", "name": "alpha", "auth": "primary", "address": "192.0.2.7"},
        {"node": "s1", "name": "beta"},
    ]
    answers = iter(["secondary", "1"])
    said: list[str] = []
    result = AccountsMigrate.choose(
        browse, ask=lambda _p: next(answers), say=said.append, fetch_ca=_fetch()
    )
    assert result == "secondary"
    assert NodeSettings.pinned_primary() == "p1"
    assert AccountsSecondary.ca_path("p1").read_bytes() == CA
    assert len(said) == 1 and "alpha" in said[0]
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


@pytest.mark.parametrize("wanted", ["abc", "alpha", "ALPHA", "alpha.local", "alpha.local."])
def test_pinning_by_name_stores_the_node_id_and_its_ca(wanted):
    """The pair pinned `seventh-puffin`, a name: the forwarder looked for a node with that id."""
    fetch = _fetch()
    assert AccountsMigrate.run_primary(_args(primary_id=wanted), _browse(), fetch) == 0
    assert NodeSettings.pinned_primary() == "abc"
    assert fetch.seen == [("192.0.2.7", None)]
    assert AccountsSecondary.ca_path("abc").read_bytes() == CA


def test_pinning_a_node_not_advertised_changes_nothing(capsys):
    assert AccountsMigrate.run_primary(_args(primary_id="nobody"), _browse(), _fetch()) == 1
    assert NodeSettings.pinned_primary() is None
    assert "No node nobody is advertised" in capsys.readouterr().out


def test_pinning_fails_cleanly_when_the_primary_serves_no_ca(capsys):
    def down(address, port):
        raise OSError("its gate did not answer (refused)")

    assert AccountsMigrate.run_primary(_args(primary_id="alpha"), _browse(), down) == 1
    assert NodeSettings.pinned_primary() is None
    assert not AccountsSecondary.ca_path("abc").exists()
    assert "could not be pinned" in capsys.readouterr().out


def test_a_changed_primary_certificate_is_refused(capsys):
    assert AccountsMigrate.run_primary(_args(primary_id="alpha"), _browse(), _fetch()) == 0
    other = CA.replace(b"primary", b"impostor")
    assert AccountsMigrate.run_primary(_args(primary_id="alpha"), _browse(), _fetch(other)) == 1
    assert AccountsSecondary.ca_path("abc").read_bytes() == CA
    assert "has changed" in capsys.readouterr().out


def test_pinning_this_node_is_refused(monkeypatch):
    from dreamference.node.node_identity import NodeIdentity

    monkeypatch.setattr(NodeIdentity, "read", classmethod(lambda cls: "abc"))
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
