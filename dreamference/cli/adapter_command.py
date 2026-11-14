"""`ragon-admin adapter {list,install,remove,verify}` (specs/DREAMFERENCE_RAGON_ADAPTERS.md §3).

Adapters are a node's: they are installed where the model is served, and a client sees them
through the node's `/v1/models`. Only `install` reaches the network; `list`, `list --available`,
`verify` and `remove` read this machine alone.
"""

import argparse
import dataclasses
import importlib
import json
import sys
from pathlib import Path
from typing import Final

from dreamference.config import DreamferenceConfig
from dreamference.hardware.adapter_catalog import AdapterCatalog
from dreamference.hardware.adapter_installer import AdapterInstaller
from dreamference.hardware.adapter_manifest import AdapterManifest
from dreamference.hardware.adapter_store import TRUST_THIRD_PARTY, AdapterStore
from dreamference.hardware.adapter_task_candidate import TaskCandidate
from dreamference.hardware.adapter_train_run import (
    DEFAULT_EPOCHS,
    STAGE_CLASSES,
    STAGES,
    AdapterTrainRun,
)
from dreamference.hardware.adapter_train_tasks import AdapterTrainTasks
from dreamference.hardware.adapter_verifier import AdapterVerifier
from dreamference.vllm_server.adapter_serving import AdapterServing

ADAPTER_COMMAND: Final[str] = "adapter"
TRAINER_SCHEME: Final[str] = "ssh://"
STAGE_DATA: Final[str] = "data"
STAGE_TRAIN: Final[str] = "train"
HOST_FORBIDDEN: Final[str] = "/ "
DEFAULT_RANK: Final[int] = 16

THIRD_PARTY_WARNING: Final[str] = """⚠️  This adapter is not signed by Dreamference.
    An adapter changes what the model writes: it can make the model suggest
    insecure code, ignore your instructions or leave out what it was asked to keep.
    Ragon will check that its files match its manifest and that it was made
    for the model this machine serves, and nothing else."""

RESTART_HINT: Final[str] = (
    "💡 The model server loads installed adapters at its next start: `ragon-admin server start`. "
    "Then choose one per session with `ragon --adapter {name}` or `/adapter {name}`."
)


