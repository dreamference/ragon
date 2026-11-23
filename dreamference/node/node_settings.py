"""Whether this node is advertised, and what it shares (specs/DREAMFERENCE_RAGON_NODE.md §4).

Kept in `~/.config/dreamference/node-advertise.json`, not in `dreamference.toml`: that file is
resolved from the working directory first, and the address a service publishes on must not
depend on the folder a command happened to be run from.
"""

import json
import os
from pathlib import Path
from typing import Any, Final

LOOPBACK: Final[str] = "127.0.0.1"
EVERY_INTERFACE: Final[str] = "0.0.0.0"
AUTH_PRIMARY: Final[str] = "primary"
# Whether the node advertises the accounts service (`[accounts] enabled`). On from 0.9.9, so the
# launcher asks for a sign-in; 0.9.8 shipped it off.
DEFAULT_ACCOUNTS_ENABLED: Final[bool] = True
ACCOUNTS_ENABLED_VARIABLE: Final[str] = "DREAMFERENCE_ACCOUNTS_ENABLED"


class NodeSettings:
    """The two switches `ragon-admin node enable|disable` set."""

    @classmethod
    def path(cls) -> Path:
        """Returns `~/.config/dreamference/node-advertise.json`, resolved at call time."""
        return Path("~/.config/dreamference/node-advertise.json").expanduser()

    @classmethod
    def load(cls) -> dict[str, bool]:
        """Returns a dict with `advertise` and `web`.

        `advertise` (the node is offered to the network) and `web` (the web UI is offered
        with it). Both False when the file is missing or unreadable: a node that was never
        enabled keeps every bind on loopback.
        """
        try:
            data: Any = json.loads(cls.path().read_text())
        except (OSError, ValueError):
            data = {}
        if not isinstance(data, dict):
            data = {}
        advertise = data.get("advertise") is True
        return {"advertise": advertise, "web": advertise and data.get("web") is not False}

    @classmethod
    def save(cls, advertise: bool, web: bool = True) -> None:
        """Sets the two switches.

        Args:
            advertise: Whether the node is advertised.
            web: Whether the web UI is shared with the network when it is.
        """
        cls._write({"advertise": advertise, "web": web})

    @classmethod
    def _raw(cls) -> dict[str, Any]:
        """The file's whole content, `{}` when missing or unreadable."""
        try:
            data: Any = json.loads(cls.path().read_text())
        except (OSError, ValueError):
            return {}
        return data if isinstance(data, dict) else {}

    @classmethod
    def _write(cls, changes: dict[str, Any]) -> None:
        """Merges `changes` into the file (a None value drops the key) and replaces it atomically."""
        data = cls._raw()
        for key, value in changes.items():
            if value is None:
                data.pop(key, None)
            else:
                data[key] = value
        path = cls.path()
        path.parent.mkdir(parents=True, exist_ok=True)
        staging = path.with_name(f".{path.name}.{os.getpid()}.tmp")
        staging.write_text(json.dumps(data, indent=2) + "\n")
        Path(staging).replace(path)

    @classmethod
    def auth_primary(cls) -> bool:
        """Whether this node is the authentication primary (specs/DREAMFERENCE_AUTH.md §7)."""
        return cls._raw().get("auth") == AUTH_PRIMARY

    @classmethod
    def pinned_primary(cls) -> str | None:
        """The node id a secondary pins as its primary, or None."""
        value = cls._raw().get("primary")
        return value if isinstance(value, str) and value else None

    @classmethod
    def set_self_primary(cls) -> None:
        """Makes this node the primary: `auth=primary` is advertised, no other id is pinned."""
        cls._write({"auth": AUTH_PRIMARY, "primary": None})

    @classmethod
    def set_pinned_primary(cls, node_id: str) -> None:
        """Makes this node a secondary of the primary with `node_id`."""
        cls._write({"auth": None, "primary": node_id})

    @classmethod
    def auth_enforce(cls) -> bool:
        """Whether a model request with no credential is refused (`[node] auth_enforce`, false).

        Read from the user-level `config.toml`; false (serve and count) unless set to true.
        """
        from dreamference.night_shift.night_shift_settings import NightShiftSettings

        path = Path("~/.config/dreamference/config.toml").expanduser()
        return NightShiftSettings.read_table(path, section="node").get("auth_enforce") is True

    @classmethod
    def accounts_enabled(cls) -> bool:
        """Whether the node advertises accounts (`DREAMFERENCE_ACCOUNTS_ENABLED`, `[accounts] enabled`).

        The variable wins (an empty one counts as unset), then the user-level `config.toml`, then
        DEFAULT_ACCOUNTS_ENABLED: true from 0.9.9; `[accounts] enabled = false` or
        DREAMFERENCE_ACCOUNTS_ENABLED=0 turns it off (the launcher asks for a sign-in only when the
        node advertises accounts).
        """
        value = os.environ.get(ACCOUNTS_ENABLED_VARIABLE, "").strip()
        if value:
            return value.lower() in ("1", "true", "yes", "on")
        from dreamference.night_shift.night_shift_settings import NightShiftSettings

        path = Path("~/.config/dreamference/config.toml").expanduser()
        enabled = NightShiftSettings.read_table(path, section="accounts").get("enabled")
        return enabled if isinstance(enabled, bool) else DEFAULT_ACCOUNTS_ENABLED

    @classmethod
    def advertised(cls) -> bool:
        """Returns True once `node enable` has run and `node disable` has not."""
        return cls.load()["advertise"]

    @classmethod
    def search_bind_address(cls) -> str:
        """Returns the SearXNG host address.

        Every interface on an advertised node, loopback otherwise.
        """
        return EVERY_INTERFACE if cls.advertised() else LOOPBACK

    @classmethod
    def web_bind_address(cls) -> str:
        """Returns the web UI (`ragon web`, port 3100) host address.

        Every interface on an advertised node that shares its web UI, loopback otherwise.
        """
        return EVERY_INTERFACE if cls.load()["web"] else LOOPBACK
