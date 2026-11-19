"""Device authorization and authorization codes of the primary (specs/DREAMFERENCE_AUTH.md §5.4, §5.5).

One table of codes serves two request shapes: Codex's (`/api/accounts/deviceauth/usercode` and
`/token`, then the authorization-code exchange at `/oauth/token` with PKCE) and RFC 8628's
(`/oauth/device/code`, then the device-code grant). A code lives 15 minutes, the poll interval is
5 seconds and the user code is 8 characters without look-alike letters. The table is in memory:
a restart ends every pending sign-in, which the owner can start again. The same table holds the
authorization codes of the Forgejo client (`AccountsForgejoClient`).
"""

import base64
import hashlib
import secrets
import threading
import time
from collections.abc import Callable
from typing import Final

from dreamference.accounts.accounts_auth_code import AuthCode
from dreamference.accounts.accounts_device_entry import DeviceEntry
from dreamference.accounts.accounts_forgejo_client import AccountsForgejoClient

CODE_LIFETIME_S: Final[int] = 900
POLL_INTERVAL_S: Final[int] = 5
USER_CODE_LENGTH: Final[int] = 8
USER_CODE_ALPHABET: Final[str] = "BCDFGHJKMNPQRSTVWXZ2346789"
WORD_PENDING: Final[str] = "authorization_pending"
WORD_OK: Final[str] = "ok"
GRANT_DEVICE: Final[str] = "urn:ietf:params:oauth:grant-type:device_code"
GRANT_AUTH_CODE: Final[str] = "authorization_code"
SHAPE_CODEX: Final[str] = "codex"
SHAPE_STANDARD: Final[str] = "standard"
CALLBACK_PATH: Final[str] = "/deviceauth/callback"