class AdapterCommand:
    """Parses and runs the `adapter` group."""

    @classmethod
    def add_parser(
        cls, subparsers: "argparse._SubParsersAction[argparse.ArgumentParser]"
    ) -> argparse.ArgumentParser:
        """Adds `adapter` and its subcommands.

        Args:
            subparsers: The top-level parser's subcommands.

        Returns:
            argparse.ArgumentParser: The group's parser.
        """
        parser = subparsers.add_parser(
            ADAPTER_COMMAND, help="Install and manage LoRA adapters for the served model"
        )
        commands = parser.add_subparsers(dest="adapter_command")
        listing = commands.add_parser("list", help="Show the installed adapters (no network)")
        listing.add_argument(
            "--available",
            action="store_true",
            help="Also list the adapters this release catalogues",
        )
        listing.add_argument("--json", action="store_true", help="Print JSON")
        install = commands.add_parser(
            "install",
            help="Download, verify and install an adapter (the only adapter step that uses the network)",
        )
        install.add_argument(
            "name",
            nargs="?",
            default=None,
            help="A catalogued adapter, as <name> or <name>@<version>",
        )
        install.add_argument(
            "--repo",
            default=None,
            help="An adapter published after this release: <org>/<repo>@<commit>",
        )
        install.add_argument(
            "--third-party",
            action="store_true",
            help="Install an adapter that is not signed by Dreamference (asks you to type its name)",
        )
        install.add_argument(
            "--hf-token",
            default=argparse.SUPPRESS,
            help="A Hugging Face token, for a private repository",
        )
        remove = commands.add_parser("remove", help="Remove an installed adapter and its files")
        remove.add_argument("name", help="The installed adapter")
        verify = commands.add_parser(
            "verify", help="Check installed adapters again: signature, files and the served base"
        )
        verify.add_argument("name", nargs="?", default=None, help="One adapter (default: all)")
        train = commands.add_parser(
            "train",
            help="Train a LoRA adapter on a repository's own history (stages; --dry-run changes nothing)",
        )
        train.add_argument("name", help="The adapter's name (the run's folder name)")
        train.add_argument("--repo", required=True, help="The git repository to learn from")
        train.add_argument("--test-cmd", default=None, help="The repository's test command")
        train.add_argument("--language", default=None, help="The repository's main language")
        train.add_argument(
            "--trainer",
            default=None,
            help="Train on this host instead of locally: ssh://HOST (sends data off the node)",
        )
        train.add_argument("--budget-hours", type=float, default=None, help="GPU hours to spend")
        train.add_argument("--rank", type=int, default=DEFAULT_RANK, help="LoRA rank")
        train.add_argument(
            "--epochs", type=int, default=DEFAULT_EPOCHS, help="Passes over the data"
        )
        train.add_argument("--stage", choices=STAGES, default=None, help="Run this stage alone")
        train.add_argument("--resume", action="store_true", help="Continue an existing run")
        train.add_argument(
            "--dry-run", action="store_true", help="Print the plan and change nothing"
        )
        return parser

    @classmethod
    def run(cls, args: argparse.Namespace, config: DreamferenceConfig) -> int:
        """Runs one subcommand.

        Args:
            args: The parsed arguments.
            config: The configuration (the model server's address, the configured model, the Hub
                token).

        Returns:
            int: The exit code.
        """
        from dreamference.node.node_identity import NodeIdentity

        if args.adapter_command == STAGE_TRAIN and args.dry_run:
            return cls._train(args)
        if NodeIdentity.read() is None:
            print(
                f"💡 Adapters are installed on the node that serves the model ({config.vllm_host}): "
                f"run `ragon-admin adapter {args.adapter_command}` there. This machine sees the node's "
                "adapters in its /v1/models and needs nothing installed."
            )
            return 1
        base = AdapterVerifier.served_base(config.vllm_host, config.model)
        handlers = {
            "list": cls._list,
            "install": lambda args, base: cls._install(args, base, config),
            "remove": cls._remove,
            "verify": cls._verify,
            "train": lambda args, base: cls._train(args),
        }
        return handlers[args.adapter_command](args, base)

    @classmethod
    def _list(cls, args: argparse.Namespace, base: dict[str, str] | None) -> int:
        """`adapter list [--available] [--json]`."""
        installed = AdapterStore.entries()
        rows = [
            {
                **{
                    key: entry.get(key)
                    for key in (
                        "name",
                        "version",
                        "display_name",
                        "repo",
                        "commit",
                        "rank",
                        "trust",
                    )
                },
                "base": entry.get("base_repo"),
                "base_matches": base is not None
                and (entry.get("base_repo"), entry.get("base_revision"))
                == (base["repo"], base["revision"]),
            }
            for _, entry in sorted(installed.items())
        ]
        available = AdapterCatalog.available() if args.available else []
        if args.json:
            print(
                json.dumps(
                    {"installed": rows, "available": available, "base": base}, indent=2, default=str
                )
            )
            return 0
        served = f"{base['key']} ({base['source']})" if base else "no model of the matrix"
        print(f"Served base: {served}")
        if not rows:
            print("No adapters installed.")
        for row in rows:
            flag = " ⚠️  third-party" if row["trust"] == TRUST_THIRD_PARTY else ""
            match = "" if row["base_matches"] else "  base mismatch"
            print(
                f"  {row['name']} {row['version']}  {row['display_name']}  rank {row['rank']}  "
                f"{row['repo']}@{str(row['commit'])[:12]}{flag}{match}"
            )
        if args.available:
            print(
                "Catalogued in this release:"
                if available
                else "This release catalogues no adapters yet."
            )
            for entry in available:
                mark = "installed" if entry["name"] in installed else "not installed"
                print(
                    f"  {entry['name']} {entry.get('version')}  {entry.get('display_name')}  "
                    f"for {entry.get('base_alias')}  ({mark})"
                )
        return 0

    @classmethod
    def _install(
        cls, args: argparse.Namespace, base: dict[str, str] | None, config: DreamferenceConfig
    ) -> int:
        """`adapter install <name>[@<version>] | --repo <org>/<repo>@<commit> [--third-party]`.

        The token is the configuration's, in the model download's order (INFERENCE §2.1):
        `--hf-token`, `HF_TOKEN`, `DREAMFERENCE_HF_TOKEN`, then the config file's `hf_token`.
        """
        if (args.name is None) == (args.repo is None):
            print("❌ Name a catalogued adapter or give --repo <org>/<repo>@<commit>, not both.")
            return 2
        if args.third_party and args.repo is None:
            print(
                "❌ --third-party needs --repo <org>/<repo>@<commit>: the catalogue lists Dreamference's adapters only."
            )
            return 2
        confirm = cls._confirm if args.third_party else None
        try:
            entry = AdapterInstaller.install(
                args.name, args.repo, base, confirm=confirm, token=config.hf_token
            )
        except ValueError as error:
            print(f"❌ {error}. Nothing was installed.")
            return 1
        flag = (
            " (third-party)" if entry["trust"] == TRUST_THIRD_PARTY else ", signed by Dreamference"
        )
        print(
            f"✅ Installed {entry['display_name']} {entry['version']} as `{entry['name']}`{flag}."
        )
        print(RESTART_HINT.format(name=entry["name"]))
        cls._warn_if_ragon(base, config)
        return 0

    @classmethod
    def _warn_if_ragon(cls, base: dict[str, str] | None, config: DreamferenceConfig) -> None:
        """Warns when the engine the next `server start` resolves is ragon-serve, which serves no adapters."""
        from dreamference.hardware import ModelMatrixRegistry
        from dreamference.hardware.model_matrix_registry import (
            ENGINE_FEATURE_ADAPTERS,
            ENGINE_RAGON,
        )

        if base is None:
            return
        engines = ModelMatrixRegistry.engines(base["key"])
        engine = config.server_engine or (engines[0] if engines else None)
        if engine == ENGINE_RAGON and not ModelMatrixRegistry.engine_has_feature(
            base["key"], ENGINE_RAGON, ENGINE_FEATURE_ADAPTERS
        ):
            print(
                "⚠️  The engine configured for this model is ragon-serve, which serves no adapters yet: "
                "`ragon-admin server start` will use SGLang while an adapter is installed "
                "(`--engine ragon` is refused)."
            )

    @classmethod
    def _confirm(cls, manifest: AdapterManifest) -> bool:
        """The typed confirmation of a third-party adapter: its name, at a terminal, no `--yes`."""
        print(THIRD_PARTY_WARNING)
        print(
            f"    It is {manifest.display_name} ({manifest.name} {manifest.version}): {manifest.description}"
        )
        # Piped input is a script or an agent, which the typed name exists to keep out.
        if not sys.stdin.isatty():
            print("❌ A third-party adapter is confirmed at a terminal: run this command in one.")
            return False
        try:
            typed = input("    Type the adapter's name to install it: ")
        except EOFError:
            return False
        return typed.strip() == manifest.name

    @classmethod
    def _remove(cls, args: argparse.Namespace, base: dict[str, str] | None) -> int:
        """`adapter remove <name>`."""
        if not AdapterInstaller.remove(args.name):
            print(f"❌ No adapter named {args.name} is installed (`ragon-admin adapter list`).")
            return 1
        AdapterServing.withdraw(args.name)
        print(
            f"✅ Removed {args.name}. A session using it is refused at its next request; "
            "`/adapter off` or a new session returns to the base model."
        )
        return 0

    @classmethod
    def _verify(cls, args: argparse.Namespace, base: dict[str, str] | None) -> int:
        """`adapter verify [<name>]`."""
        entries = AdapterStore.entries()
        if args.name is not None:
            if args.name not in entries:
                print(f"❌ No adapter named {args.name} is installed.")
                return 1
            entries = {args.name: entries[args.name]}
        if not entries:
            print("No adapters installed.")
            return 0
        failed = 0
        for name, entry in sorted(entries.items()):
            problem = AdapterInstaller.verify(entry, base)
            if problem:
                failed += 1
                print(f"❌ {name}: {problem}")
            else:
                print(f"✅ {name}: signature, files and base check out")
        return 1 if failed else 0

    @classmethod
    def _trainer_host(cls, trainer: str | None) -> str | None:
        """The host a `--trainer` names, or None for a local run.

        Args:
            trainer: The option's value.

        Returns:
            str | None: The host name.

        Raises:
            ValueError: When the value is not `ssh://HOST`.
        """
        if trainer is None:
            return None
        host = trainer[len(TRAINER_SCHEME) :] if trainer.startswith(TRAINER_SCHEME) else ""
        if not host or host.startswith("-") or any(c in host for c in HOST_FORBIDDEN):
            raise ValueError(f"--trainer must be ssh://HOST, not {trainer!r}")
        return host

    @classmethod
    def _train(cls, args: argparse.Namespace) -> int:
        """Plan or run the next stage of a training run.

        Args:
            args: The parsed `train` arguments.

        Returns:
            int: The exit code.
        """
        checked = cls._preflight(args)
        if isinstance(checked, int):
            return checked
        run, host, repo, training, holdout = checked
        if args.dry_run:
            return cls._plan(args, run, host, len(training), len(holdout))
        stage = args.stage or run.next_stage()
        if args.stage is None and run.state()["done"] and not args.resume:
            print(f"❌ The run {args.name!r} exists: add --resume, or pick another name.")
            stage = None
        elif stage is None:
            print(f"✅ Every stage of {args.name!r} is done.")
            return 0
        if stage is None:
            return 1
        run.write_config(
            {
                "repo": str(repo),
                "test_cmd": args.test_cmd,
                "language": args.language,
                "trainer": args.trainer,
                "budget_hours": args.budget_hours,
                "rank": args.rank,
                "epochs": args.epochs,
            }
        )
        if stage == STAGE_DATA:
            return cls._write_tasks(run, training, holdout)
        return cls._dispatch(stage, run, host)

    @classmethod
    def _write_tasks(
        cls,
        run: AdapterTrainRun,
        training: tuple[TaskCandidate, ...],
        holdout: tuple[TaskCandidate, ...],
    ) -> int:
        """Run the data stage: keep the mined tasks in the run directory.

        Args:
            run: The run.
            training: The training tasks.
            holdout: The hold-out tasks.

        Returns:
            int: 0.
        """
        payload = {
            "train": [dataclasses.asdict(t) for t in training],
            "holdout": [dataclasses.asdict(t) for t in holdout],
        }
        (run.directory / "tasks.json").write_text(json.dumps(payload, indent=2) + "\n")
        run.mark_done(STAGE_DATA)
        print(
            f"✅ data: {len(training)} tasks, {len(holdout)} held out, "
            f"in {run.directory / 'tasks.json'}"
        )
        return 0

    @classmethod
    def _preflight(
        cls, args: argparse.Namespace
    ) -> "int | tuple[AdapterTrainRun, str | None, Path, tuple[TaskCandidate, ...], tuple[TaskCandidate, ...]]":
        """Check the arguments, the egress gate and the repository.

        Args:
            args: The parsed `train` arguments.

        Returns:
            An exit code when the run must stop, else the run, the trainer host, the repository
            and the training and hold-out tasks.
        """
        from dreamference.config.airgapped_resolver import AIRGAPPED_OFF, AirgappedResolver

        try:
            host = cls._trainer_host(args.trainer)
            run = AdapterTrainRun(args.name)
            repo = Path(args.repo).expanduser()
            if not (repo / ".git").exists():
                raise ValueError(f"{repo} is not a git repository")
        except ValueError as error:
            print(f"❌ {error}")
            return 2
        if host is not None and AirgappedResolver.level(cwd=repo) != AIRGAPPED_OFF:
            print(
                f"❌ Training on {host} sends data off this node, which /airgapped on refuses "
                "(it also refuses when the level cannot be read). Turn it off or train locally."
            )
            return 1
        try:
            training, holdout = AdapterTrainTasks.collect(repo)
        except RuntimeError as error:
            print(f"❌ {error}")
            return 1
        return run, host, repo, training, holdout

    @classmethod
    def _consent(cls, run: AdapterTrainRun, host: str) -> bool:
        """Ask for the host's name once, before the first upload to it.

        Args:
            run: The run, which remembers the answer.
            host: The trainer host.

        Returns:
            bool: True when the host is known or the person typed its name.
        """
        if host in run.state()["consented"]:
            return True
        print(
            f"💡 The packed training data (and nothing else) will be sent to {host}. "
            f"Type the host name to allow it: ",
            end="",
        )
        if input().strip() != host:
            print("❌ Not confirmed; nothing was sent.")
            return False
        run.record_consent(host)
        return True

    @classmethod
    def _dispatch(cls, stage: str, run: AdapterTrainRun, host: str | None) -> int:
        """Run a stage's class, or say which lane brings it.

        Args:
            stage: One of the stages after `data`.
            run: The run.
            host: The trainer host, None for local.

        Returns:
            int: The stage's exit code; 1 when its class is not in this release or the host
                was not confirmed.
        """
        if stage == STAGE_TRAIN and host is not None and not cls._consent(run, host):
            return 1
        module, name, lane = STAGE_CLASSES[stage]
        try:
            stage_class = getattr(importlib.import_module(module), name)
        except (ImportError, AttributeError):
            print(f"❌ The {stage} stage is not in this release yet: lane {lane} brings it.")
            return 1
        code = int(stage_class.run(run))
        if code == 0:
            run.mark_done(stage)
        return code

    @classmethod
    def _plan(
        cls,
        args: argparse.Namespace,
        run: AdapterTrainRun,
        host: str | None,
        training: int,
        holdout: int,
    ) -> int:
        """Print what a run would do and change nothing.

        Args:
            args: The parsed `train` arguments.
            run: The run (only its directory is named).
            host: The trainer host, None for local.
            training: Candidate tasks outside the hold-out.
            holdout: The hold-out's size.

        Returns:
            int: 0.
        """
        estimate = AdapterTrainRun.estimate(
            training, epochs=args.epochs, budget_hours=args.budget_hours
        )
        print(f"Plan for {args.name!r} (dry run: nothing is written or sent)")
        print(
            f"  candidate tasks: {training} for training, {holdout} held out "
            "(gradability is known after the generate stage)"
        )
        print(
            f"  token estimate: {estimate['tokens']:,.0f} over {args.epochs} epochs; "
            f"GPU hours: {estimate['gpu_hours']:.1f} at "
            f"{estimate['tokens_per_second']:.0f} tokens/s (default rate; a pilot replaces it)"
        )
        print(f"  trainer: {host if host else 'local (this node)'}")
        print(f"  run directory: {run.directory}")
        if host:
            print(f"  leaves the node: the packed training jsonl, sent to {host} only")
        else:
            print("  leaves the node: nothing")
        return 0
