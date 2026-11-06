"""Every ``ragon-admin <group> <sub>`` named in the docs exists in the argparse tree."""

import argparse
import re
from pathlib import Path
from typing import Final

from dreamference.cli.dreamference_cli_controller import DreamferenceCLIController

ROOT: Final[Path] = Path(__file__).resolve().parent.parent
COMMAND: Final[re.Pattern[str]] = re.compile(
    r"(?<!unrelated to `)ragon-admin ([a-z][a-z0-9-]*)(?: ([a-z][a-z0-9-]*))?(?![\w./-])"
)


def _choices(parser: argparse.ArgumentParser) -> dict[str, argparse.ArgumentParser] | None:
    """Return the subcommand map of a parser, or None when it has no subparsers."""
    for action in parser._actions:
        if isinstance(action, argparse._SubParsersAction):
            return dict(action.choices)
    return None


def _doc_files() -> list[Path]:
    """Return AGENTS.md and every Markdown file under docs/."""
    return [ROOT / "AGENTS.md", *sorted((ROOT / "docs").rglob("*.md"))]


def _mentions() -> list[tuple[str, str, str | None]]:
    """Return (location, group, sub) for every documented command."""
    found = []
    for path in _doc_files():
        for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            where = f"{path.relative_to(ROOT)}:{number}"
            found.extend((where, *match.groups()) for match in COMMAND.finditer(line))
    return found


def _problems() -> list[str]:
    """Collect documented commands that the parser does not know."""
    top = _choices(DreamferenceCLIController.build_parser()) or {}
    found: list[str] = []
    for where, group, sub in _mentions():
        subs = _choices(top[group]) if group in top else None
        unknown = group not in top or (sub is not None and subs is not None and sub not in subs)
        if unknown:
            found.append(f"{where}: ragon-admin {group} {sub or ''}".strip())
    return found


def test_the_parser_has_commands_to_check() -> None:
    """The guard is not vacuous: the parser has groups and the docs name commands."""
    assert _choices(DreamferenceCLIController.build_parser())
    assert any(COMMAND.search(p.read_text(encoding="utf-8")) for p in _doc_files())


def test_every_documented_ragon_admin_command_exists() -> None:
    """A command removed from the parser must also leave the docs."""
    assert _problems() == []
