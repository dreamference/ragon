"""Choosing the primary and the 0.9.9 migration (specs/DREAMFERENCE_AUTH.md §7).

`ragon-admin node primary [<node id> | --self]` shows or changes the choice; `ragon-admin node
migrate` makes this node the primary of the upgrade (the rest secondaries) and registers a service
account for it and for every named secondary; `ragon-admin init` asks the same question once.
"""

import argparse
import sys
import uuid
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
# address, port, node id -> that node's CA certificate (PEM); raises OSError.
FetchCa = Callable[[str, str | int | None, str], bytes]


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
    def run_primary(
        cls,
        args: argparse.Namespace,
        browse: Browse | None = None,
        fetch_ca: FetchCa | None = None,
    ) -> int:
        """`ragon-admin node primary [<node id or name>, --self]`.

        Pinning finds the primary in the advertisements (by node id or name), pins its CA and
        pins its id, then makes the running accounts service follow the new role (§6, §7).

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
            cls.follow_role()
            cls._say(f"✅ This node is the primary{f' ({own})' if own else ''}.")
            return 0
        if args.primary_id:
            return cls.pin_named(args.primary_id, own, browse, fetch_ca)
        cls._say(cls.describe(own))
        return 0

    @classmethod
    def pin_named(
        cls,
        wanted: str,
        own: str | None,
        browse: Browse | None = None,
        fetch_ca: FetchCa | None = None,
    ) -> int:
        """`node primary <node id or name>`: the one advertised primary of that id or name, pinned.

        Refused: this node; nothing advertised; a name or id that several advertisements carry
        (an impostor, or a stale record: the operator pins by UUID on a quiet network); a node
        that keeps no accounts. The success line prints the UUID and the CA's fingerprint, which
        `node primary` on the primary prints too: first contact trusts the network once (§6).

        Returns:
            int: The exit code.
        """
        matches = [] if wanted == own else cls.matches(wanted, browse or cls.default_browse())
        if wanted == own or any(entry["node"] == own for entry in matches):
            cls._say("❌ That is this node: use --self.")
            return 1
        if not matches:
            cls._say(
                f"❌ No node {wanted} is advertised on this network: give the primary's node id"
                " or name while its server runs."
            )
            return 1
        try:
            entry = cls.one_node(wanted, matches)
            fingerprint = cls.pin(entry, fetch_ca)
        except AccountsError as error:
            cls._say(f"❌ {error}")
            return 1
        name = entry.get("name") or entry["node"]
        cls._say(f"✅ This node is a secondary of {name} ({entry['node']}).")
        cls._say(
            f"🔐 Its CA: SHA-256 {fingerprint}. `ragon-admin node primary` on {name} must print"
            " the same."
        )
        return 0

    @classmethod
    def default_browse(cls) -> Browse:
        """The LAN's advertisements, one entry per resolved record (`NodeBrowser.browse`)."""
        from dreamference.node.node_browser import NodeBrowser

        return lambda: NodeBrowser.browse(merge=False)

    @classmethod
    def matches(cls, wanted: str, browse: Browse) -> list[dict[str, str]]:
        """Every advertisement with this node id, name or `.local` host name."""
        lowered = wanted.lower().removesuffix(".")
        found = []
        for entry in browse():
            names = {entry.get("name", "").lower(), entry.get("host", "").lower().rstrip(".")}
            if entry.get("node") and (
                entry["node"].lower() == lowered or lowered in names or f"{lowered}.local" in names
            ):
                found.append(entry)
        return found

    @classmethod
    def one_node(cls, wanted: str, matches: list[dict[str, str]]) -> dict[str, str]:
        """The one primary the matches name, at its best address.

        One node may be seen on several interfaces and addresses under its one mDNS host name
        (which mDNS keeps unique); two node ids, two host names or two ports are two advertisers.

        Raises:
            AccountsError: Several advertisers, or a node that does not advertise `auth=primary`.
        """
        from dreamference.node.node_browser import NodeBrowser

        sources = {
            (entry["node"].lower(), entry.get("host", "").lower().rstrip("."), entry.get("port"))
            for entry in matches
        }
        if len(sources) > 1:
            seen = ", ".join(
                sorted({f"{e['node']}@{e.get('address')}:{e.get('port')}" for e in matches})
            )
            raise AccountsError(
                f"{wanted} is advertised by several nodes: {seen}. Pin by UUID on a quiet network."
            )
        entry = min(matches, key=lambda e: NodeBrowser.rank(e.get("address", "")))
        if entry.get(AUTH_TXT) != CHOICE_PRIMARY:
            raise AccountsError(
                f"{entry.get('name') or entry['node']} does not keep accounts (auth=primary not"
                " advertised): pin the primary, or run `ragon-admin node primary --self` there."
            )
        return entry

    @classmethod
    def pin(cls, entry: dict[str, str], fetch_ca: FetchCa | None = None) -> str:
        """Makes this node a secondary of the advertised `entry`: its CA, its id, the service.

        Returns:
            str: The pinned CA's SHA-256 fingerprint.

        Raises:
            AccountsError: The advertised id is not a node id, or the primary's CA could not be
                fetched, belongs to another node, or differs from the pinned one.
        """
        from dreamference.accounts.accounts_secondary import AccountsSecondary
        from dreamference.vllm_server.model_gate_tls import ModelGateTls

        name = entry.get("name") or entry.get("node") or "?"
        try:  # the id names a directory: only a node id (a UUID, as NodeIdentity) is taken
            node_id = str(uuid.UUID(entry.get("node", "")))
        except ValueError as error:
            raise AccountsError(f"{name} advertises no valid node id.") from error
        fetch = fetch_ca or AccountsSecondary.fetch_ca
        if not entry.get("address"):
            raise AccountsError(f"{name} advertises no address.")
        try:
            pem = fetch(entry["address"], entry.get("port"), node_id)
            fingerprint = ModelGateTls.fingerprint(pem)
        except (OSError, ValueError) as error:
            raise AccountsError(f"The CA of {name} could not be pinned: {error}.") from error
        path = AccountsSecondary.ca_path(node_id)
        if path.is_file() and path.read_bytes().strip() != pem.strip():
            raise AccountsError(
                f"The certificate of {name} has changed. If the node was set up again, remove"
                f" {path} and run this again."
            )
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(pem)
        NodeSettings.set_pinned_primary(node_id)
        cls.advertise()
        cls.follow_role()
        return fingerprint

    @classmethod
    def follow_role(cls) -> None:
        """The accounts service and the gate's info follow a changed role at once (§7).

        Without this a node pinned after `server start` kept its primary-mode service, which
        answered every sign-in and key from its own empty store (the w48s test pair, 11-24).
        """
        from dreamference.accounts.accounts_service import AccountsService
        from dreamference.vllm_server.model_gate import GATE_DIR, ModelGate

        if AccountsService.follow_role():
            cls._say("🔄 The accounts service follows the new role.")
        if GATE_DIR.is_dir():
            ModelGate.write_node_info()

    @classmethod
    def describe(cls, own: str | None) -> str:
        """This node's place in the topology, with the CA fingerprint a secondary pins (§6).

        The primary prints its gate's CA, a secondary the CA it pinned: the operator compares
        the two out of band, the one check against an impostor at first contact.
        """
        from dreamference.accounts.accounts_secondary import AccountsSecondary
        from dreamference.vllm_server.model_gate_tls import CA_CERT, ModelGateTls

        if NodeSettings.auth_primary():
            line = f"primary: this node{f' ({own})' if own else ''}"
            ca = ModelGateTls.directory() / CA_CERT
        else:
            pinned = NodeSettings.pinned_primary()
            if not pinned:
                return "primary: not chosen (ragon-admin node primary)"
            line = f"primary: {pinned}"
            ca = AccountsSecondary.ca_path(pinned)
        try:
            return f"{line}\nCA SHA-256: {ModelGateTls.fingerprint(ca.read_bytes())}"
        except (OSError, ValueError):
            return f"{line}\nCA SHA-256: none yet ({ca} is missing or unreadable)"

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
        fetch_ca: FetchCa | None = None,
    ) -> str | None:
        """The `ragon-admin init` question: primary (keeps the accounts) or secondary.

        Args:
            browse: Lists the advertised nodes.
            ask: Reads one answer.
            say: Prints one line.
            fetch_ca: Fetches the chosen primary's CA (tests); `AccountsSecondary.fetch_ca`.

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
            cls.follow_role()
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
        entry = primaries[int(picked) - 1]
        try:
            fingerprint = cls.pin(entry, fetch_ca)
        except AccountsError as error:
            say(f"❌ {error} Run `ragon-admin node primary <node id>` later.")
            return None
        say(
            f"🔐 {entry.get('name', '?')}'s CA: SHA-256 {fingerprint}. `ragon-admin node primary`"
            " there must print the same."
        )
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
