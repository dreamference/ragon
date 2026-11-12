"""The node's stable id (specs/DREAMFERENCE_RAGON_NODE.md §5.1, §6.1).

A UUID written once to `~/.config/dreamference/node-id`. Clients remember a node by it, not by
its address or name, and the launcher treats a machine that has the file as a node: it talks to
its own model server on loopback and never browses the network for one.
"""

import os
import uuid
from pathlib import Path


class NodeIdentity:
    """Reads and creates the node id."""

    @classmethod
    def path(cls) -> Path:
        """Where this machine's node id is kept.

        Returns:
            Path: `~/.config/dreamference/node-id`, resolved at call time so a test's own home
            folder is used.
        """
        return Path("~/.config/dreamference/node-id").expanduser()

    @classmethod
    def read(cls) -> str | None:
        """This machine's node id.

        Returns:
            Optional[str]: The id, or None if this machine has none (or the file is not a UUID).
        """
        try:
            text = cls.path().read_text().strip()
        except OSError:
            return None
        try:
            return str(uuid.UUID(text))
        except ValueError:
            return None

    @classmethod
    def ensure(cls) -> str:
        """Returns the node id, creating it on first use.

        It is never rewritten: a client that remembered this node must find the same id after an
        update or a reinstall.

        Returns:
            str: The id.
        """
        existing = cls.read()
        if existing:
            return existing
        path = cls.path()
        path.parent.mkdir(parents=True, exist_ok=True)
        node_id = str(uuid.uuid4())
        staging = path.with_name(f".{path.name}.{os.getpid()}.tmp")
        staging.write_text(node_id + "\n")
        Path(staging).replace(path)
        return node_id
