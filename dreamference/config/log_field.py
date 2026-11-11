"""Typed log fields (LOGGING §5.3): the type decides how a value is written and redacted."""

from dataclasses import dataclass
from typing import Final

NUMBER_TYPES: Final[tuple[str, ...]] = ("count", "bytes", "ms", "s", "ratio")


@dataclass(frozen=True)
class LogField:
    """One typed field of a log line.

    There is no free-text type: a call site that wants to log a string from outside picks one of
    the constructors below or does not log it.
    """

    type: str
    key: str
    value: object
    allowed: tuple[str, ...] = ()

    @classmethod
    def _make(
        cls, kind: str, values: dict[str, object], allowed: tuple[str, ...] = ()
    ) -> tuple["LogField", ...]:
        return tuple(cls(kind, key, value, allowed) for key, value in values.items())

    @classmethod
    def count(cls, **values: object) -> tuple["LogField", ...]:
        """Number fields: counts, token numbers, ids that are numbers."""
        return cls._make("count", values)

    @classmethod
    def bytes_(cls, **values: object) -> tuple["LogField", ...]:
        """Byte sizes."""
        return cls._make("bytes", values)

    @classmethod
    def ms(cls, **values: object) -> tuple["LogField", ...]:
        """Durations in milliseconds, from a monotonic clock."""
        return cls._make("ms", values)

    @classmethod
    def s(cls, **values: object) -> tuple["LogField", ...]:
        """Durations in seconds, from a monotonic clock."""
        return cls._make("s", values)

    @classmethod
    def ratio(cls, **values: object) -> tuple["LogField", ...]:
        """Ratios."""
        return cls._make("ratio", values)

    @classmethod
    def id_(cls, **values: object) -> tuple["LogField", ...]:
        """Ragon's own short ids (`s-3f2a9c`)."""
        return cls._make("id", values)

    @classmethod
    def enum(cls, allowed: tuple[str, ...], **values: object) -> tuple["LogField", ...]:
        """Values from a fixed set the call site declares; anything else is written `other`."""
        return cls._make("enum", values, tuple(allowed))

    @classmethod
    def version(cls, **values: object) -> tuple["LogField", ...]:
        """A version, git sha or image digest."""
        return cls._make("version", values)

    @classmethod
    def path(cls, **values: object) -> tuple["LogField", ...]:
        """Ragon's own paths; the home becomes `~`."""
        return cls._make("path", values)

    @classmethod
    def url(cls, **values: object) -> tuple["LogField", ...]:
        """Scheme, host and path only."""
        return cls._make("url", values)

    @classmethod
    def addr(cls, **values: object) -> tuple["LogField", ...]:
        """An address: `[addr]` at info unless loopback, as is at debug."""
        return cls._make("addr", values)

    @classmethod
    def pseudo(cls, **values: object) -> tuple["LogField", ...]:
        """A salted pseudonym of a value with many possible forms."""
        return cls._make("pseudo", values)

    @classmethod
    def size_of(cls, **values: object) -> tuple["LogField", ...]:
        """The size of something that is never written: a number, or the length of a value."""
        return cls._make("size_of", values)

    @classmethod
    def error(
        cls, exc: BaseException | dict[str, object], key: str = "error"
    ) -> tuple["LogField", ...]:
        """An error class and its message, which the writer redacts.

        Args:
            exc: An exception, or a dict with `class` and `message`.
            key: The field key; the message is written as `<key>_msg`.
        """
        if isinstance(exc, dict):
            info = {"class": str(exc.get("class", "Error")), "message": str(exc.get("message", ""))}
        else:
            info = {"class": type(exc).__name__, "message": str(exc)}
        return (cls("error", key, info),)
