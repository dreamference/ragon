"""The primary node's account store (specs/DREAMFERENCE_AUTH.md §9, §12, §13).

One SQLite file under the node's state directory, mode 0600. Passwords are hashed with argon2id
(the empty password too), refresh tokens and API keys are stored as SHA-256 only, and an audit
line never carries a password, a token or a key.
"""

import base64
import contextlib
import hashlib
import json
import os
import re
import secrets
import socket
import sqlite3
import subprocess
import time
import uuid
from collections.abc import Callable
from pathlib import Path
from typing import Any, Final

from argon2 import PasswordHasher
from argon2.exceptions import InvalidHashError, VerificationError

from dreamference.accounts.accounts_error import AccountsError

ROLE_ADMIN: Final[str] = "admin"
ROLE_USER: Final[str] = "user"
USERNAME_PATTERN: Final[re.Pattern[str]] = re.compile(r"^[A-Za-z0-9._-]{1,32}$")
MAX_PASSWORD_LENGTH: Final[int] = 256
KEY_PREFIX: Final[str] = "sk-ragon-"
SERVICE_PREFIX: Final[str] = "ragon-node-"
AUDIT_TAG: Final[str] = "ragon-accounts-audit"
STORE_MODE: Final[int] = 0o600

SCHEMA: Final[str] = """
CREATE TABLE IF NOT EXISTS users (
    user_id TEXT PRIMARY KEY,
    username TEXT NOT NULL,
    username_lower TEXT NOT NULL,
    password_hash TEXT NOT NULL,
    role TEXT NOT NULL,
    service INTEGER NOT NULL DEFAULT 0,
    must_change INTEGER NOT NULL DEFAULT 0,
    created REAL NOT NULL,
    removed REAL
);
CREATE UNIQUE INDEX IF NOT EXISTS users_name ON users(username_lower) WHERE removed IS NULL;
CREATE TABLE IF NOT EXISTS tokens (
    token_hash TEXT PRIMARY KEY, user_id TEXT NOT NULL, client_id TEXT NOT NULL,
    machine TEXT NOT NULL, created REAL NOT NULL, last_use REAL, revoked REAL
);
CREATE TABLE IF NOT EXISTS keys (
    key_hash TEXT PRIMARY KEY, user_id TEXT NOT NULL, key_name TEXT NOT NULL,
    created REAL NOT NULL, last_use REAL, revoked REAL
);
CREATE UNIQUE INDEX IF NOT EXISTS keys_name ON keys(user_id, key_name) WHERE revoked IS NULL;
CREATE TABLE IF NOT EXISTS device_codes (
    code_hash TEXT PRIMARY KEY, user_code TEXT NOT NULL, client_id TEXT NOT NULL,
    expires REAL NOT NULL, approved_user_id TEXT, consumed INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS bindings (
    chat_kind TEXT NOT NULL, chat_identity TEXT NOT NULL, user_id TEXT NOT NULL,
    created REAL NOT NULL, PRIMARY KEY (chat_kind, chat_identity)
);
CREATE TABLE IF NOT EXISTS pairing_codes (
    code TEXT PRIMARY KEY, user_id TEXT NOT NULL, chat_kind TEXT NOT NULL,
    expires REAL NOT NULL, consumed INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS usage (
    day TEXT NOT NULL, user_id TEXT NOT NULL, client_kind TEXT NOT NULL,
    requests INTEGER NOT NULL DEFAULT 0, input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0, PRIMARY KEY (day, user_id, client_kind)
);
"""


