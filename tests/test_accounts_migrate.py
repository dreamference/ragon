"""Tests for the primary/secondary choice and the migration (AUTH §7)."""

import argparse

from dreamference.accounts.accounts_migrate import AccountsMigrate
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


def test_primary_self_and_pin():
    assert AccountsMigrate.run_primary(_args(primary_self=True)) == 0
    assert NodeSettings.auth_primary() and NodeSettings.pinned_primary() is None
    assert AccountsMigrate.run_primary(_args(primary_id="abc")) == 0
    assert not NodeSettings.auth_primary() and NodeSettings.pinned_primary() == "abc"
    assert AccountsMigrate.run_primary(_args(primary_id="x", primary_self=True)) == 2
    assert NodeSettings.load()["advertise"] is False  # the switches are untouched


def test_choose_secondary_lists_advertised_primaries():
    browse = lambda: [  # noqa: E731
        {"node": "p1", "name": "alpha", "auth": "primary"},
        {"node": "s1", "name": "beta"},
    ]
    answers = iter(["secondary", "1"])
    said: list[str] = []
    result = AccountsMigrate.choose(browse, ask=lambda _p: next(answers), say=said.append)
    assert result == "secondary"
    assert NodeSettings.pinned_primary() == "p1"
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
