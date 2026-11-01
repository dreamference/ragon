"""Every ragon-admin command group the parser has is named in docs/dev/cli.md."""

import argparse
import re
from pathlib import Path

from dreamference.cli import DreamferenceCLIController

CLI_DOC = Path(__file__).resolve().parent.parent / "docs" / "dev" / "cli.md"


def _top_level_commands() -> list[str]:
    parser = DreamferenceCLIController.build_parser()
    for action in parser._actions:
        if isinstance(action, argparse._SubParsersAction):
            return list(action.choices)
    return []


def test_every_parser_group_is_listed_in_the_cli_notes() -> None:
    text = CLI_DOC.read_text(encoding="utf-8")
    missing = [
        name
        for name in _top_level_commands()
        if not re.search(r"`" + re.escape(name) + r"[`\s{]", text)
    ]
    assert not missing, f"docs/dev/cli.md does not list: {missing}"
