# ruff: noqa: PLR2004 - field kinds, levels and byte widths are the spec's literals
"""The one line format (LOGGING §5.1): typed fields in, one escaped text line out."""

import ipaddress
import math
import re
from datetime import UTC, datetime
from typing import Final
from urllib.parse import urlsplit

from dreamference.config.log_field import NUMBER_TYPES, LogField
from dreamference.config.log_redactor import ADDR, SECRET, LogRedactor

MAX_LINE_BYTES: Final[int] = 4096
LEVEL_NAMES: Final[tuple[str, ...]] = ("ERROR", "WARN", "INFO", "DEBUG", "TRACE")
_BARE = re.compile(r"[A-Za-z0-9._:/~+@\[\]-]+")
_ID = re.compile(r"[a-z]{1,4}-[A-Za-z0-9]{2,32}")
_VERSION = re.compile(r"[A-Za-z0-9._+:@/-]{1,100}")
_TOKEN = re.compile(r"[a-z0-9_.-]{1,64}")
_MESSAGE_BAD = re.compile(r"[^a-z0-9 _.:/-]")


class LogLineFormatter:
    """Builds `ts component LEVEL message key=value ...` with every value typed and escaped."""

    @classmethod
    def timestamp(cls, epoch: float) -> str:
        """Return UTC, always milliseconds, always `Z`."""
        moment = datetime.fromtimestamp(epoch, UTC)
        return moment.strftime("%Y-%m-%dT%H:%M:%S.") + f"{moment.microsecond // 1000:03d}Z"

    @classmethod
    def message(cls, text: str) -> str:
        """Force a message into `[a-z0-9][a-z0-9 _.:/-]{0,119}`; a literal from the source already is."""
        text = _MESSAGE_BAD.sub("_", str(text).lower())[:120].strip()
        return text if text and text[0].isalnum() else "message " + text[:100].lstrip("_ .:/-")

    @classmethod
    def key(cls, text: str) -> str:
        """Force a key into `[a-z][a-z0-9_]*`."""
        text = re.sub(r"[^a-z0-9_]", "_", str(text).lower())[:64]
        return text if text and text[0].isalpha() else "k" + text

    @classmethod
    def quote(cls, text: str) -> str:
        """Quote and escape a value: control, C1, bidi and non-UTF-8 bytes never reach the line."""
        out = ['"']
        for char in text:
            code = ord(char)
            if char == '"':
                out.append('\\"')
            elif char == "\\":
                out.append("\\\\")
            elif char == "\n":
                out.append("\\n")
            elif char == "\t":
                out.append("\\t")
            elif 0xDC80 <= code <= 0xDCFF:
                out.append(f"\\x{{{code - 0xDC00:02X}}}")
            elif (
                code < 0x20
                or code == 0x7F
                or 0x80 <= code <= 0x9F
                or 0x202A <= code <= 0x202E
                or 0x2066 <= code <= 0x2069
                or 0xD800 <= code <= 0xDFFF
            ):
                out.append(f"\\u{{{code:04X}}}")
            else:
                out.append(char)
        out.append('"')
        return "".join(out)

    @classmethod
    def text_of(cls, value: object) -> str:
        """Return a value as text; bytes decode with invalid bytes kept as surrogates for `quote`."""
        if isinstance(value, bytes | bytearray):
            return bytes(value).decode("utf-8", "surrogateescape")
        return str(value)

    @classmethod
    def render(cls, text: str) -> str:
        """Return the value bare when its characters allow it, otherwise quoted."""
        return text if _BARE.fullmatch(text) else cls.quote(text)

    @classmethod
    def number(cls, value: object) -> str:
        """Write a number; anything else is `[number]`."""
        if isinstance(value, bool) or not isinstance(value, int | float):
            return "[number]"
        if isinstance(value, float):
            if not math.isfinite(value):
                return "[number]"
            return f"{value:.3f}".rstrip("0").rstrip(".") or "0"
        return str(value)

    @classmethod
    def value(cls, field: LogField, level: str, redactor: LogRedactor) -> list[tuple[str, str]]:  # noqa: C901, PLR0911 - one branch per field type
        """Return the `(key, written value)` pairs one field stands for.

        Args:
            field: The typed field.
            level: The line's level name, lower case.
            redactor: The process's redactor.
        """
        key = cls.key(field.key)
        if redactor.secret_like(key):
            return [(key, SECRET)]
        kind, raw = field.type, field.value
        if kind in NUMBER_TYPES:
            return [(key, cls.number(raw))]
        if kind == "size_of":
            size = (
                raw
                if isinstance(raw, int | float) and not isinstance(raw, bool)
                else len(cls.text_of(raw).encode("utf-8", "surrogateescape"))
            )
            return [(key, cls.number(size))]
        text = cls.text_of(raw)
        if kind == "id":
            return [(key, text if _ID.fullmatch(text) else "[id]")]
        if kind == "enum":
            return [(key, text if text in field.allowed and _TOKEN.fullmatch(text) else "other")]
        if kind == "version":
            return [(key, text if _VERSION.fullmatch(text) else "[version]")]
        if kind == "pseudo":
            return [(key, redactor.pseudonym(key, text))]
        if kind == "quoted":  # the writer's own notes: a message literal, redacted
            return [(key, cls.render(redactor.redact(text, level)))]
        if kind == "path":
            return [(key, cls.render(redactor.redact(text, level)))]
        if kind == "url":
            return [(key, cls.render(cls.url(text, level, redactor)))]
        if kind == "addr":
            return [(key, cls.address(text, level))]
        if kind == "error":
            klass = raw.get("class", "Error") if isinstance(raw, dict) else "Error"
            message = raw.get("message", "") if isinstance(raw, dict) else ""
            klass = klass if re.fullmatch(r"[A-Za-z_][A-Za-z0-9_.]{0,80}", klass) else "Error"
            return [
                (key, klass),
                (key + "_msg", cls.quote(redactor.redact(cls.text_of(message), level))),
            ]
        return [(key, "[type]")]

    @classmethod
    def address(cls, text: str, level: str) -> str:
        """Write an address: loopback as is, others `[addr]` at info and as is at debug (O9)."""
        try:
            parsed = ipaddress.ip_address(text.strip("[]"))
        except ValueError:
            return "[addr]"
        if parsed.is_loopback or level in ("debug", "trace"):
            return cls.render(str(parsed))
        return ADDR

    @classmethod
    def url(cls, text: str, level: str, redactor: LogRedactor) -> str:
        """Keep scheme, host and path only."""
        try:
            parts = urlsplit(text)
            host = parts.hostname or ""
            port = f":{parts.port}" if parts.port else ""
        except ValueError:
            return "[url]"
        if not parts.scheme or not host:
            return "[url]"
        if ":" in host:
            host = f"[{host}]"
        return redactor.redact(f"{parts.scheme}://{host}{port}{parts.path}", level)

    @classmethod
    def format(
        cls,
        epoch: float,
        component: str,
        level: int,
        message: str,
        fields: tuple[LogField, ...],
        redactor: LogRedactor,
    ) -> str:
        """Build one line, at most 4 KiB, cut at the last whole field with `truncated=1`.

        Args:
            epoch: The wall clock at the call.
            component: The component name.
            level: 0 error .. 4 trace.
            message: The literal message.
            fields: The typed fields, in the order the call site gave them.
            redactor: The process's redactor.
        """
        name = LEVEL_NAMES[level]
        head = f"{cls.timestamp(epoch)} {component} {name} {cls.message(message)}"
        parts: list[str] = []
        for field in fields:
            try:
                pairs = cls.value(field, name.lower(), redactor)
            except Exception:  # noqa: BLE001 - a bad value never fails the caller
                pairs = [(cls.key(field.key), "[error]")]
            parts.extend(f"{key}={written}" for key, written in pairs)
        line = " ".join([head, *parts])
        if len(line.encode("utf-8", "replace")) <= MAX_LINE_BYTES:
            return line
        while (
            parts
            and len(" ".join([head, *parts, "truncated=1"]).encode("utf-8", "replace"))
            > MAX_LINE_BYTES
        ):
            parts.pop()
        return " ".join([head, *parts, "truncated=1"])
