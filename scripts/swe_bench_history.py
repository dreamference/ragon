"""The history of every published SWE-bench number, regenerated from docs/swe-bench-runs.json.

The coordinator appends one entry to docs/swe-bench-runs.json after each graded run; this script
rewrites docs/benchmarks/swe-bench-history.md from it. The page holds one table per slice with every
run (date, build/engine, recipe, resolved out of the slice's tasks, timeouts, starved, and the gross
flips against the previous run on that slice), the spread statement, and a one-paragraph method.

No box name, address, user name or home path reaches the page: the data is checked, as the release
entries are, and the flip counts are recomputed here from the data's own pair lists, never from a
report held on a box.

Usage::

    python3 scripts/swe_bench_history.py            # rewrite docs/benchmarks/swe-bench-history.md
    python3 scripts/swe_bench_history.py --check    # fail when the page is not what the data renders to

Standard library only. Exit 0 on success; 1 with one line on standard error otherwise.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Final

ROOT: Final[Path] = Path(__file__).resolve().parent.parent
DATA: Final[Path] = ROOT / "docs" / "swe-bench-runs.json"
PAGE: Final[Path] = ROOT / "docs" / "benchmarks" / "swe-bench-history.md"
# What must never reach the page: a path, an address, an account or a host name.
FORBIDDEN: Final[re.Pattern] = re.compile(
    r"/home/|/Users/|@|\b\d{1,3}(\.\d{1,3}){3}\b|\.local\b|gx10", re.IGNORECASE
)
TASK_NAME: Final[re.Pattern] = re.compile(r"[0-9A-Za-z._-]+")
MISSING: Final[str] = "n/a"
NOT_CHECKED: Final[str] = "not checked"
# Two or more runs on a slice is where a spread can be measured.
MINIMUM_SPREAD_RUNS: Final[int] = 2
REQUIRED_RUN_FIELDS: Final[tuple[str, ...]] = (
    "date",
    "run",
    "build",
    "engine",
    "recipe",
    "resolved",
)
COUNT_FIELDS: Final[tuple[str, ...]] = ("timeouts", "starved")


class HistoryError(Exception):
    """The data is missing a field, inconsistent, or carries something that must not be published."""


def load() -> dict:
    """Reads and checks the run log.

    Returns:
        dict: The document, its slices in file order, each run dated.
    """
    raw = DATA.read_text(encoding="utf-8")
    if FORBIDDEN.search(raw):
        raise HistoryError("the run log carries a path, address, account or host name")
    try:
        document = json.loads(raw)
    except ValueError as error:
        raise HistoryError(f"{DATA.name}: unreadable ({error})") from error
    for slice_record in document["slices"]:
        for run in slice_record["runs"]:
            check_run(run, slice_record)
    return document


def check_run(run: dict, slice_record: dict) -> None:
    """Checks one run entry of the log.

    Args:
        run: The parsed entry.
        slice_record: Its slice, whose task count bounds the resolved count.

    Raises:
        HistoryError: Naming what is wrong.
    """
    label = f"{slice_record['name']}: run {run.get('run')!r}"
    for key in REQUIRED_RUN_FIELDS:
        if key not in run:
            raise HistoryError(f"{label} lacks {key!r}")
    if not 0 <= run["resolved"] <= slice_record["tasks"]:
        raise HistoryError(f"{label} resolves {run['resolved']} of {slice_record['tasks']}")
    for key in COUNT_FIELDS:
        value = run.get(key)
        if value is not None and not (isinstance(value, int) and value >= 0):
            raise HistoryError(f"{label}: {key} is not a count")
    resolved_only = run.get("resolved_only")
    if resolved_only is not None and not all(
        TASK_NAME.fullmatch(str(name)) for name in resolved_only
    ):
        raise HistoryError(f"{label}: a flipped task name")


def gross_flips(current: dict, previous: dict | None) -> int | None:
    """The gross flipped tasks between two runs of the same slice.

    A flip is an instance resolved in exactly one of the two runs: both directions together, so it
    is the run-to-run noise even when the counts cancel. Where the coordinator recorded the lists of
    instances each run resolved alone, the flips are the lists' sizes; without them they are the
    difference of the resolved counts, which the spread statement says so.

    Args:
        current: The later run.
        previous: The earlier run it is compared against, None for the slice's first run.

    Returns:
        Optional[int]: The flips, or None when the earlier run is unknown.
    """
    if previous is None:
        return None
    only_current = current.get("resolved_only")
    only_previous = previous.get("resolved_only")
    if only_current is not None and only_previous is not None:
        return len(only_current) + len(only_previous)
    return abs(current["resolved"] - previous["resolved"])


def flips_between(slice_record: dict) -> list[int]:
    """The gross flips of every run against its predecessor, in order.

    Args:
        slice_record: The slice.

    Returns:
        List[int]: One entry per run past the first.
    """
    runs = slice_record["runs"]
    return [
        flips
        for flips in (gross_flips(runs[index], runs[index - 1]) for index in range(1, len(runs)))
        if flips is not None
    ]


def timeouts_cell(run: dict) -> str:
    """The run's timeouts and, where checked, its starved ones."""
    timeouts = run.get("timeouts")
    if timeouts is None:
        return MISSING
    cell = f"{timeouts} timeout" + ("s" if timeouts != 1 else "")
    starved = run.get("starved")
    if starved is None:
        return f"{cell}, starved {NOT_CHECKED}"
    return f"{cell}, {starved} starved"


