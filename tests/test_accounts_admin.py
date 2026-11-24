"""`ragon-admin node user`: the sub-commands over a store."""

import argparse
from unittest.mock import patch

import pytest

from dreamference.accounts import AccountsAdmin, AccountsStore


def _parse(*argv):
    parser = argparse.ArgumentParser()
    AccountsAdmin.add_parser(parser.add_subparsers(dest="node_command"))
    return parser.parse_args(["user", *argv])


def _store(tmp_path, lines):
    return AccountsStore(tmp_path / "a.db", primary_name="box", audit_sink=lines.append)


def test_accounts_admin_add_list_promote_demote_remove(tmp_path, capsys):
    lines: list[str] = []
    store = _store(tmp_path, lines)
    with patch("getpass.getpass", return_value=""):
        assert AccountsAdmin.run(_parse("add", "root"), store) == 0
        assert AccountsAdmin.run(_parse("add", "bob"), store) == 0
    assert AccountsAdmin.run(_parse("promote", "bob"), store) == 0
    assert AccountsAdmin.run(_parse("demote", "root"), store) == 0
    assert AccountsAdmin.run(_parse("demote", "bob"), store) == 1
    assert "bob is the last admin of box." in capsys.readouterr().out
    assert AccountsAdmin.run(_parse("list", "--sessions"), store) == 0
    assert "root\tuser" in capsys.readouterr().out
    assert AccountsAdmin.run(_parse("remove", "root"), store) == 0
    assert AccountsAdmin.run(_parse("remove", "ghost"), store) == 1


def test_accounts_admin_reset_key_export_import_usage(tmp_path, capsys):
    lines: list[str] = []
    store = _store(tmp_path, lines)
    store.create_user("u", "pw")
    assert AccountsAdmin.run(_parse("reset", "u"), store) == 0
    assert "One-time password" in capsys.readouterr().out
    assert AccountsAdmin.run(_parse("key", "u", "--name", "ci"), store) == 0
    out = capsys.readouterr().out
    assert "sk-ragon-" in out and "OPENAI_API_KEY" in out
    assert AccountsAdmin.run(_parse("key", "u", "--revoke", "ci"), store) == 0
    assert AccountsAdmin.run(_parse("key", "u", "--revoke", "ci"), store) == 1
    capsys.readouterr()
    assert AccountsAdmin.run(_parse("export"), store) == 0
    dump = tmp_path / "dump.json"
    dump.write_text(capsys.readouterr().out)
    other = AccountsStore(tmp_path / "b.db", audit_sink=lambda _l: None)
    assert AccountsAdmin.run(_parse("import", str(dump)), other) == 0
    store.record_usage("u", "ragon", 3, 4)
    assert AccountsAdmin.run(_parse("usage", "--days", "2"), store) == 0
    assert "1 requests" in capsys.readouterr().out
    assert AccountsAdmin.run(_parse(), store) == 2


def test_accounts_admin_log_reads_the_journal(capsys):
    with patch("subprocess.run") as run:
        run.return_value.stdout = "line\n"
        run.return_value.returncode = 0
        assert AccountsAdmin.run(_parse("log")) == 0
    assert run.call_args.args[0][:3] == ["journalctl", "--no-pager", "-t"]
    assert "line" in capsys.readouterr().out


def test_the_first_account_from_node_user_add_makes_the_node_primary(tmp_path, capsys, monkeypatch):
    from dreamference.accounts import accounts_migrate
    from dreamference.node.node_identity import NodeIdentity
    from dreamference.node.node_settings import NodeSettings

    monkeypatch.setattr(NodeIdentity, "read", classmethod(lambda cls: "0123abcd-node"))
    advertised = []
    monkeypatch.setattr(
        accounts_migrate.AccountsMigrate, "advertise", classmethod(lambda cls: advertised.append(1))
    )
    store = _store(tmp_path, [])
    with patch("getpass.getpass", return_value=""):
        assert AccountsAdmin.run(_parse("add", "root"), store) == 0
        assert "This node is now the primary" in capsys.readouterr().out
        assert NodeSettings.auth_primary() and advertised == [1]
        assert AccountsAdmin.run(_parse("add", "bob"), store) == 0
    assert "primary" not in capsys.readouterr().out
    assert advertised == [1]
    assert [u["username"] for u in store.list_users() if u["service"]] == ["ragon-node-0123abcd"]


@pytest.mark.parametrize(
    "argv",
    [
        ("add", "root"),
        ("key", "root"),
        ("reset", "root"),
        ("remove", "root"),
        ("promote", "root"),
        ("demote", "root"),
        ("import", "users.json"),
        ("list",),
        ("usage",),
        ("export",),
    ],
)
def test_node_user_on_a_secondary_is_refused_and_opens_no_store(argv, capsys):
    from dreamference.accounts.accounts_store import AccountsStore as Store
    from dreamference.node.node_settings import NodeSettings

    NodeSettings.set_pinned_primary("6f1d2c3b-4a59-4e68-9f70-8a1b2c3d4e5f")
    with patch("getpass.getpass", side_effect=AssertionError("asked for a password")):
        assert AccountsAdmin.run(_parse(*argv)) == 1
    out = capsys.readouterr().out
    assert "secondary of 6f1d2c3b-4a59-4e68-9f70-8a1b2c3d4e5f" in out and "Created" not in out
    assert not Store.default_path().exists()
    assert not NodeSettings.auth_primary()


def test_node_user_add_without_a_node_id_says_what_to_run(tmp_path, capsys, monkeypatch):
    from dreamference.node.node_identity import NodeIdentity

    monkeypatch.setattr(NodeIdentity, "read", classmethod(lambda cls: None))
    store = _store(tmp_path, [])
    with patch("getpass.getpass", return_value=""):
        assert AccountsAdmin.run(_parse("add", "root"), store) == 0
    assert "run ragon-admin node migrate" in capsys.readouterr().out
