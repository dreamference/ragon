"""The node's service key (specs/DREAMFERENCE_AUTH.md §13).

Every unattended caller of `ragon` (Night Shift, the SWE-bench harness, the egress audit, the
Codex test runner, the first-run check) has no `auth.json`, so it passes the node's service
key as `RAGON_API_KEY`. The key sits in `~/.config/dreamference/service-key`, mode `0600`.
"""

import os
import stat
from collections.abc import MutableMapping
from pathlib import Path
from typing import Final

KEY_VARIABLE: Final[str] = "RAGON_API_KEY"
KEY_PREFIX: Final[str] = "sk-ragon-"


class ServiceKey:
    """Reads the node's service key and puts it in an environment."""

    @classmethod
    def path(cls) -> Path:
        """Where this node's service key is kept.

        Returns:
            Path: `~/.config/dreamference/service-key`, resolved at call time.
        """
        return Path("~/.config/dreamference/service-key").expanduser()

    @classmethod
    def read(cls) -> str | None:
        """The service key, if the file is private to its owner and holds a key.

        Returns:
            str | None: The key, or None when the file is missing, readable by group or other,
            not ours, or does not hold an `sk-ragon-` key.
        """
        path = cls.path()
        try:
            info = path.stat()
            text = path.read_text().strip()
        except OSError:
            return None
        if info.st_mode & (stat.S_IRWXG | stat.S_IRWXO) or info.st_uid != os.getuid():
            return None
        return text if text.startswith(KEY_PREFIX) else None

    @classmethod
    def environment(cls) -> dict[str, str]:
        """The variables a caller that builds its own environment merges in.

        Returns:
            dict[str, str]: `{RAGON_API_KEY: key}`, or empty when there is no usable key.
        """
        env: dict[str, str] = {}
        cls.apply(env)
        return env

    @classmethod
    def apply(cls, env: MutableMapping[str, str]) -> bool:
        """Set `RAGON_API_KEY` in `env` from the key file, unless the caller already set one.

        Args:
            env: The environment of the `ragon` about to start.

        Returns:
            bool: True when `env` now carries a key.
        """
        if env.get(KEY_VARIABLE):
            return True
        key = cls.read()
        if key is None:
            return False
        env[KEY_VARIABLE] = key
        return True
