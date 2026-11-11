# ruff: noqa: PLR2004 - field kinds, levels and byte widths are the spec's literals
"""Redaction at the writer (LOGGING §7.4): once, before a line exists, from one set of rules."""

import getpass
import hashlib
import hmac
import ipaddress
import re
import socket
from collections import Counter
from pathlib import Path
from typing import Final
from urllib.parse import urlsplit

SECRET_SUFFIXES: Final[frozenset[str]] = frozenset(
    {"token", "key", "secret", "password", "passwd", "cookie", "credential", "auth"}
)
SECRET: Final[str] = "[secret]"
ADDR: Final[str] = "[addr]"

# name | keyword that must appear | pattern. The same shapes are in ragon-rs/vectors/logging/secrets.txt,
# which the Rust and C++ writers read; tests/test_logging_writer.py checks the two are equal.
TOKEN_SHAPES: Final[tuple[tuple[str, str, str], ...]] = (
    ("github", "gh", r"\bgh[pousr]_[A-Za-z0-9]{20,}"),
    ("github-pat", "github_pat_", r"\bgithub_pat_[A-Za-z0-9_]{20,}"),
    ("slack", "xox", r"\bxox[abposr]-[A-Za-z0-9-]{10,}"),
    ("openai", "sk-", r"\bsk-(?:ant-|proj-)?[A-Za-z0-9_-]{20,}"),
    ("jwt", "eyJ", r"\beyJ[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}"),
    ("aws", "A", r"\b(?:AKIA|ASIA)[A-Z0-9]{16}\b"),
    ("google", "AIza", r"\bAIza[A-Za-z0-9_-]{35}\b"),
    ("gitlab", "glpat-", r"\bglpat-[A-Za-z0-9_-]{20,}"),
    ("npm", "npm_", r"\bnpm_[A-Za-z0-9]{36}\b"),
    ("pypi", "pypi-", r"\bpypi-[A-Za-z0-9_-]{20,}"),
    ("huggingface", "hf_", r"\bhf_[A-Za-z0-9]{30,}"),
    ("telegram", ":", r"\b[0-9]{8,10}:[A-Za-z0-9_-]{35}\b"),
)

_KEY_BLOCK = re.compile(r"-----BEGIN [A-Z0-9 ]+-----.*?-----END [A-Z0-9 ]+-----", re.DOTALL)
_AUTH_HEADER = re.compile(r"(?i)\bAuthorization\s*:[^\r\n]*")
_BEARER = re.compile(r"(?i)\b(?:Bearer|Basic)\s+[A-Za-z0-9._~+/=-]{8,}")
_FLAG = re.compile(r"(?<![\w-])(--?[A-Za-z][A-Za-z0-9_-]*)(=|[ \t]+)([^\s\"']+|\"[^\"]*\"|'[^']*')")
_PAIR = re.compile(r"(?<![\w-])([A-Za-z][A-Za-z0-9_-]*)=(\"[^\"]*\"|'[^']*'|[^\s\"',;&]+)")
_JSON_PAIR = re.compile(r"\"([A-Za-z][A-Za-z0-9_-]*)\"(\s*:\s*)\"(?:[^\"\\]|\\.)*\"")
_URL = re.compile(r"\b[A-Za-z][A-Za-z0-9+.-]*://[^\s\"'<>]+")
_EMAIL = re.compile(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}")
_OTHER_HOMES = re.compile(r"(?:/home/|/Users/)[^/\s\"']+|[A-Za-z]:\\Users\\[^\\\s\"']+")
_IPV4 = re.compile(r"(?<![\w.])(?:\d{1,3}\.){3}\d{1,3}(?![\w.])")
_IPV6 = re.compile(r"(?<![\w:])(?:[0-9A-Fa-f]{0,4}:){2,7}[0-9A-Fa-f]{0,4}(?![\w:])")


