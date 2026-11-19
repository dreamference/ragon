"""The accounts store: rules, hashing, roles, tokens, keys and the audit lines."""

import stat

import pytest

from dreamference.accounts import AccountsError, AccountsStore
from dreamference.config.ports import ACCOUNTS_PORT


@pytest.fixture
def lines():
    return []


@pytest.fixture
def store(tmp_path, lines):
    return AccountsStore(
        tmp_path / "state" / "accounts.db", primary_name="box", audit_sink=lines.append
    )


def test_accounts_port_is_18100():
    assert ACCOUNTS_PORT == 18100


def test_accounts_store_file_is_mode_0600(store):
    assert stat.S_IMODE(store.path.stat().st_mode) == 0o600


@pytest.mark.parametrize("name", ["", "a" * 33, "has space", "ü", "a/b"])
def test_accounts_store_rejects_bad_usernames(store, name):
    with pytest.raises(AccountsError):
        store.create_user(name, "x")


def test_accounts_store_username_unique_ignoring_case(store):
    store.create_user("Alice", "x")
    with pytest.raises(AccountsError):
        store.create_user("alice", "y")
    assert store.verify_password("ALICE", "x")["username"] == "Alice"


def test_accounts_store_password_length_and_empty_password(store):
    store.create_user("e", "")
    assert store.verify_password("e", "") is not None
    assert store.verify_password("e", "x") is None
    with pytest.raises(AccountsError):
        store.create_user("long", "p" * 257)
    store.create_user("max", "p" * 256)
    assert store.verify_password("max", "p" * 256)


def test_accounts_store_hash_is_argon2id(store):
    store.create_user("a", "secret")
    assert (
        store.db.execute("SELECT password_hash FROM users").fetchone()[0].startswith("$argon2id$")
    )


def test_accounts_store_first_non_service_account_is_admin(store):
    service = store.ensure_service_account("12345678-aaaa-bbbb-cccc-dddddddddddd")
    assert service["username"] == "ragon-node-12345678" and service["role"] == "user"
    assert store.create_user("first", "")["role"] == "admin"
    assert store.create_user("second", "")["role"] == "user"
    assert (
        store.ensure_service_account("12345678-aaaa-bbbb-cccc-dddddddddddd")["user_id"]
        == service["user_id"]
    )


def test_accounts_store_service_account_refused_at_password_grant(store):
    store.ensure_service_account("abcdef01-0000-0000-0000-000000000000")
    pw = store.db.execute("SELECT 1 FROM users WHERE service = 1").fetchone()
    assert pw and store.verify_password("ragon-node-abcdef01", "") is None


def test_accounts_store_last_admin_cannot_be_demoted_or_removed(store):
    store.create_user("root", "")
    with pytest.raises(AccountsError, match=r"^root is the last admin of box\.$"):
        store.set_role("root", "user")
    with pytest.raises(AccountsError, match=r"^root is the last admin of box\.$"):
        store.remove_user("root")
    store.create_user("two", "")
    store.set_role("two", "admin")
    store.set_role("root", "user")


def test_accounts_store_reset_sets_must_change_and_revokes_tokens(store):
    store.create_user("u", "old")
    token = store.issue_refresh_token("u", "cli", "m")
    assert store.refresh_token_valid(token)
    one_time = store.reset_password("u")
    assert not store.refresh_token_valid(token)
    assert store.verify_password("u", "old") is None
    assert store.verify_password("u", one_time)["must_change"] is True
    store.set_password("u", "new")
    assert store.verify_password("u", "new")["must_change"] is False


def test_accounts_store_remove_revokes_everything(store):
    store.create_user("root", "")
    store.create_user("u", "p")
    token = store.issue_refresh_token("u", "cli", "m")
    key = store.mint_key("u")
    store.remove_user("u")
    assert not store.refresh_token_valid(token)
    assert store.verify_key(key) is None
    store.create_user("u", "again")


def test_accounts_store_keys_are_hashed_and_revocable_by_name(store):
    store.create_user("u", "")
    key = store.mint_key("u", "ci")
    assert key.startswith("sk-ragon-")
    assert (
        store.db.execute("SELECT COUNT(*) FROM keys WHERE key_hash = ?", (key,)).fetchone()[0] == 0
    )
    assert store.db.execute("SELECT key_hash FROM keys").fetchone()[0] == store._hash(key)
    assert store.verify_key(key)["username"] == "u"
    with pytest.raises(AccountsError):
        store.mint_key("u", "ci")
    store.revoke_key("u", "ci")
    assert store.verify_key(key) is None


def test_accounts_store_usage_and_export_import(store, tmp_path):
    store.create_user("u", "pw")
    key = store.mint_key("u")
    store.record_usage("u", "ragon", 10, 5)
    store.record_usage("u", "ragon", 1, 1)
    assert store.usage(1)[0]["requests"] == 2 and store.usage(1)[0]["input_tokens"] == 11
    token = store.issue_refresh_token("u", "cli", "m")
    dump = store.export_json()
    assert token not in dump and key not in dump
    other = AccountsStore(tmp_path / "o.db", audit_sink=lambda _l: None)
    assert other.import_json(dump) == 1
    assert other.verify_password("u", "pw") and other.verify_key(key)
    assert other.import_json(dump) == 0
    with pytest.raises(AccountsError):
        other.import_json("nope")


def test_accounts_store_audit_lines_carry_no_secret(store, lines):
    store.create_user("u", "hunter2-pw")
    store.verify_password("u", "hunter2-pw")
    store.verify_password("u", "wrong-pw")
    token = store.issue_refresh_token("u", "cli", "m")
    key = store.mint_key("u")
    one_time = store.reset_password("u")
    store.set_password("u", "brandnew-pw")
    store.revoke_key("u", "default")
    journal = "\n".join(lines)
    assert "event=signin" in journal and "result=invalid_grant" in journal
    for secret in ("hunter2-pw", "wrong-pw", "brandnew-pw", token, key, one_time, "$argon2"):
        assert secret not in journal
