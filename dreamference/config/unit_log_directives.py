"""The logging directives every systemd service unit Ragon writes carries (LOGGING §3.3)."""

from __future__ import annotations

import re
from typing import Final

USER_ENVIRONMENT_FILE: Final[str] = "-%h/.config/dreamference/log.env"
SYSTEM_ENVIRONMENT_FILE: Final[str] = "-/etc/dreamference/log.env"
RATE_INTERVAL: Final[str] = "30s"
RATE_BURST: Final[int] = 20000
_SERVICE_HEADER: Final[re.Pattern[str]] = re.compile(r"^\[Service\]\n", re.MULTILINE)
_OWN_DIRECTIVE: Final[re.Pattern[str]] = re.compile(
    r"^(SyslogIdentifier|LogExtraFields|LogRateLimit\w+)=.*\n", re.MULTILINE
)


class UnitLogDirectives:
    """Builds and injects the `[Service]` log directives of a Ragon unit."""

    @classmethod
    def block(cls, component: str, *, system: bool = False) -> str:
        """Returns the directive lines for `component`.

        Args:
            component: A name from `components.txt` (a test checks every caller's).
            system: True for a system unit (no `%h`).

        Returns:
            str: Newline-terminated directive lines.
        """
        env = SYSTEM_ENVIRONMENT_FILE if system else USER_ENVIRONMENT_FILE
        return (
            f"SyslogIdentifier=ragon-{component}\n"
            f"EnvironmentFile={env}\n"
            f"LogExtraFields=RAGON_COMPONENT={component}\n"
            f"LogRateLimitIntervalSec={RATE_INTERVAL}\n"
            f"LogRateLimitBurst={RATE_BURST}\n"
        )

    @classmethod
    def inject(cls, text: str, component: str, *, system: bool = False) -> str:
        """Adds the directives right after every `[Service]` header of `text`.

        Args:
            text: A unit file's text.
            component: A name from `components.txt` (a test checks every caller's).
            system: True for a system unit.

        Returns:
            str: The text with the directives; a text without `[Service]` is unchanged.
        """
        block = cls.block(component, system=system)
        return _SERVICE_HEADER.sub(lambda _m: "[Service]\n" + block, _OWN_DIRECTIVE.sub("", text))
