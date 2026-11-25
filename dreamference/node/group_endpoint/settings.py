"""The group endpoint's ``[node]`` settings: ``group``, ``backends``, ``endpoint_only``, ``queue_wait_s``.

Parsed from a config dict (the ``[node]`` table, or the whole config holding it). Absent keys take
the defaults of the specification (single endpoint, section 10); a wrong type or value is refused
with a ``ValueError`` naming the key. There is no limit setting.
"""

import re
from collections.abc import Mapping
from dataclasses import dataclass
from typing import Any, Final

GROUPS: Final[tuple[str, ...]] = ("serve", "hold-only", "out")
DEFAULT_GROUP: Final[str] = "serve"
PAIRED: Final[str] = "paired"
DEFAULT_QUEUE_WAIT_S: Final[int] = 60
MAX_QUEUE_WAIT_S: Final[int] = 3600
NODE_ID: Final[re.Pattern] = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]{0,63}")


@dataclass(frozen=True)
class GroupEndpointSettings:
    """The validated group-endpoint settings.

    Attributes:
        group: ``serve``, ``hold-only`` or ``out``.
        backends: Node ids, or ``"paired"``; empty means the endpoint is off.
        endpoint_only: A member accepts only the primary and loopback.
        queue_wait_s: Seconds a connection is held waiting for a slot.
    """

    group: str = DEFAULT_GROUP
    backends: tuple[str, ...] | str = ()
    endpoint_only: bool = False
    queue_wait_s: int = DEFAULT_QUEUE_WAIT_S

    @property
    def endpoint_on(self) -> bool:
        """Whether the primary's endpoint is switched on (``backends`` is not empty)."""
        return bool(self.backends)

    @classmethod
    def from_config(cls, config: Mapping[str, Any] | None) -> "GroupEndpointSettings":
        """Parse and validate the settings.

        Args:
            config: The ``[node]`` table, or a dict holding it under ``node``; None for defaults.

        Returns:
            The settings.

        Raises:
            ValueError: A key has a wrong type or value, or a node id is malformed.
        """
        table: Mapping[str, Any] = config or {}
        if isinstance(table.get("node"), Mapping):
            table = table["node"]
        return cls(
            group=cls._group(table.get("group", DEFAULT_GROUP)),
            backends=cls._backends(table.get("backends", ())),
            endpoint_only=cls._flag(table.get("endpoint_only", False)),
            queue_wait_s=cls._wait(table.get("queue_wait_s", DEFAULT_QUEUE_WAIT_S)),
        )

    @staticmethod
    def _group(value: object) -> str:
        if not isinstance(value, str) or value not in GROUPS:
            raise ValueError(f"[node] group must be one of {', '.join(GROUPS)}")
        return value

    @staticmethod
    def _backends(value: object) -> tuple[str, ...] | str:
        if value == PAIRED:
            return PAIRED
        if not isinstance(value, (list, tuple)):
            raise ValueError(f'[node] backends must be a list of node ids or "{PAIRED}"')
        ids: list[str] = []
        for item in value:
            if not isinstance(item, str) or not NODE_ID.fullmatch(item):
                raise ValueError(f"[node] backends holds a bad node id: {item!r}")
            if item not in ids:
                ids.append(item)
        return tuple(ids)

    @staticmethod
    def _flag(value: object) -> bool:
        if not isinstance(value, bool):
            raise ValueError("[node] endpoint_only must be true or false")
        return value

    @staticmethod
    def _wait(value: object) -> int:
        if (
            isinstance(value, bool)
            or not isinstance(value, int)
            or not 0 <= value <= MAX_QUEUE_WAIT_S
        ):
            raise ValueError(
                f"[node] queue_wait_s must be a whole number from 0 to {MAX_QUEUE_WAIT_S}"
            )
        return value
