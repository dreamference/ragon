"""Chat and phone bindings and pairing codes (specs/DREAMFERENCE_AUTH.md §9, §14.1).

A user asks for a 6-character pairing code for one chat kind (valid 60 minutes, consumed by its
first use); the user sends it to the bridge's bot, the bridge redeems it with its node's service
key, and the chat identity is bound to that user. One chat identity maps to exactly one user:
re-pairing moves it. Each binding has its own API key for the bridge, revoked with the binding.
"""

import contextlib
import hashlib
import secrets
import time
from typing import Any, Final

from dreamference.accounts.accounts_error import AccountsError
from dreamference.accounts.accounts_store import AccountsStore

CODE_LENGTH: Final[int] = 6
CODE_LIFETIME_S: Final[int] = 3600
# No 0/O or 1/I/L: a code is read off a screen and typed on a phone.
CODE_ALPHABET: Final[str] = "ABCDEFGHJKMNPQRSTUVWXYZ23456789"
CHAT_KINDS: Final[tuple[str, ...]] = ("signal", "telegram", "discord", "matrix")
MAX_IDENTITY: Final[int] = 256
KEY_PREFIX: Final[str] = "bridge-"
SEPARATORS: Final[str] = " -"


class AccountsBindings:
    """Pairing codes and bindings over one `AccountsStore`."""

    def __init__(self, store: AccountsStore) -> None:
        """Binds to the store (its `bindings` and `pairing_codes` tables).

        Args:
            store: The account store.
        """
        self.store = store

    @classmethod
    def binding_id(cls, kind: str, identity: str) -> str:
        """A stable, URL-safe id for one binding."""
        return hashlib.sha256(f"{kind}\0{identity}".encode()).hexdigest()[:16]

    @classmethod
    def key_name(cls, kind: str, identity: str) -> str:
        """The name of the bridge key minted for a binding."""
        return KEY_PREFIX + cls.binding_id(kind, identity)

    @classmethod
    def check_kind(cls, kind: object) -> str:
        """Returns a valid chat kind or raises."""
        if not isinstance(kind, str) or kind not in CHAT_KINDS:
            raise AccountsError("The chat kind must be one of " + ", ".join(CHAT_KINDS) + ".")
        return kind

    @classmethod
    def normalize(cls, code: str) -> str:
        """Upper-cases a typed code and drops spaces and dashes."""
        return "".join(ch for ch in code.upper() if ch not in SEPARATORS)

    def create_code(self, user_id: str, kind: str) -> dict[str, Any]:
        """Makes a pairing code for the user and chat kind.

        Args:
            user_id: The user the chat will bind to.
            kind: The chat kind.

        Returns:
            dict[str, Any]: `code`, `chat_kind` and `expires_in` seconds.
        """
        kind = self.check_kind(kind)
        user = self.store.user_by_id(user_id)
        if user is None:
            raise AccountsError("No such user.")
        now = time.time()
        self.store.db.execute("DELETE FROM pairing_codes WHERE expires < ?", (now,))
        code = "".join(secrets.choice(CODE_ALPHABET) for _ in range(CODE_LENGTH))
        self.store.db.execute(
            "INSERT INTO pairing_codes (code, user_id, chat_kind, expires) VALUES (?, ?, ?, ?)",
            (code, user_id, kind, now + CODE_LIFETIME_S),
        )
        self.store.db.commit()
        self.store.audit("pairing-code", user["username"], kind, "ok")
        return {"code": code, "chat_kind": kind, "expires_in": CODE_LIFETIME_S}

    def redeem(self, code: str, kind: str, identity: str) -> dict[str, Any]:
        """Consumes a code and binds the chat identity to the code's user.

        Args:
            code: The pairing code the user sent to the bot.
            kind: The chat kind of the bot that received it.
            identity: The chat id (Matrix user id, Telegram chat id, …).

        Returns:
            dict[str, Any]: The binding, the user's name and the bridge key (shown once).

        Raises:
            AccountsError: When the code is unknown, used, expired or for another chat kind.
        """
        kind = self.check_kind(kind)
        if not identity or len(identity) > MAX_IDENTITY:
            raise AccountsError("The chat identity is missing or too long.")
        db = self.store.db
        row = db.execute(
            "SELECT * FROM pairing_codes WHERE code = ?", (self.normalize(code),)
        ).fetchone()
        if (
            row is None
            or row["consumed"]
            or row["expires"] < time.time()
            or row["chat_kind"] != kind
        ):
            self.store.audit("pairing", "-", kind, "failed")
            raise AccountsError("That pairing code is not valid.")
        user = self.store.user_by_id(row["user_id"])
        if user is None:
            raise AccountsError("That pairing code is not valid.")
        db.execute("UPDATE pairing_codes SET consumed = 1 WHERE code = ?", (row["code"],))
        previous = db.execute(
            "SELECT user_id FROM bindings WHERE chat_kind = ? AND chat_identity = ?",
            (kind, identity),
        ).fetchone()
        if previous is not None:
            self._revoke_key(previous["user_id"], kind, identity)
        db.execute(
            "INSERT INTO bindings (chat_kind, chat_identity, user_id, created) VALUES (?, ?, ?, ?)"
            " ON CONFLICT(chat_kind, chat_identity) DO UPDATE SET user_id = excluded.user_id,"
            " created = excluded.created",
            (kind, identity, user["user_id"], time.time()),
        )
        db.commit()
        key = self.store.mint_key(user["username"], self.key_name(kind, identity))
        self.store.audit("pairing", user["username"], kind, "ok")
        return {
            "id": self.binding_id(kind, identity),
            "chat_kind": kind,
            "chat_identity": identity,
            "username": user["username"],
            "key": key,
        }

    def list(self, user_id: str) -> list[dict[str, Any]]:
        """The user's bindings, newest first."""
        rows = self.store.db.execute(
            "SELECT chat_kind, chat_identity, created FROM bindings WHERE user_id = ?"
            " ORDER BY created DESC",
            (user_id,),
        )
        return [
            {
                "id": self.binding_id(r["chat_kind"], r["chat_identity"]),
                "chat_kind": r["chat_kind"],
                "chat_identity": r["chat_identity"],
                "created": r["created"],
            }
            for r in rows
        ]

    def remove(self, user_id: str, binding_id: str) -> bool:
        """Removes one of the user's bindings and revokes its bridge key.

        Returns:
            bool: False when the user has no such binding.
        """
        for item in self.list(user_id):
            if item["id"] == binding_id:
                self.store.db.execute(
                    "DELETE FROM bindings WHERE chat_kind = ? AND chat_identity = ?",
                    (item["chat_kind"], item["chat_identity"]),
                )
                self.store.db.commit()
                self._revoke_key(user_id, item["chat_kind"], item["chat_identity"])
                user = self.store.user_by_id(user_id)
                self.store.audit(
                    "unpair", user["username"] if user else "-", item["chat_kind"], "ok"
                )
                return True
        return False

    def _revoke_key(self, user_id: str, kind: str, identity: str) -> None:
        user = self.store.user_by_id(user_id)
        if user is None:
            return
        with contextlib.suppress(AccountsError):
            self.store.revoke_key(user["username"], self.key_name(kind, identity))
