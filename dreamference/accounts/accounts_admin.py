"""`ragon-admin node user`: the admin commands of a primary's accounts (AUTH spec §12)."""

import argparse
import getpass
import socket
import subprocess
import sys
import time
from pathlib import Path
from typing import Final

from dreamference.accounts.accounts_store import (
    AUDIT_TAG,
    ROLE_ADMIN,
    ROLE_USER,
    AccountsError,
    AccountsStore,
)
from dreamference.accounts.accounts_usage import AccountsUsage
from dreamference.config.ports import DEFAULT_MODEL_PORT

CMD_LOG: Final[str] = "log"
STAMP: Final[str] = "%Y-%m-%d %H:%M"
COMMANDS: Final[tuple[str, ...]] = (
    "list",
    "add",
    "reset",
    "remove",
    "promote",
    "demote",
    "key",
    "log",
    "usage",
    "export",
    "import",
)


class AccountsAdmin:
    """Parser and dispatch of `ragon-admin node user`."""

    @classmethod
    def add_parser(
        cls, node_subparsers: "argparse._SubParsersAction[argparse.ArgumentParser]"
    ) -> None:
        """Adds `user` and its sub-commands under `ragon-admin node`.

        Args:
            node_subparsers: The `node` parser's sub-parsers.
        """
        user = node_subparsers.add_parser("user", help="Manage the accounts of this primary node")
        sub = user.add_subparsers(dest="user_command")
        listing = sub.add_parser("list", help="Users with role, service flag, created and last use")
        listing.add_argument("--sessions", action="store_true", help="Also every token and binding")
        add = sub.add_parser("add", help="Create a user; the password is read without echo")
        add.add_argument("name")
        for verb, text in (
            ("reset", "Print a one-time password; the next sign-in must set a new one"),
            ("remove", "Remove a user and revoke tokens, keys and bindings"),
            ("promote", "Make a user an admin"),
            ("demote", "Make an admin a plain user"),
        ):
            sub.add_parser(verb, help=text).add_argument("name")
        key = sub.add_parser("key", help="Mint an API key (shown once) or revoke one by name")
        key.add_argument("name")
        key.add_argument("--name", dest="key_name", default="default", help="The key's name")
        key.add_argument("--revoke", default=None, help="Revoke the key of this name")
        sub.add_parser("log", help="The audit lines from the journal")
        usage = sub.add_parser("usage", help="Requests and tokens per user per day and client kind")
        usage.add_argument("--days", type=int, default=7)
        usage.add_argument(
            "--unauthenticated",
            action="store_true",
            help="The requests served without credentials, per day, node, client kind and source",
        )
        sub.add_parser("export", help="Print users, roles and key hashes as JSON (never tokens)")
        sub.add_parser("import", help="Import an export").add_argument("file")

    @staticmethod
    def _say(text: str) -> None:
        sys.stdout.write(text + "\n")

    @classmethod
    def run(cls, args: argparse.Namespace, store: AccountsStore | None = None) -> int:
        """Runs one sub-command.

        Args:
            args: The parsed arguments.
            store: The store to use; opened at its default path when None.

        Returns:
            int: The exit code.
        """
        command = getattr(args, "user_command", None)
        if command is None:
            cls._say("❌ Say what to do: " + ", ".join(COMMANDS))
            return 2
        if command == CMD_LOG:
            return cls._log()
        store = store or AccountsStore()
        handler = getattr(cls, f"_do_{command}")
        try:
            handler(args, store)
        except AccountsError as error:
            cls._say(f"❌ {error}")
            return 1
        return 0

    @classmethod
    def _do_list(cls, args: argparse.Namespace, store: AccountsStore) -> None:
        for user in store.list_users(sessions=args.sessions):
            flag = " service" if user["service"] else ""
            last = user["last_use"]
            used = time.strftime(STAMP, time.gmtime(last)) if last else "never"
            created = time.strftime("%Y-%m-%d", time.gmtime(user["created"]))
            cls._say(
                f"{user['username']}\t{user['role']}{flag}\tcreated {created}\tlast use {used}"
            )
            for token in user.get("tokens", []):
                cls._say(f"    session {token['machine']} ({token['client']})")
            for binding in user.get("bindings", []):
                cls._say(f"    binding {binding['kind']}: {binding['identity']}")

    @classmethod
    def _do_add(cls, args: argparse.Namespace, store: AccountsStore) -> None:
        password = getpass.getpass("Password (may be empty): ")
        if password != getpass.getpass("Again: "):
            raise AccountsError("The two passwords differ.")
        user = store.create_user(args.name, password)
        cls._say(f"✅ Created {user['username']} ({user['role']})")

    @classmethod
    def _do_reset(cls, args: argparse.Namespace, store: AccountsStore) -> None:
        cls._say(
            f"✅ One-time password for {args.name} (shown once): {store.reset_password(args.name)}"
        )
        cls._say("💡 The next sign-in must set a new password; refresh tokens are revoked.")

    @classmethod
    def _do_remove(cls, args: argparse.Namespace, store: AccountsStore) -> None:
        store.remove_user(args.name)
        cls._say(f"✅ Removed {args.name}")

    @classmethod
    def _do_promote(cls, args: argparse.Namespace, store: AccountsStore) -> None:
        store.set_role(args.name, ROLE_ADMIN)
        cls._say(f"✅ {args.name} is now admin")

    @classmethod
    def _do_demote(cls, args: argparse.Namespace, store: AccountsStore) -> None:
        store.set_role(args.name, ROLE_USER)
        cls._say(f"✅ {args.name} is now user")

    @classmethod
    def _do_key(cls, args: argparse.Namespace, store: AccountsStore) -> None:
        if args.revoke:
            store.revoke_key(args.name, args.revoke)
            cls._say(f"✅ Revoked key {args.revoke} of {args.name}")
        else:
            cls._say(cls._snippets(store.mint_key(args.name, args.key_name)))

    @classmethod
    def _do_usage(cls, args: argparse.Namespace, store: AccountsStore) -> None:
        if getattr(args, "unauthenticated", False):
            for count in AccountsUsage(store).unauthenticated(args.days):
                cls._say(
                    f"{count['day']}\t{count['node_id']}\t{count['client_kind']}\t"
                    f"{count['source']}\t{count['requests']} requests"
                )
            return
        for row in store.usage(args.days):
            cls._say(
                f"{row['day']}\t{row['username']}\t{row['client_kind']}\t{row['requests']} requests\t"
                f"{row['input_tokens']} in\t{row['output_tokens']} out"
            )

    @classmethod
    def _do_export(cls, args: argparse.Namespace, store: AccountsStore) -> None:
        cls._say(store.export_json())

    @classmethod
    def _do_import(cls, args: argparse.Namespace, store: AccountsStore) -> None:
        cls._say(f"✅ Imported {store.import_json(Path(args.file).read_text())} user(s)")

    @classmethod
    def _snippets(cls, key: str) -> str:
        node = f"https://{socket.gethostname()}.local:{DEFAULT_MODEL_PORT}"
        return (
            f"✅ Key (shown once): {key}\n\n"
            f'model_provider = "ragon"\n[model_providers.ragon]\nname = "Ragon"\n'
            f'base_url = "{node}/v1"\nwire_api = "responses"\nenv_key = "OPENAI_API_KEY"\n\n'
            f"export OPENAI_BASE_URL={node}/v1\nexport OPENAI_API_KEY={key}"
        )

    @classmethod
    def _log(cls) -> int:
        try:
            result = subprocess.run(
                ["journalctl", "--no-pager", "-t", AUDIT_TAG, "-o", "short-iso"],
                capture_output=True,
                text=True,
                check=False,
                timeout=30,
            )
        except (OSError, subprocess.SubprocessError):
            cls._say("❌ journalctl is not available")
            return 1
        sys.stdout.write(result.stdout)
        return result.returncode