class AccountsStore:
    """The accounts of one primary node."""

    _hasher: PasswordHasher = PasswordHasher()

    def __init__(
        self,
        path: Path | None = None,
        primary_name: str | None = None,
        audit_sink: Callable[[str], None] | None = None,
    ) -> None:
        """Opens (creating it, mode 0600) the store.

        Args:
            path: The SQLite file; defaults to `default_path()`.
            primary_name: The node's name for messages; defaults to the host name.
            audit_sink: Receives each audit line; defaults to the journal.
        """
        self.path = path or self.default_path()
        self.primary_name = primary_name or socket.gethostname()
        self.audit_sink = audit_sink or self._journal
        self.path.parent.mkdir(parents=True, exist_ok=True)
        fd = os.open(self.path, os.O_CREAT | os.O_RDWR, STORE_MODE)
        os.close(fd)
        self.path.chmod(STORE_MODE)
        self.db = sqlite3.connect(self.path)
        self.db.row_factory = sqlite3.Row
        self.db.executescript(SCHEMA)
        self.db.commit()

    @classmethod
    def default_path(cls) -> Path:
        """Returns the store's path under the node's state directory.

        Returns:
            Path: `~/.local/share/dreamference/accounts/accounts.db`.
        """
        return Path.home() / ".local" / "share" / "dreamference" / "accounts" / "accounts.db"

    @staticmethod
    def _journal(line: str) -> None:
        """Writes one audit line to the journal through `logger`; a missing tool is not an error."""
        with contextlib.suppress(OSError, subprocess.SubprocessError):
            subprocess.run(["logger", "-t", AUDIT_TAG, line], check=False, timeout=5)

    @staticmethod
    def _hash(value: str) -> str:
        return hashlib.sha256(value.encode()).hexdigest()

    def audit(self, event: str, username: str, client: str, result: str) -> None:
        """Emits one audit line: username, event, client kind, result. Never a secret.

        Args:
            event: What happened (signin, create, reset, revoke, failure, pairing, ...).
            username: The account concerned.
            client: The client kind (cli, ragon, web, ...).
            result: ok or the failure kind.
        """
        clean = lambda text: re.sub(r"\s+", "_", str(text))  # noqa: E731
        self.audit_sink(
            f"user={clean(username)} event={clean(event)} client={clean(client)} result={clean(result)}"
        )

    @classmethod
    def check_username(cls, name: str) -> None:
        """Raises AccountsError unless the name is 1-32 characters of A-Za-z0-9._-."""
        if not USERNAME_PATTERN.match(name):
            raise AccountsError(
                "A username is 1 to 32 characters of letters, digits, '.', '_' and '-'."
            )

    @classmethod
    def check_password(cls, password: str) -> None:
        """Raises AccountsError when the password is longer than 256 characters (empty is fine)."""
        if len(password) > MAX_PASSWORD_LENGTH:
            raise AccountsError(f"A password is at most {MAX_PASSWORD_LENGTH} characters.")

    def _row(self, name: str) -> sqlite3.Row:
        row = self.db.execute(
            "SELECT * FROM users WHERE username_lower = ? AND removed IS NULL", (name.lower(),)
        ).fetchone()
        if row is None:
            raise AccountsError(f"No user named {name}.")
        return row

    def _admins(self) -> int:
        return self.db.execute(
            "SELECT COUNT(*) FROM users WHERE role = ? AND removed IS NULL", (ROLE_ADMIN,)
        ).fetchone()[0]

    def _public(self, row: sqlite3.Row) -> dict[str, Any]:
        return {
            "user_id": row["user_id"],
            "username": row["username"],
            "role": row["role"],
            "service": bool(row["service"]),
            "must_change": bool(row["must_change"]),
            "created": row["created"],
        }

    def create_user(self, name: str, password: str, service: bool = False) -> dict[str, Any]:
        """Creates a user. The first account that is not a service account becomes admin.

        Args:
            name: The username (case-insensitively unique).
            password: 0-256 characters; empty is allowed.
            service: True for a node's service account.

        Returns:
            dict[str, Any]: The new user (no hash).

        Raises:
            AccountsError: On a bad name or password, or a name already taken.
        """
        self.check_username(name)
        self.check_password(password)
        if self.db.execute(
            "SELECT 1 FROM users WHERE username_lower = ? AND removed IS NULL", (name.lower(),)
        ).fetchone():
            raise AccountsError(f"{name} already exists.")
        has_admin = self._admins() > 0
        role = ROLE_ADMIN if not service and not has_admin else ROLE_USER
        user_id = uuid.uuid4().hex
        self.db.execute(
            "INSERT INTO users (user_id, username, username_lower, password_hash, role, service,"
            " must_change, created) VALUES (?, ?, ?, ?, ?, ?, 0, ?)",
            (
                user_id,
                name,
                name.lower(),
                self._hasher.hash(password),
                role,
                int(service),
                time.time(),
            ),
        )
        self.db.commit()
        self.audit("create", name, "admin", "ok")
        return self._public(self._row(name))

    def verify_password(
        self, name: str, password: str, client: str = "cli"
    ) -> dict[str, Any] | None:
        """Checks a password sign-in; a service account is refused here.

        Args:
            name: The username (any case).
            password: The password, possibly empty.
            client: The client kind, for the audit line.

        Returns:
            dict[str, Any] | None: The user, or None when the sign-in fails.
        """
        row = self.db.execute(
            "SELECT * FROM users WHERE username_lower = ? AND removed IS NULL", (name.lower(),)
        ).fetchone()
        if row is None or row["service"]:
            self.audit("signin", name, client, "invalid_grant")
            return None
        try:
            self._hasher.verify(row["password_hash"], password)
        except (VerificationError, InvalidHashError):
            self.audit("signin", name, client, "invalid_grant")
            return None
        self.audit("signin", name, client, "ok")
        return self._public(row)

    def set_password(self, name: str, password: str) -> None:
        """Sets a new password and clears the must-change flag."""
        self.check_password(password)
        row = self._row(name)
        self.db.execute(
            "UPDATE users SET password_hash = ?, must_change = 0 WHERE user_id = ?",
            (self._hasher.hash(password), row["user_id"]),
        )
        self.db.commit()
        self.audit("password", row["username"], "user", "ok")

    def reset_password(self, name: str) -> str:
        """Sets a random one-time password, requires a change, revokes the refresh tokens.

        Args:
            name: The user.

        Returns:
            str: The one-time password, returned once and kept nowhere.
        """
        row = self._row(name)
        one_time = secrets.token_urlsafe(12)
        self.db.execute(
            "UPDATE users SET password_hash = ?, must_change = 1 WHERE user_id = ?",
            (self._hasher.hash(one_time), row["user_id"]),
        )
        self.db.execute(
            "UPDATE tokens SET revoked = ? WHERE user_id = ? AND revoked IS NULL",
            (time.time(), row["user_id"]),
        )
        self.db.commit()
        self.audit("reset", row["username"], "admin", "ok")
        return one_time

    def remove_user(self, name: str) -> None:
        """Removes a user and revokes tokens and keys, deleting bindings and pairing codes.

        Raises:
            AccountsError: When the user is the last admin.
        """
        row = self._row(name)
        if row["role"] == ROLE_ADMIN and self._admins() <= 1:
            raise AccountsError(f"{row['username']} is the last admin of {self.primary_name}.")
        now = time.time()
        uid = row["user_id"]
        self.db.execute("UPDATE users SET removed = ? WHERE user_id = ?", (now, uid))
        self.db.execute(
            "UPDATE tokens SET revoked = ? WHERE user_id = ? AND revoked IS NULL", (now, uid)
        )
        self.db.execute(
            "UPDATE keys SET revoked = ? WHERE user_id = ? AND revoked IS NULL", (now, uid)
        )
        self.db.execute("DELETE FROM bindings WHERE user_id = ?", (uid,))
        self.db.execute("DELETE FROM pairing_codes WHERE user_id = ?", (uid,))
        self.db.commit()
        self.audit("remove", row["username"], "admin", "ok")

    def set_role(self, name: str, role: str) -> None:
        """Promotes or demotes a user; the last admin cannot be demoted.

        Raises:
            AccountsError: When demoting the last admin.
        """
        row = self._row(name)
        if role != ROLE_ADMIN and row["role"] == ROLE_ADMIN and self._admins() <= 1:
            raise AccountsError(f"{row['username']} is the last admin of {self.primary_name}.")
        self.db.execute("UPDATE users SET role = ? WHERE user_id = ?", (role, row["user_id"]))
        self.db.commit()
        self.audit("role", row["username"], "admin", role)

    def issue_refresh_token(self, name: str, client_id: str, machine: str) -> str:
        """Mints a refresh token (32 random bytes, base64url), stored as SHA-256.

        Returns:
            str: The token, shown once.
        """
        row = self._row(name)
        token = base64.urlsafe_b64encode(secrets.token_bytes(32)).decode().rstrip("=")
        self.db.execute(
            "INSERT INTO tokens (token_hash, user_id, client_id, machine, created) VALUES (?, ?, ?, ?, ?)",
            (self._hash(token), row["user_id"], client_id, machine, time.time()),
        )
        self.db.commit()
        return token

    def refresh_token_valid(self, token: str) -> bool:
        """Returns True when the token is stored, not revoked, and its user still exists."""
        row = self.db.execute(
            "SELECT t.revoked, u.removed FROM tokens t JOIN users u ON u.user_id = t.user_id"
            " WHERE t.token_hash = ?",
            (self._hash(token),),
        ).fetchone()
        return row is not None and row["revoked"] is None and row["removed"] is None

    def mint_key(self, name: str, key_name: str = "default") -> str:
        """Mints an API key `sk-ragon-<32 random bytes, base64url>`; only its SHA-256 is stored.

        Raises:
            AccountsError: When the user already has a live key of that name.
        """
        row = self._row(name)
        if self.db.execute(
            "SELECT 1 FROM keys WHERE user_id = ? AND key_name = ? AND revoked IS NULL",
            (row["user_id"], key_name),
        ).fetchone():
            raise AccountsError(f"{row['username']} already has a key named {key_name}.")
        key = KEY_PREFIX + base64.urlsafe_b64encode(secrets.token_bytes(32)).decode().rstrip("=")
        self.db.execute(
            "INSERT INTO keys (key_hash, user_id, key_name, created) VALUES (?, ?, ?, ?)",
            (self._hash(key), row["user_id"], key_name, time.time()),
        )
        self.db.commit()
        self.audit("key", row["username"], "admin", "ok")
        return key

    def revoke_key(self, name: str, key_name: str) -> None:
        """Revokes the user's key of that name.

        Raises:
            AccountsError: When there is no such live key.
        """
        row = self._row(name)
        cursor = self.db.execute(
            "UPDATE keys SET revoked = ? WHERE user_id = ? AND key_name = ? AND revoked IS NULL",
            (time.time(), row["user_id"], key_name),
        )
        self.db.commit()
        if cursor.rowcount == 0:
            raise AccountsError(f"{row['username']} has no key named {key_name}.")
        self.audit("revoke", row["username"], "admin", "ok")

    def verify_key(self, key: str) -> dict[str, Any] | None:
        """Returns the key's user when the key is live and its user exists, else None."""
        row = self.db.execute(
            "SELECT u.* FROM keys k JOIN users u ON u.user_id = k.user_id"
            " WHERE k.key_hash = ? AND k.revoked IS NULL AND u.removed IS NULL",
            (self._hash(key),),
        ).fetchone()
        if row is None:
            return None
        self.db.execute(
            "UPDATE keys SET last_use = ? WHERE key_hash = ?", (time.time(), self._hash(key))
        )
        self.db.commit()
        return self._public(row)

    def ensure_service_account(self, node_id: str) -> dict[str, Any]:
        """Creates the node's service account `ragon-node-<8 hex>` if missing.

        Args:
            node_id: The node id (a UUID); its first 8 hex characters name the account.

        Returns:
            dict[str, Any]: The account.
        """
        name = SERVICE_PREFIX + node_id.replace("-", "")[:8].lower()
        try:
            return self._public(self._row(name))
        except AccountsError:
            return self.create_user(name, secrets.token_urlsafe(24), service=True)

    def record_usage(
        self, name: str, client_kind: str, input_tokens: int, output_tokens: int
    ) -> None:
        """Adds one request to today's usage row for the user and client kind."""
        row = self._row(name)
        day = time.strftime("%Y-%m-%d", time.gmtime())
        self.db.execute(
            "INSERT INTO usage (day, user_id, client_kind, requests, input_tokens, output_tokens)"
            " VALUES (?, ?, ?, 1, ?, ?) ON CONFLICT(day, user_id, client_kind) DO UPDATE SET"
            " requests = requests + 1, input_tokens = input_tokens + excluded.input_tokens,"
            " output_tokens = output_tokens + excluded.output_tokens",
            (day, row["user_id"], client_kind, input_tokens, output_tokens),
        )
        self.db.commit()

    def usage(self, days: int = 7) -> list[dict[str, Any]]:
        """Returns requests and tokens per user per day and client kind for the last N days."""
        cutoff = time.strftime("%Y-%m-%d", time.gmtime(time.time() - max(days, 1) * 86400))
        rows = self.db.execute(
            "SELECT u.username, s.day, s.client_kind, s.requests, s.input_tokens, s.output_tokens"
            " FROM usage s JOIN users u ON u.user_id = s.user_id WHERE s.day >= ?"
            " ORDER BY s.day, u.username, s.client_kind",
            (cutoff,),
        ).fetchall()
        return [dict(r) for r in rows]

    def list_users(self, sessions: bool = False) -> list[dict[str, Any]]:
        """Lists users with role, service flag, created and last use; optionally every token."""
        result = []
        for row in self.db.execute(
            "SELECT * FROM users WHERE removed IS NULL ORDER BY username_lower"
        ):
            entry = self._public(row)
            last = self.db.execute(
                "SELECT MAX(m) FROM (SELECT MAX(last_use) m FROM tokens WHERE user_id = ?"
                " UNION ALL SELECT MAX(last_use) FROM keys WHERE user_id = ?)",
                (row["user_id"], row["user_id"]),
            ).fetchone()[0]
            entry["last_use"] = last
            if sessions:
                entry["tokens"] = [
                    {"client": t["client_id"], "machine": t["machine"], "last_use": t["last_use"]}
                    for t in self.db.execute(
                        "SELECT * FROM tokens WHERE user_id = ? AND revoked IS NULL",
                        (row["user_id"],),
                    )
                ]
                entry["bindings"] = [
                    {"kind": b["chat_kind"], "identity": b["chat_identity"]}
                    for b in self.db.execute(
                        "SELECT * FROM bindings WHERE user_id = ?", (row["user_id"],)
                    )
                ]
            result.append(entry)
        return result

    def export_json(self) -> str:
        """Exports users, roles, password hashes and key hashes as JSON; never tokens."""
        users = []
        for row in self.db.execute("SELECT * FROM users WHERE removed IS NULL ORDER BY created"):
            entry = self._public(row)
            entry["password_hash"] = row["password_hash"]
            entry["keys"] = [
                {"key_hash": k["key_hash"], "key_name": k["key_name"], "created": k["created"]}
                for k in self.db.execute(
                    "SELECT * FROM keys WHERE user_id = ? AND revoked IS NULL", (row["user_id"],)
                )
            ]
            users.append(entry)
        return json.dumps({"version": 1, "users": users}, indent=2)

    def import_json(self, text: str) -> int:
        """Imports an export; users that already exist (any case) are skipped.

        Returns:
            int: The number of users added.

        Raises:
            AccountsError: When the text is not an export.
        """
        try:
            data = json.loads(text)
            users = data["users"]
        except (ValueError, KeyError, TypeError) as error:
            raise AccountsError("The file is not an accounts export.") from error
        added = 0
        for entry in users:
            self.check_username(entry["username"])
            if self.db.execute(
                "SELECT 1 FROM users WHERE username_lower = ? AND removed IS NULL",
                (entry["username"].lower(),),
            ).fetchone():
                continue
            self.db.execute(
                "INSERT INTO users (user_id, username, username_lower, password_hash, role, service,"
                " must_change, created) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                (
                    entry["user_id"],
                    entry["username"],
                    entry["username"].lower(),
                    entry["password_hash"],
                    entry["role"],
                    int(entry["service"]),
                    int(entry["must_change"]),
                    entry["created"],
                ),
            )
            for key in entry.get("keys", []):
                self.db.execute(
                    "INSERT OR IGNORE INTO keys (key_hash, user_id, key_name, created) VALUES (?, ?, ?, ?)",
                    (key["key_hash"], entry["user_id"], key["key_name"], key["created"]),
                )
            added += 1
        self.db.commit()
        return added
