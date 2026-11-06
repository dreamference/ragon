#!/usr/bin/env python3
r"""The 1.0 install and update matrix (release scope §11.3) as a checklist with evidence.

Every cell is one install, update or rollback, followed by a cold start, a real conversation
with tools and ``ragon web``. A cell passes only with a pasted result: no evidence, no pass.
The rows the Windows programme owns are marked so its results are recorded by that programme.

State lives in one JSON file (``--state``, default ``release-matrix.json``), written by
``record`` and read by ``show`` and ``check``. Machine-free: it runs nothing itself.

    release_matrix.py show   [--json]
    release_matrix.py record ROW COLUMN pass|fail (--evidence TEXT | --evidence-file PATH)
    release_matrix.py check            # exit 0 only if every cell passes

Exit codes: 0 all pass (or the command succeeded), 1 cells open or failed, 2 bad input.
"""

from __future__ import annotations

import argparse
import json
import sys
from datetime import UTC, datetime
from pathlib import Path
from typing import Final

OK: Final[int] = 0
OPEN: Final[int] = 1
ERROR: Final[int] = 2

COLUMNS: Final[tuple[str, ...]] = ("fresh", "update", "rollback")
PASS: Final[str] = "pass"
FAIL: Final[str] = "fail"
RELEASE_OWNER: Final[str] = "release"
RECORD: Final[str] = "record"
SHOW: Final[str] = "show"
CHECK: Final[str] = "check"
WINDOWS_OWNER: Final[str] = "windows programme"

# (row id, label, columns that apply, owner)
ROWS: Final[tuple[tuple[str, str, tuple[str, ...], str], ...]] = (
    ("gb10-node", "GB10 node", COLUMNS, "release"),
    ("gb10-node-adapter", "GB10 node with an adapter installed", COLUMNS, "release"),
    ("amd-windows-node", "AMD Windows node (install.ps1)", COLUMNS, WINDOWS_OWNER),
    ("paired-node-gb10", "Second node paired over SSH: GB10", COLUMNS[:2], "release"),
    (
        "paired-node-amd-windows",
        "Second node paired over SSH: AMD Windows",
        COLUMNS[:2],
        WINDOWS_OWNER,
    ),
    ("linux-client", "Linux x86-64 client", COLUMNS[:2], "release"),
    ("windows-client", "Windows client", COLUMNS[:2], WINDOWS_OWNER),
    ("macos-client", "macOS client", COLUMNS[:2], "release"),
    ("desktop-gb10", "Desktop app on a GB10 node", COLUMNS[:2], "release"),
    ("desktop-windows", "Desktop app on Windows", COLUMNS[:2], WINDOWS_OWNER),
    ("bridges", "Bridges: state survives update, still off where off", ("update",), "release"),
)


class MatrixError(Exception):
    """Input that cannot be recorded: an unknown cell, or a pass with no evidence."""


def cells() -> list[tuple[str, str]]:
    """Every (row, column) the matrix asks for, in table order."""
    return [(row, column) for row, _label, columns, _owner in ROWS for column in columns]


def load(path: Path) -> dict:
    """The recorded state, or an empty one when the file does not exist yet."""
    if not path.exists():
        return {"cells": {}}
    try:
        state = json.loads(path.read_text())
    except json.JSONDecodeError as error:
        raise MatrixError(f"{path} is not valid JSON: {error}") from error
    if not isinstance(state, dict) or not isinstance(state.get("cells"), dict):
        raise MatrixError(f"{path} has no 'cells' object")
    return state


def record(state: dict, row: str, column: str, result: str, evidence: str) -> None:
    """Set one cell. A pass needs non-blank evidence; a fail is kept with whatever was seen."""
    if (row, column) not in cells():
        raise MatrixError(f"no such cell: {row}/{column}")
    if result not in (PASS, FAIL):
        raise MatrixError("the result is 'pass' or 'fail'")
    if result == PASS and not evidence.strip():
        raise MatrixError(f"{row}/{column}: a pass needs a pasted result as evidence")
    state["cells"][f"{row}/{column}"] = {
        "result": result,
        "evidence": evidence.strip(),
        "at": datetime.now(UTC).strftime("%Y-%m-%dT%H:%M:%SZ"),
    }


def status(state: dict) -> dict[str, str]:
    """Each cell's status: pass, fail or open. A recorded pass without evidence counts as open."""
    out: dict[str, str] = {}
    for row, column in cells():
        entry = state["cells"].get(f"{row}/{column}") or {}
        result = entry.get("result")
        if result == PASS and str(entry.get("evidence", "")).strip():
            out[f"{row}/{column}"] = "pass"
        elif result == FAIL:
            out[f"{row}/{column}"] = "fail"
        else:
            out[f"{row}/{column}"] = "open"
    return out


def render(state: dict) -> str:
    """The matrix as text: one line per row, a mark per column, then the totals."""
    marks = {"pass": "✓", "fail": "✗", "open": "·"}
    cell_status = status(state)
    lines = []
    for row, label, columns, owner in ROWS:
        shown = "  ".join(f"{column}:{marks[cell_status[f'{row}/{column}']]}" for column in columns)
        suffix = f"  [{owner}]" if owner != RELEASE_OWNER else ""
        lines.append(f"{label:<58} {shown}{suffix}")
    counts = {key: list(cell_status.values()).count(key) for key in marks}
    lines.append(
        f"pass {counts['pass']}, fail {counts['fail']}, open {counts['open']} of {len(cell_status)}"
    )
    return "\n".join(lines)


def main(argv: list[str] | None = None) -> int:
    """Command line entry point."""
    parser = argparse.ArgumentParser(description="The 1.0 install and update matrix.")
    parser.add_argument("--state", type=Path, default=Path("release-matrix.json"))
    sub = parser.add_subparsers(dest="command", required=True)
    show = sub.add_parser("show", help="print the matrix with its evidence status")
    show.add_argument("--json", action="store_true")
    rec = sub.add_parser("record", help="record one cell's result and evidence")
    rec.add_argument("row")
    rec.add_argument("column")
    rec.add_argument("result", choices=(PASS, FAIL))
    group = rec.add_mutually_exclusive_group()
    group.add_argument("--evidence", default="")
    group.add_argument("--evidence-file", type=Path)
    sub.add_parser("check", help="exit 0 only if every cell passes")
    args = parser.parse_args(argv)
    try:
        state = load(args.state)
        if args.command == RECORD:
            text = args.evidence_file.read_text() if args.evidence_file else args.evidence
            record(state, args.row, args.column, args.result, text)
            args.state.write_text(json.dumps(state, indent=2, sort_keys=True) + "\n")
            print(f"✅ recorded {args.row}/{args.column}: {args.result}")  # noqa: T201
            return OK
    except (MatrixError, OSError) as error:
        print(f"❌ {error}", file=sys.stderr)  # noqa: T201
        return ERROR
    if args.command == SHOW and args.json:
        print(json.dumps({"status": status(state), "cells": state["cells"]}, indent=2))  # noqa: T201
        return OK
    print(render(state))  # noqa: T201
    if args.command == CHECK:
        return OK if all(value == PASS for value in status(state).values()) else OPEN
    return OK


if __name__ == "__main__":
    raise SystemExit(main())