class LogRedactor:
    """Replaces what must never reach a log line: secrets, the machine's own names, addresses.

    The machine's home, host name and user name are read once at construction, so no list in the
    repository names anyone. Counts of what was replaced are kept per process.
    """

    def __init__(self, home: str = "", host: str = "", user: str = "", salt: bytes = b"") -> None:
        """Hold the machine's own names and the pseudonym salt.

        Args:
            home: The home directory, written `~`.
            host: The host name (and its short form), written `[host]`.
            user: The user name, written `[user]`.
            salt: The key of the pseudonyms.
        """
        self.home = home.rstrip("/\\")
        self.host = host
        self.user = user
        self.salt = salt
        self.counts: Counter[str] = Counter()
        self._shapes = tuple((keyword, re.compile(pattern)) for _, keyword, pattern in TOKEN_SHAPES)

    @classmethod
    def for_machine(cls, salt: bytes = b"") -> "LogRedactor":
        """Build a redactor from this machine's own names.

        Args:
            salt: The key of the pseudonyms.
        """
        try:
            user = getpass.getuser()
        except Exception:  # noqa: BLE001 - no passwd entry in some containers
            user = ""
        try:
            host = socket.gethostname()
        except Exception:  # noqa: BLE001
            host = ""
        return cls(home=str(Path.home()), host=host, user=user, salt=salt)

    @classmethod
    def secret_like(cls, key: str) -> bool:
        """Return whether a field or flag name is secret-like: its last `_` segment is on the list."""
        segments = re.split(r"[_-]", key.lower())
        return segments[-1] in SECRET_SUFFIXES

    def pseudonym(self, kind: str, value: str) -> str:
        """Return `p-` plus 12 hex of HMAC-SHA256(salt, kind || value)."""
        digest = hmac.new(self.salt, f"{kind}\x1f{value}".encode(), hashlib.sha256).hexdigest()
        return "p-" + digest[:12]

    def _count(self, name: str, text: str, new: str) -> str:
        if new != text:
            self.counts[name] += 1
        return new

    def redact(self, text: str, level: str = "info") -> str:
        """Redact free text, in the order of LOGGING §7.4.

        Args:
            text: An error message or a value that will be quoted.
            level: The line's level; addresses are replaced only above `debug`.
        """
        text = self._count("key", text, _KEY_BLOCK.sub("[key]", text))
        text = self._count("header", text, _AUTH_HEADER.sub(SECRET, text))
        text = self._count("bearer", text, _BEARER.sub(SECRET, text))
        for keyword, pattern in self._shapes:
            if keyword in text:
                text = self._count("token", text, pattern.sub(SECRET, text))
        text = self._count("flag", text, _FLAG.sub(self._flag, text))
        text = self._count("pair", text, _PAIR.sub(self._pair, text))
        text = self._count("pair", text, _JSON_PAIR.sub(self._json_pair, text))
        text = self._count("url", text, _URL.sub(self._url, text))
        text = self._count("email", text, _EMAIL.sub("[email]", text))
        text = self._names(text)
        if level not in ("debug", "trace"):
            text = self._addresses(text)
        return text

    def _flag(self, match: re.Match[str]) -> str:
        return (
            f"{match[1]}{match[2]}{SECRET}" if self.secret_like(match[1].lstrip("-")) else match[0]
        )

    def _pair(self, match: re.Match[str]) -> str:
        return f"{match[1]}={SECRET}" if self.secret_like(match[1]) else match[0]

    def _json_pair(self, match: re.Match[str]) -> str:
        return f'"{match[1]}"{match[2]}"{SECRET}"' if self.secret_like(match[1]) else match[0]

    @staticmethod
    def _url(match: re.Match[str]) -> str:
        raw = match[0]
        trailing = ""
        while raw and raw[-1] in ".,;)]}":
            trailing = raw[-1] + trailing
            raw = raw[:-1]
        try:
            parts = urlsplit(raw)
            host = parts.hostname or ""
            if ":" in host:
                host = f"[{host}]"
            port = f":{parts.port}" if parts.port else ""
        except ValueError:
            return match[0].split("?")[0].split("#")[0].split("@")[-1]
        return f"{parts.scheme}://{host}{port}{parts.path}{trailing}"

    def _names(self, text: str) -> str:
        if self.home:
            text = self._count("home", text, re.sub(re.escape(self.home) + r"(?![\w])", "~", text))
        text = self._count("home", text, _OTHER_HOMES.sub("~", text))
        if len(self.host) >= 3:
            short = self.host.split(".")[0]
            for name in {self.host, short} if len(short) >= 3 else {self.host}:
                text = self._count(
                    "host",
                    text,
                    re.sub(
                        r"(?<![\w.-])" + re.escape(name) + r"(?![\w-])", "[host]", text, flags=re.I
                    ),
                )
        if len(self.user) >= 3:
            text = self._count(
                "user",
                text,
                re.sub(r"(?<![\w-])" + re.escape(self.user) + r"(?![\w-])", "[user]", text),
            )
        return text

    def _addresses(self, text: str) -> str:
        def swap(match: re.Match[str]) -> str:
            try:
                address = ipaddress.ip_address(match[0])
            except ValueError:
                return match[0]
            return match[0] if address.is_loopback else ADDR

        return self._count("addr", text, _IPV6.sub(swap, _IPV4.sub(swap, text)))
