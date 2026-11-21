"""Refresh the RBRB public artefacts with the running week so far.

python scripts/rbrb_refresh.py            # export the running week, ingest it provisionally, regenerate, stage
python scripts/rbrb_refresh.py --commit   # the same, then commit the four files as the docs team bot

Does nothing when `rbrb-gate` is not on PATH. Only the aggregate week numbers of the export are read (the ledger
stays private). It never pushes itself. The as_team.py flush runs this script first, inside a throwaway worktree on
a branch rbrb-refresh-<stamp> from a fresh origin/main (never in the checkout), and the flush pushes the resulting
commit to origin main with a fast-forward-only push; a refused push is retried by the next flush.
"""

import argparse
import datetime
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

import rbrb

ROOT = rbrb.ROOT
PATHS = ("docs/rbrb.json", "images/rbrb.svg", "images/rbrb-badge.svg", "README.md")
TEAM = "docs"


def running_week(today: datetime.date) -> str:
    """The ISO week of `today`, e.g. "2026-W47"."""
    year, num, _ = today.isocalendar()
    return f"{year}-W{num:02d}"


def _git(*args: str) -> str:
    return subprocess.run(
        ["git", "-C", str(ROOT), *args], capture_output=True, text=True, check=True
    ).stdout


def refresh(commit: bool, today: datetime.date | None = None) -> int:
    """Export the running week, ingest it provisionally, regenerate and stage; optionally commit.

    Returns an exit code.
    """
    gate = shutil.which("rbrb-gate")
    if gate is None:
        print("rbrb-gate is not on PATH: nothing to refresh")
        return 0
    week = running_week(today or datetime.datetime.now(datetime.timezone.utc).date())
    with tempfile.TemporaryDirectory() as folder:
        export = Path(folder) / "export.json"
        done = subprocess.run(
            [gate, "--export", str(export), "--week", week], capture_output=True, text=True
        )
        if done.returncode != 0 or not export.exists():
            print(f"rbrb-gate could not export {week}", file=sys.stderr)
            return 1
        if rbrb.main(["--ingest", str(export), "--provisional"]) != 0:
            return 1
    _git("add", "--", *PATHS)
    if not commit:
        return 0
    staged = _git("diff", "--cached", "--name-only").split()
    if not staged:
        print(f"RBRB {week}: unchanged")
        return 0
    if set(staged) - set(PATHS):
        print("refusing to commit: other files are staged", file=sys.stderr)
        return 1
    script = ROOT / "scripts" / "as_team.py"
    subprocess.run(
        [
            sys.executable,
            str(script),
            "commit",
            TEAM,
            "-m",
            f"docs: RBRB {week} so far",
            "--ragon-tokens",
            "0",
        ],
        cwd=ROOT,
        check=True,
    )
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--commit", action="store_true", help="commit the staged files as the docs team bot"
    )
    return refresh(parser.parse_args(argv).commit)


if __name__ == "__main__":
    sys.exit(main())