class AccountsDevice:
    """The table of device codes and authorization codes."""

    def __init__(
        self,
        issuer: str,
        verification_uri: str,
        clock: Callable[[], float] = time.time,
        forgejo: AccountsForgejoClient | None = None,
    ) -> None:
        """Binds the table to its issuer and the page that approves codes.

        Args:
            issuer: The primary's issuer URL; Codex's callback is `{issuer}/deviceauth/callback`.
            verification_uri: The page where a person enters the code (`.../device`).
            clock: Seconds since the epoch; tests pass their own.
            forgejo: The Forgejo client registered on the primary, if any.
        """
        self.issuer = issuer.rstrip("/")
        self.verification_uri = verification_uri
        self.clock = clock
        self.forgejo = forgejo
        self.lock = threading.Lock()
        self.entries: dict[str, DeviceEntry] = {}
        self.auth_codes: dict[str, AuthCode] = {}

    # ----- helpers ------------------------------------------------------------------------

    @classmethod
    def new_user_code(cls) -> str:
        """Eight characters from the alphabet without look-alikes."""
        return "".join(secrets.choice(USER_CODE_ALPHABET) for _ in range(USER_CODE_LENGTH))

    @classmethod
    def display_code(cls, user_code: str) -> str:
        """The code as two groups of four, `ABCD-EFGH`."""
        return f"{user_code[:4]}-{user_code[4:]}"

    @classmethod
    def normalize(cls, user_code: str) -> str:
        """The code without dashes or spaces, upper case."""
        return "".join(ch for ch in user_code.upper() if ch.isalnum())

    @classmethod
    def s256(cls, verifier: str) -> str:
        """The PKCE S256 challenge of a verifier."""
        digest = hashlib.sha256(verifier.encode("ascii")).digest()
        return base64.urlsafe_b64encode(digest).rstrip(b"=").decode()

    def purge(self) -> None:
        """Drops expired rows (call with the lock held)."""
        now = self.clock()
        for key in [k for k, e in self.entries.items() if e.expires_at <= now]:
            del self.entries[key]
        for key in [k for k, a in self.auth_codes.items() if a.expires_at <= now]:
            del self.auth_codes[key]

    def by_user_code(self, user_code: str) -> DeviceEntry | None:
        """The live row for a user code, or None (call with the lock held)."""
        wanted = self.normalize(user_code)
        for entry in self.entries.values():
            if entry.user_code == wanted:
                return entry
        return None

    # ----- start --------------------------------------------------------------------------

    def start(self, client_id: str, shape: str) -> DeviceEntry:
        """Opens a device sign-in.

        Args:
            client_id: The client asking.
            shape: `SHAPE_CODEX` or `SHAPE_STANDARD`.

        Returns:
            DeviceEntry: The new row.
        """
        with self.lock:
            self.purge()
            code = self.new_user_code()
            while self.by_user_code(code) is not None:
                code = self.new_user_code()
            verifier = secrets.token_urlsafe(48)
            entry = DeviceEntry(
                device_id=f"deviceauth_{secrets.token_urlsafe(16)}",
                device_code=secrets.token_urlsafe(32),
                user_code=code,
                client_id=client_id,
                shape=shape,
                expires_at=self.clock() + CODE_LIFETIME_S,
                verifier=verifier,
                challenge=self.s256(verifier),
            )
            self.entries[entry.device_id] = entry
            return entry

    def codex_usercode(self, client_id: str) -> dict[str, str]:
        """`POST /api/accounts/deviceauth/usercode`: Codex's answer (interval is a string)."""
        entry = self.start(client_id, SHAPE_CODEX)
        return {
            "device_auth_id": entry.device_id,
            "user_code": self.display_code(entry.user_code),
            "interval": str(POLL_INTERVAL_S),
        }

    def standard_code(self, client_id: str) -> dict[str, object]:
        """`POST /oauth/device/code`: RFC 8628's answer."""
        entry = self.start(client_id, SHAPE_STANDARD)
        shown = self.display_code(entry.user_code)
        return {
            "device_code": entry.device_code,
            "user_code": shown,
            "verification_uri": self.verification_uri,
            "verification_uri_complete": f"{self.verification_uri}?user_code={shown}",
            "expires_in": CODE_LIFETIME_S,
            "interval": POLL_INTERVAL_S,
        }

    # ----- the page -----------------------------------------------------------------------

    def exists(self, user_code: str) -> bool:
        """True when a live, undecided code matches (the page's first step)."""
        with self.lock:
            self.purge()
            entry = self.by_user_code(user_code)
            return entry is not None and entry.user is None and not entry.denied

    def approve(self, user_code: str, user_id: str, machine: str = "") -> bool:
        """Records that the user with `user_id` approved the code; True when a live code matched."""
        with self.lock:
            self.purge()
            entry = self.by_user_code(user_code)
            if entry is None or entry.denied:
                return False
            entry.user = user_id
            entry.machine = machine
            if entry.shape == SHAPE_CODEX:
                entry.auth_code = secrets.token_urlsafe(32)
                self.auth_codes[entry.auth_code] = AuthCode(
                    user_id,
                    entry.client_id,
                    f"{self.issuer}{CALLBACK_PATH}",
                    entry.challenge,
                    entry.expires_at,
                    machine,
                )
            return True

    def deny(self, user_code: str) -> bool:
        """Records that the person refused the code."""
        with self.lock:
            self.purge()
            entry = self.by_user_code(user_code)
            if entry is None:
                return False
            entry.denied = True
            return True

    # ----- polling ------------------------------------------------------------------------

    def codex_poll(self, device_id: str, user_code: str) -> tuple[int, dict[str, str]]:
        """`POST /api/accounts/deviceauth/token`: 403 while pending, then the code and PKCE pair."""
        with self.lock:
            self.purge()
            entry = self.entries.get(device_id)
            if (
                entry is None
                or entry.shape != SHAPE_CODEX
                or entry.user_code != self.normalize(user_code)
            ):
                return 404, {}
            if entry.denied:
                return 400, {"error": "access_denied"}
            if entry.auth_code is None:
                return 403, {}
            del self.entries[device_id]
            return 200, {
                "authorization_code": entry.auth_code,
                "code_challenge": entry.challenge,
                "code_verifier": entry.verifier,
            }

    @classmethod
    def pending_word(cls, entry: DeviceEntry) -> str:
        """The answer for a polled row: denied, approved or still pending."""
        if entry.denied:
            return "access_denied"
        return WORD_PENDING if entry.user is None else WORD_OK

    def device_grant(self, device_code: str, client_id: str) -> tuple[str, DeviceEntry | None]:
        """The device-code grant: an RFC 8628 error word, or `ok` with the approved row.

        Args:
            device_code: What `/oauth/device/code` answered.
            client_id: The client polling.

        Returns:
            tuple[str, DeviceEntry | None]: `authorization_pending`, `slow_down`, `expired_token`,
                `access_denied`, `invalid_grant` or `ok` with the row (consumed).
        """
        with self.lock:
            now = self.clock()
            entry = next((e for e in self.entries.values() if e.device_code == device_code), None)
            if entry is None:
                return "expired_token", None
            if entry.expires_at <= now:
                del self.entries[entry.device_id]
                return "expired_token", None
            if entry.client_id != client_id or entry.shape != SHAPE_STANDARD:
                return "invalid_grant", None
            fast = bool(entry.last_poll) and now - entry.last_poll < POLL_INTERVAL_S
            entry.last_poll = now
            word = "slow_down" if fast else self.pending_word(entry)
            if word != WORD_PENDING and not fast:
                del self.entries[entry.device_id]
            return word, entry if word == WORD_OK else None

    # ----- authorization codes ------------------------------------------------------------

    def issue_forgejo_code(
        self, user_id: str, client_id: str, redirect_uri: str, challenge: str = ""
    ) -> str | None:
        """A code for the Forgejo client after sign-in; None when the client or URI is not known."""
        if self.forgejo is None or client_id != self.forgejo.client_id:
            return None
        if not self.forgejo.allows(redirect_uri):
            return None
        code = secrets.token_urlsafe(32)
        with self.lock:
            self.purge()
            self.auth_codes[code] = AuthCode(
                user_id, client_id, redirect_uri, challenge, self.clock() + CODE_LIFETIME_S
            )
        return code

    def redeem(
        self, code: str, client_id: str, redirect_uri: str, verifier: str, secret: str = ""
    ) -> tuple[str | None, AuthCode | None]:
        """The authorization-code grant: the code is used once, whatever the outcome.

        Args:
            code: The authorization code.
            client_id: The client exchanging it.
            redirect_uri: Must equal the one the code was issued for.
            verifier: The PKCE verifier; checked against the stored challenge when there is one.
            secret: The Forgejo client's secret, when it sends one.

        Returns:
            tuple[str | None, AuthCode | None]: An error description, or None with the code row.
        """
        with self.lock:
            self.purge()
            row = self.auth_codes.pop(code, None)
        if row is None:
            return "The authorization code is not valid.", None
        if row.client_id != client_id or row.redirect_uri != redirect_uri:
            return "The client or the redirect URI does not match the code.", None
        if row.challenge:
            if not verifier or not secrets.compare_digest(self.s256(verifier), row.challenge):
                return "The code verifier does not match.", None
            return None, row
        forgejo = self.forgejo
        checked = forgejo is not None and client_id == forgejo.client_id
        if checked and not (secret and forgejo and forgejo.secret_matches(secret)):
            return "The client secret does not match.", None
        return None, row
