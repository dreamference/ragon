"""The log level spec string: one grammar in Python, Rust and C++ (LOGGING §4.2)."""

from dataclasses import dataclass, field
from typing import Final

LEVELS: Final[tuple[str, ...]] = ("error", "warn", "info", "debug", "trace", "off")
ASSIGNMENT: Final[str] = "="


@dataclass(frozen=True)
class LogLevelSpec:
    """A parsed spec: an optional default level and per-component overrides.

    An unknown level or an empty part is a warning, and the rest of the string still applies.
    """

    default: str | None = None
    overrides: dict[str, str] = field(default_factory=dict)
    warnings: tuple[str, ...] = ()

    @classmethod
    def is_set(cls, candidate: object) -> bool:
        """Return whether a tier holds a spec at all.

        Args:
            candidate: A raw value from one tier (a kwarg, the environment, a file).

        Returns:
            True for a non-blank string; a blank or non-string value leaves the tier unset.
        """
        return isinstance(candidate, str) and candidate.strip() != ""

    @classmethod
    def parse(cls, spec: str) -> "LogLevelSpec":
        """Parse a spec string.

        Args:
            spec: The text of `RAGON_LOG` or `log_level`, e.g. `info,engine=debug`.

        Returns:
            The parsed default, the overrides in order, and one warning per ignored part.
        """
        default: str | None = None
        overrides: dict[str, str] = {}
        warnings: list[str] = []
        if spec.strip() == "":
            return cls()
        parts = [part.strip() for part in spec.split(",")]
        for index, part in enumerate(parts):
            if ASSIGNMENT in part:
                component, _, level = (text.strip() for text in part.partition(ASSIGNMENT))
                if component == "" or level not in LEVELS:
                    warnings.append(f"ignored log level part {part!r}")
                    continue
                overrides[component] = level
            elif part in LEVELS and index == 0:
                default = part
            else:
                warnings.append(f"ignored log level part {part!r}")
        return cls(default=default, overrides=overrides, warnings=tuple(warnings))
