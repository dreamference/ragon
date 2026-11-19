"""Choosing the primary and the 0.9.9 migration (specs/DREAMFERENCE_AUTH.md §7).

`ragon-admin node primary [<node id> | --self]` shows or changes the choice; `ragon-admin node
migrate` makes this node the primary of the upgrade (the rest secondaries) and registers a service
account for it and for every named secondary; `ragon-admin init` asks the same question once.
"""

import argparse
import sys
from collections.abc import Callable
from typing import Final

from dreamference.accounts.accounts_error import AccountsError
from dreamference.accounts.accounts_store import AccountsStore
from dreamference.node.node_identity import NodeIdentity
from dreamference.node.node_service_file import NodeServiceFile
from dreamference.node.node_settings import NodeSettings

CHOICE_PRIMARY: Final[str] = "primary"
CHOICE_SECONDARY: Final[str] = "secondary"
AUTH_TXT: Final[str] = "auth"
PRIMARY_COMMAND: Final[str] = "primary"
MIGRATE_COMMAND: Final[str] = "migrate"
Browse = Callable[[], list[dict[str, str]]]


class AccountsMigrate:
    """Parser and logic of `node primary`, `node migrate` and the `init` question."""

    PRIMARY_COMMAND: Final[str] = PRIMARY_COMMAND
    MIGRATE_COMMAND: Final[str] = MIGRATE_COMMAND

    @classmethod
    def add_parser(
        cls, node_subparsers: "argparse._SubParsersAction[argparse.ArgumentParser]"
    ) -> None:
        """Adds `primary` and `migrate` under `ragon-admin node`."""
        primary = node_subparsers.add_parser(
            "primary", help="Show, pin (<node id>) or take (--self) the authentication primary"
        )
        primary.add_argument("primary_id", nargs="?", default=None, help="The primary's node id")
        primary.add_argument("--self", dest="primary_self", action="store_true")
        migrate = node_subparsers.add_parser(
            "migrate",
            help="0.9.9 upgrade on the primary: take the role, register the service accounts",
        )
        migrate.add_argument(
            "--secondary",
            dest="secondaries",
            action="append",
            default=[],
            help="A secondary's node id (repeat); advertised nodes are used when none is named",
        )

    @staticmethod
    def _say(text: str) -> None:
        sys.stdout.write(text + "\n")

    # ----- node primary -------------------------------------------------------------------

    @classmethod
    def advertise(cls) -> None:
        """Rewrites the advertisement's `auth` record to match the choice (no-op if not advertised)."""
        NodeServiceFile.update(auth_primary=NodeSettings.auth_primary())

    @classmethod
    def run_primary(cls, args: argparse.Namespace) -> int:
        """`ragon-admin node primary [<node id>, --self]`.

        Returns:
            int: The exit code.
        """
        if args.primary_self and args.primary_id:
            cls._say("❌ Give a node id or --self, not both.")
            return 2
        own = NodeIdentity.read()
        if args.primary_self:
            NodeSettings.set_self_primary()
            cls.advertise()
            cls._say(f"✅ This node is the primary{f' ({own})' if own else ''}.")
            return 0
        if args.primary_id:
            if args.primary_id == own:
                cls._say("❌ That is this node: use --self.")
                return 1
            NodeSettings.set_pinned_primary(args.primary_id)
            cls.advertise()
            cls._say(f"✅ This node is a secondary of {args.primary_id}.")
            return 0
        cls._say(cls.describe(own))
        return 0

    @classmethod
    def describe(cls, own: str | None) -> str:
        """One line: this node's place in the topology."""
        if NodeSettings.auth_primary():
            return f"primary: this node{f' ({own})' if own else ''}"
        pinned = NodeSettings.pinned_primary()
        return f"primary: {pinned}" if pinned else "primary: not chosen (ragon-admin node primary)"

    # ----- init ---------------------------------------------------------------------------

    @classmethod
    def advertised_primaries(cls, browse: Browse) -> list[dict[str, str]]:
        """The nodes that advertise `auth=primary`."""
        return [entry for entry in browse() if entry.get(AUTH_TXT) == CHOICE_PRIMARY]

    @classmethod
    def choose(
        cls,
        browse: Browse,
        ask: Callable[[str], str] = input,
        say: Callable[[str], None] | None = None,
    ) -> str | None:
        """The `ragon-admin init` question: primary (keeps the accounts) or secondary.

        Args:
            browse: Lists the advertised nodes.
            ask: Reads one answer.
            say: Prints one line.

        Returns:
            str | None: `primary`, `secondary`, or None when nothing was chosen.
        """
        say = say or cls._say
        if NodeSettings.auth_primary() or NodeSettings.pinned_primary():
            return None
        answer = ask(
            "Is this node the primary (keeps the accounts) or a secondary? [primary/secondary] "
        )
        if answer.strip().lower().startswith("p"):
            NodeSettings.set_self_primary()
            cls.advertise()
            return CHOICE_PRIMARY
        primaries = cls.advertised_primaries(browse)
        for number, entry in enumerate(primaries, 1):
            say(f"  {number}. {entry.get('name', '?')} ({entry.get('node', '?')})")
        if not primaries:
            say("No primary is advertised on this network.")
            return None
        picked = ask(f"Which primary? [1-{len(primaries)}] ").strip()
        if not picked.isdigit() or not 1 <= int(picked) <= len(primaries):
            say("Nothing chosen; run `ragon-admin node primary <node id>` later.")
            return None
        NodeSettings.set_pinned_primary(primaries[int(picked) - 1]["node"])
        cls.advertise()
        return CHOICE_SECONDARY

    # ----- node migrate -------------------------------------------------------------------

    @classmethod
    def migrate(
        cls, store: AccountsStore, own_id: str, secondary_ids: list[str]
    ) -> dict[str, list[str]]:
        """Makes this node the primary and registers the service accounts.

        Args:
            store: This node's account store.
            own_id: This node's id.
            secondary_ids: The other nodes' ids.

        Returns:
            dict[str, list[str]]: `accounts`, the service accounts now present (this node's first),
            and `commands`, what to run on each secondary.
        """
        if not own_id:
            raise AccountsError("This node has no id: run ragon-admin node enable first.")
        NodeSettings.set_self_primary()
        cls.advertise()
        ids = [own_id, *[node for node in secondary_ids if node and node != own_id]]
        accounts = [store.ensure_service_account(node)["username"] for node in dict.fromkeys(ids)]
        commands = [
            f"ragon-admin node primary {own_id}" for node in secondary_ids if node != own_id
        ]
        return {"accounts": accounts, "commands": commands}

    @classmethod
    def run_migrate(
        cls,
        args: argparse.Namespace,
        store: AccountsStore | None = None,
        browse: Browse | None = None,
    ) -> int:
        """`ragon-admin node migrate`.

        Returns:
            int: The exit code.
        """
        own = NodeIdentity.read() or ""
        secondaries = list(args.secondaries)
        if not secondaries:
            if browse is None:
                from dreamference.node.node_browser import NodeBrowser

                browse = NodeBrowser.browse
            secondaries = [e["node"] for e in browse() if e.get("node") and e["node"] != own]
        try:
            result = cls.migrate(store or AccountsStore(), own, secondaries)
        except AccountsError as error:
            cls._say(f"❌ {error}")
            return 1
        cls._say(
            f"✅ This node is the primary; {len(result['accounts'])} service account(s) registered."
        )
        for node_id in secondaries:
            cls._say(f"💡 On {node_id}: ragon-admin node primary {own}")
        return 0