def slice_table(slice_record: dict) -> list[str]:
    """One slice's section: its heading, run table and spread statement."""
    tasks = slice_record["tasks"]
    lines = [
        f"## {slice_record['name']} ({tasks} tasks, `{slice_record['list']}`)",
        "",
        "| Date | Run | Build / engine | Recipe | Resolved | Timeouts | Gross flips vs previous |",
        "| --- | --- | --- | --- | --- | --- | --- |",
    ]
    for index, run in enumerate(slice_record["runs"]):
        previous = slice_record["runs"][index - 1] if index else None
        flips = gross_flips(run, previous)
        flips_text = "first" if flips is None else f"{flips} of {tasks}"
        lines.append(
            f"| {run['date']} | {run['run']} | {run['build']} / {run['engine']} | "
            f"{run['recipe']} | {run['resolved']} of {tasks} | {timeouts_cell(run)} | {flips_text} |"
        )
    lines.extend(["", spread_statement(slice_record)])
    return lines


def spread_statement(slice_record: dict) -> str:
    """The slice's spread: the range of its resolved counts and the flips between its runs."""
    runs = slice_record["runs"]
    counts = [run["resolved"] for run in runs]
    if len(counts) < MINIMUM_SPREAD_RUNS:
        return "One run so far; the run-to-run spread on this slice is not measured yet."
    range_text = (
        f"its {len(counts)} runs resolve {min(counts)} to {max(counts)} of {slice_record['tasks']}"
    )
    flips = flips_between(slice_record)
    if not flips:
        return f"{range_text}; the flips between its runs are not recorded."
    low, high = min(flips), max(flips)
    flip_text = (
        f"{low} gross flipped tasks between its runs"
        if low == high
        else f"{low} to {high} gross flipped tasks between its runs"
    )
    return f"Spread: {range_text}; {flip_text}."


def render(document: dict) -> str:
    """The page for the run log.

    Args:
        document: The checked document.

    Returns:
        str: The page's Markdown.
    """
    lines = [
        "# SWE-bench: history of every published number",
        "",
        "Every published SWE-bench number, with its history and its spread, on one page: one table "
        "per slice, every run on it in date order, and the spread between the runs.",
        "",
    ]
    for slice_record in document["slices"]:
        lines.extend(slice_table(slice_record))
        lines.append("")
    lines.extend(["## Method", "", document["method"], ""])
    return "\n".join(lines)


def main(argv: list[str] | None = None) -> int:
    """Runs the command line.

    Args:
        argv: The arguments; ``sys.argv`` by default.

    Returns:
        int: The exit status.
    """
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--check",
        action="store_true",
        help="fail when the page is not what the run log renders to, instead of rewriting it",
    )
    args = parser.parse_args(argv)
    try:
        text = render(load())
        if args.check:
            if PAGE.read_text(encoding="utf-8") != text:
                raise HistoryError(
                    f"{PAGE.name} is not what {DATA.name} renders to; rerun without --check"
                )
        else:
            PAGE.parent.mkdir(parents=True, exist_ok=True)
            PAGE.write_text(text, encoding="utf-8")
    except (HistoryError, KeyError, TypeError, OSError) as error:
        print(str(error), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
