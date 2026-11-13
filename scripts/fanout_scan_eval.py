#!/usr/bin/env python3
"""Measures `fanout_scan.py` on this repository's own fixed bugs, against a one-call baseline.

    fanout_scan_eval.py cases --out DIR [--repo ROOT] [--rev origin/main] [--limit N]
    fanout_scan_eval.py run   --out DIR [--host URL] [--concurrency N] [--split eval|dev|all]
    fanout_scan_eval.py score --out DIR [--host URL] [--no-judge]

`cases` mines the history for bug-fix commits that changed one to three Python or Rust source
files a little, usually with a test: the subject says what was broken or fixed. For each it keeps
the primary file before the fix (`cases/<sha>/before`) and after it (`after`), and the old-side
line ranges the fix touched, which is where the known bug is. It writes `cases.json` with the
count left after each filter. A few cases are set aside as `dev` for tuning the prompts and are
never scored.

`run` scans each case's before and after files with the fan-out and the baseline and appends
one JSON line per scan to `runs.jsonl`; a scan already there is skipped, so a run that stops can
be started again. `score` reads both and writes `score.json` and `precision_sample.jsonl`: thirty
confirmed findings from the after files, for grading by hand.

Recall counts a case found when a finding's line is within three lines of the fix's old-side
ranges (five of a pure insertion), and, with the judge, when the model on the same server agrees
that a finding describes the commit's bug. Nothing here writes to the repository.
"""

import argparse
import hashlib
import json
import os
import random
import re
import subprocess
import sys
from collections.abc import Callable
from pathlib import Path
from typing import Any, Final

sys.path.insert(0, str(Path(__file__).resolve().parent))
import fanout_scan as fs

SUBJECT_RE: Final[re.Pattern[str]] = re.compile(
    r"not a crash|instead of|no longer|\bfix(es|ed)?\b|\bbug\b|broken|wrong|crash|never |is refused|"
    r"fails? closed|left behind|leak|race|hang|stale|escap|unescaped|off.by.one|missing",
    re.IGNORECASE,
)
NOT_A_FIX_RE: Final[re.Pattern[str]] = re.compile(
    r"\bruff\b|\bmypy\b|clippy|code standard|format(ting)?\b|typing|lint|rename|ratchet|\bspec\b|"
    r"docs?:|README|changelog|release notes|bump|revert",
    re.IGNORECASE,
)
SKIP_PREFIXES: Final[tuple[str, ...]] = (
    "codex/",
    "ragon-engine/",
    "codex-tests/",
    "codex-patches/",
)
MAX_SOURCE_FILES: Final[int] = 3
MAX_CHANGED_LINES: Final[int] = 120
MAX_FILE_LINES: Final[int] = 1500
LINE_SLACK: Final[int] = 3
INSERT_SLACK: Final[int] = 5
DEV_SHARE: Final[int] = 12  # one case in twelve, by hash, is kept for tuning
PRECISION_SAMPLE: Final[int] = 30
HUNK_RE: Final[re.Pattern[str]] = re.compile(r"^@@ -(\d+)(?:,(\d+))? \+\d+(?:,\d+)? @@")

CLASSIFY_SYSTEM: Final[str] = (
    "You sort commits of a software project. A defect fix changes code that behaved wrongly (a crash, "
    "a wrong result, a leak, a hang, a security hole, a broken edge case) so that it behaves rightly. "
    "A new feature, a refactor, a test-only change, a docs or style change or a tuning of defaults is "
    "not a defect fix. Answer `yes` or `no`."
)
CLASSIFY_DIFF_LINES: Final[int] = 120
MIN_SUBJECT: Final[int] = 12

JUDGE_QUESTION: Final[str] = (
    "A later commit fixed a bug in this file. Its message: `{subject}`. It changed lines {ranges}.\n"
    "A reviewer reported: `L{line} {text}`.\nDoes the report describe the bug that commit fixed, not "
    "some other problem? Answer `yes` or `no`."
)

# git's diff markers: a numstat row's "-" cell is a binary file, a name-status path with
# "=>" was renamed by the commit
NUMSTAT_BINARY: Final[str] = "-"
RENAME_ARROW: Final[str] = "=>"
# the case sets `selected` picks from
SPLIT_ALL: Final[str] = "all"
SPLIT_EVAL: Final[str] = "eval"
SPLIT_DEV: Final[str] = "dev"
# the two scan methods, in scan order
METHOD_FANOUT: Final[str] = "fanout"
METHOD_BASELINE: Final[str] = "baseline"
METHODS: Final[tuple[str, ...]] = (METHOD_FANOUT, METHOD_BASELINE)
# `recall_flags`' verdict keys
FLAG_JUDGE: Final[str] = "judge"
FLAG_BOTH: Final[str] = "both"
# per-case method keys in `score`'s document
KEY_ROUND1: Final[str] = "fanout_round1"
KEY_MEDIUM: Final[str] = "fanout_medium"
KEY_SUMMARY: Final[str] = "fanout_summary"
# `main`'s command names
CMD_CASES: Final[str] = "cases"
CMD_RUN: Final[str] = "run"


def git(repo: str, *args: str) -> str:
    """Runs one git command in the repository and returns its output."""
    return subprocess.run(
        ["git", "-C", repo, *args], check=True, capture_output=True, text=True
    ).stdout


def is_source(path: str) -> bool:
    """A Python or Rust file outside the tests, the submodules and the patch folders."""
    return (
        fs.language_of(path) != ""
        and not fs.is_test_file(path)
        and not path.startswith(SKIP_PREFIXES)
    )


def old_ranges(diff: str) -> list[dict[str, int]]:
    """The old-side ranges of a `-U0` diff's hunks; a pure insertion is the line it follows, marked."""
    ranges = []
    for line in diff.splitlines():
        match = HUNK_RE.match(line)
        if match:
            start, count = (
                int(match.group(1)),
                int(match.group(2) if match.group(2) is not None else 1),
            )
            if count == 0:
                ranges.append({"start": max(1, start), "end": start + 1, "insert": 1})
            else:
                ranges.append({"start": start, "end": start + count - 1, "insert": 0})
    return ranges


def deleted_code_lines(diff: str) -> int:
    """Counts removed lines that are code: not blank, not a comment, not a docstring line."""
    count = 0
    for line in diff.splitlines():
        if line.startswith("-") and not line.startswith("---"):
            text = line[1:].strip()
            if text and not text.startswith(("#", "//", '"""', "'''", "///", "//!")):
                count += 1
    return count


def is_dev(sha: str) -> bool:
    """Whether a case is set aside for tuning, by a stable hash of its commit."""
    return int(hashlib.sha256(sha.encode()).hexdigest(), 16) % DEV_SHARE == 0


def classify_question(subject: str, diff: str) -> str:
    """The classifier's question about one commit: its message and the start of its diff."""
    shown = "\n".join(diff.splitlines()[:CLASSIFY_DIFF_LINES])
    return f"Commit message: {subject}\n\nDiff of its source files:\n{shown}\n\nIs this commit a defect fix?"


def model_classifier(client: fs.ScanClient) -> Callable[[str, str], bool]:
    """A classifier that asks the model whether a commit fixes a defect."""

    def classify(subject: str, diff: str) -> bool:
        answer = client.chat(
            [
                {"role": "system", "content": CLASSIFY_SYSTEM},
                {"role": "user", "content": classify_question(subject, diff)},
            ],
            4,
        )
        return answer.strip().lower().startswith("yes")

    return classify


def numstat_rows(numstat: str) -> tuple[list[tuple[str, int, int]], list[str]]:
    """A `--numstat` listing as (files, tests): (path, added, deleted) rows, its test paths."""
    files: list[tuple[str, int, int]] = []
    tests: list[str] = []
    for row in numstat.splitlines():
        added, deleted, path = [*row.split("\t"), "", ""][:3]
        if added == NUMSTAT_BINARY:
            continue
        files.append((path, int(added), int(deleted)))
        if fs.is_test_file(path) and fs.language_of(path):
            tests.append(path)
    return files, tests


def one_to_three_unrenamed_sources(files: list[tuple[str, int, int]]) -> bool:
    """One to three source files, none renamed: the commit's shape that makes a case scannable."""
    sources = [f for f in files if is_source(f[0])]
    return 1 <= len(sources) <= MAX_SOURCE_FILES and not any(RENAME_ARROW in f[0] for f in sources)


def name_status_renames(status: str, files: list[tuple[str, int, int]]) -> bool:
    """Whether the commit added, deleted or renamed one of its source files."""
    sources = {f[0] for f in files if is_source(f[0]) and RENAME_ARROW not in f[0]}
    return any(
        row.startswith(("A", "D", "R")) and row.split("\t")[-1] in sources
        for row in status.splitlines()
    )


def source_diff(repo: str, sha: str, sources: list[tuple[str, int, int]]) -> str:
    """The commit's diff over all of its source files, the classifier's evidence."""
    return git(repo, "diff", f"{sha}^", sha, "--", *[s[0] for s in sources])


def scannable(repo: str, sha: str) -> tuple[list[tuple[str, int, int]], list[str]] | None:
    """The commit's (files, tests) when its source files make a scannable case, else None."""
    numstat = git(repo, "show", "--numstat", "--format=", "--diff-filter=AMDR", "-M", sha)
    files, tests = numstat_rows(numstat)
    if not one_to_three_unrenamed_sources(files):
        return None
    status = git(repo, "show", "--name-status", "--format=", sha)
    if name_status_renames(status, files):
        return None
    return files, tests


def classified_fix(
    stages: dict[str, int],
    classify: Callable[[str, str], bool] | None,
    subject: str,
    repo: str,
    sha: str,
    sources: list[tuple[str, int, int]],
) -> bool:
    """Whether the classifier, when there is one, takes the commit for a defect fix (counted)."""
    if classify is None:
        return True
    if not classify(subject, source_diff(repo, sha, sources)):
        return False
    stages["classified_fix"] += 1
    return True


def case_files(repo: str, sha: str, path: str, before: str, out: Path) -> None:
    """Writes the case's primary file before the fix and after it under `out/cases/<sha>`."""
    folder = out / "cases" / sha[:12]
    folder.mkdir(exist_ok=True, parents=True)
    after = git(repo, "show", f"{sha}:{path}")
    for name, text in (("before", before), ("after", after)):
        with (folder / name).open("w", encoding="utf-8") as handle:
            handle.write(text)


def small_enough(sources: list[tuple[str, int, int]]) -> bool:
    """The commit changed at most `MAX_CHANGED_LINES` lines of its source files."""
    return sum(a + d for _, a, d in sources) <= MAX_CHANGED_LINES


def primary_path(sources: list[tuple[str, int, int]]) -> str:
    """The source file the commit changed the most: the one a case is built around."""
    return max(sources, key=lambda f: f[1] + f[2])[0]


def case_entry(
    sha: str,
    subject: str,
    path: str,
    before: str,
    diff: str,
    files: list[tuple[str, int, int]],
) -> dict[str, Any]:
    """The `cases.json` entry of one kept commit."""
    sources = [f for f in files if is_source(f[0])]
    tests = [f[0] for f in files if fs.is_test_file(f[0]) and fs.language_of(f[0])]
    return {
        "sha": sha[:12],
        "subject": subject,
        "path": path,
        "lang": fs.language_of(path),
        "lines_before": len(before.splitlines()),
        "ranges": old_ranges(diff),
        "tests": tests,
        "other_sources": [s[0] for s in sources if s[0] != path],
        "split": SPLIT_DEV if is_dev(sha) else SPLIT_EVAL,
    }


def subject_passes(subject: str, classify: Callable[[str, str], bool] | None) -> bool:
    """The history's subject filters: long enough, not a known non-fix, a fix word unless classified."""
    return (
        len(subject) >= MIN_SUBJECT
        and not NOT_A_FIX_RE.search(subject)
        and (classify is not None or bool(SUBJECT_RE.search(subject)))
    )


def mine(
    repo: str,
    rev: str,
    out: str,
    limit: int,
    classify: Callable[[str, str], bool] | None = None,
) -> dict[str, Any]:
    """Builds the case set from the history and writes `cases.json` and the case files under `out`.

    Args:
        repo: The repository.
        rev: The branch whose history is mined.
        out: The output directory.
        limit: The most cases to keep, newest first.
        classify: `(subject, diff) -> bool`, whether a commit fixes a defect; None to go by the
            subject's words alone (`SUBJECT_RE`).

    Returns:
        The written document: the cases and the count left after each filter.
    """
    log = git(repo, "log", rev, "--no-merges", "--format=%H%x09%s")
    rev_short = git(repo, "rev-parse", "--short", rev).strip()
    stages = {
        "commits": 0,
        "subject": 0,
        "source_files": 0,
        "small": 0,
        "deletes_code": 0,
        "has_test": 0,
    }
    stages.update({"size": 0, "classified_fix": 0} if classify else {"size": 0})
    cases = []
    out_dir = Path(out)
    for entry in log.splitlines():
        sha, _, subject = entry.partition("\t")
        stages["commits"] += 1
        if not subject_passes(subject, classify):
            continue
        stages["subject"] += 1
        found = scannable(repo, sha)
        if found is None:
            continue
        files, tests = found
        stages["source_files"] += 1
        sources = [f for f in files if is_source(f[0])]
        if not small_enough(sources):
            continue
        stages["small"] += 1
        path = primary_path(sources)
        diff = git(repo, "diff", "-U0", f"{sha}^", sha, "--", path)
        if deleted_code_lines(diff) == 0:
            continue
        stages["deletes_code"] += 1
        if not tests:
            continue
        stages["has_test"] += 1
        before = git(repo, "show", f"{sha}^:{path}")
        if len(before.splitlines()) > MAX_FILE_LINES:
            continue
        stages["size"] += 1
        if not classified_fix(stages, classify, subject, repo, sha, sources):
            continue
        case_files(repo, sha, path, before, out_dir)
        cases.append(case_entry(sha, subject, path, before, diff, files))
        if len(cases) >= limit:
            break
    document = {
        "rev": rev_short,
        "classifier": "model" if classify else "subject",
        "stages": stages,
        "cases": cases,
    }
    out_dir.mkdir(exist_ok=True, parents=True)
    with (out_dir / "cases.json").open("w", encoding="utf-8") as handle:
        json.dump(document, handle, indent=1)
    return document


def matches(line: int, ranges: list[dict[str, int]]) -> bool:
    """Whether a finding's line falls on the fix: within three lines of a range, five of an insertion."""
    for r in ranges:
        slack = INSERT_SLACK if r.get("insert") else LINE_SLACK
        if r["start"] - slack <= line <= r["end"] + slack:
            return True
    return False


def ranges_text(ranges: list[dict[str, int]]) -> str:
    """The fix's ranges as the judge reads them, e.g. `12-14, 30`."""
    return ", ".join(
        str(r["start"]) if r["start"] == r["end"] else f"{r['start']}-{r['end']}" for r in ranges
    )


def load_runs(path: str) -> dict[tuple[str, str, str], dict[str, Any]]:
    """The scans already in `runs.jsonl`, by (case, side, method)."""
    runs: dict[tuple[str, str, str], dict[str, Any]] = {}
    run_file = Path(path)
    if not run_file.exists():
        return runs
    for line in run_file.open(encoding="utf-8"):
        if not line.strip():
            continue
        row = json.loads(line)
        runs[(row["sha"], row["side"], row["method"])] = row
    return runs


def selected(cases: list[dict[str, Any]], split: str) -> list[dict[str, Any]]:
    """The cases of one split (`eval`, `dev` or `all`)."""
    return [c for c in cases if split == SPLIT_ALL or c["split"] == split]


def case_source(out: Path, sha: str, side: str) -> str:
    """The text of one case's scanned file (its `before` or its `after`)."""
    with (out / "cases" / sha / side).open(encoding="utf-8") as handle:
        return handle.read()


def record_scan(runs_file: Path, row: dict[str, Any]) -> None:
    """Appends one scan's row to `runs.jsonl` and prints its progress line."""
    with runs_file.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(row) + "\n")
    s = row["summary"]
    print(  # noqa: T201 - the tool's per-scan progress report
        f"{row['sha']} {row['side']:6s} {row['method']:8s} {s['lines']:5d} lines "
        f"{s['confirmed']:3d} found {s['probes_round1'] + s['probes_round2']:4d} probes "
        f"{s['seconds']:7.1f} s",
        flush=True,
    )


def scan_case(
    client: fs.Client,
    case: dict[str, Any],
    side: str,
    concurrency: int,
    out_dir: Path,
    done: dict[tuple[str, str, str], dict[str, Any]],
) -> int:
    """Scans one case's side with each method not yet in `done`, appending to `runs.jsonl`."""
    source = case_source(out_dir, case["sha"], side)
    count = 0
    for method in METHODS:
        if (case["sha"], side, method) in done:
            continue
        result = (
            fs.scan(client, case["path"], source, concurrency)
            if method == METHOD_FANOUT
            else fs.baseline(client, case["path"], source)
        )
        row = {"sha": case["sha"], "side": side, "method": method, **result}
        record_scan(out_dir / "runs.jsonl", row)
        count += 1
    return count


def run(out: str, host: str, concurrency: int, split: str, sides: tuple[str, ...]) -> int:
    """Scans every selected case not yet in `runs.jsonl`; returns how many scans it ran."""
    out_dir = Path(out)
    with (out_dir / "cases.json").open(encoding="utf-8") as handle:
        cases = selected(json.load(handle)["cases"], split)
    done = load_runs(str(out_dir / "runs.jsonl"))
    client = fs.Client(host)
    return sum(
        scan_case(client, case, side, concurrency, out_dir, done)
        for case in cases
        for side in sides
    )


def judge(
    client: fs.ScanClient, case: dict[str, Any], source: str, finding: dict[str, Any]
) -> bool:
    """Asks the model whether a finding describes the case's known bug, over the before file's prefix."""
    question = JUDGE_QUESTION.format(
        subject=case["subject"],
        ranges=ranges_text(case["ranges"]),
        line=finding["line"],
        text=finding["text"],
    )
    answer = client.chat(fs.messages(case["path"], fs.number_lines(source), question), 4)
    return answer.strip().lower().startswith("yes")


def recall_flags(
    case: dict[str, Any], findings: list[dict[str, Any]], judged: list[bool] | None
) -> dict[str, bool]:
    """Whether a case's bug was found: by line, by the judge, and by both on one finding."""
    by_line = [matches(f["line"], case["ranges"]) for f in findings]
    flags = {"line": any(by_line)}
    if judged is not None:
        flags[FLAG_JUDGE] = any(judged)
        flags[FLAG_BOTH] = any(a and b for a, b in zip(by_line, judged, strict=True))
    return flags


def rate(values: list[bool]) -> float | None:
    """The share of true values, or None of none."""
    return round(sum(values) / len(values), 3) if values else None


def mean(values: list[float]) -> float | None:
    """The mean, or None of no values."""
    return round(sum(values) / len(values), 3) if values else None


def method_stats(method: str, rows: list[dict[str, Any]]) -> dict[str, Any]:
    """One method's recall, findings-per-file and the probe and time means from its per-case rows."""
    summaries = [r[method + "_summary"] for r in rows]
    return {
        "cases": len(rows),
        "recall_line": rate([r[method]["line"] for r in rows]),
        "recall_judge": rate([r[method][FLAG_JUDGE] for r in rows if FLAG_JUDGE in r[method]]),
        "recall_both": rate([r[method][FLAG_BOTH] for r in rows if FLAG_BOTH in r[method]]),
        "findings_per_file": mean([r[method]["findings"] for r in rows]),
        "findings_per_100_lines": mean(
            [100 * r[method]["findings"] / max(1, r["lines"]) for r in rows]
        ),
        "seconds_per_file": mean([s["seconds"] for s in summaries]),
        "probes_per_file": mean([s["probes_round1"] + s["probes_round2"] for s in summaries]),
        "cache_hit_rate": mean(
            [s["cache_hit_rate"] for s in summaries if s.get("cache_hit_rate") is not None]
        ),
    }


def after_findings(
    cases: list[dict[str, Any]], runs: dict[tuple[str, str, str], dict[str, Any]]
) -> list[dict[str, Any]]:
    """Every finding of the after files of the runs, tagged with its case and its method."""
    return [
        {"sha": c["sha"], "path": c["path"], "method": m, **f}
        for c in cases
        for m in METHODS
        if (scan := runs.get((c["sha"], "after", m)))
        for f in scan["findings"]
    ]


def precision_sample(after: list[dict[str, Any]], seed: int, out_dir: Path) -> None:
    """Writes `precision_sample.jsonl`: the pool of confirmed findings, for grading by hand."""
    sample: list[dict[str, Any]] = []
    for m in METHODS:
        pool = [f for f in after if f["method"] == m]
        random.Random(seed).shuffle(pool)
        sample.extend(pool[:PRECISION_SAMPLE])
    with (out_dir / "precision_sample.jsonl").open("w", encoding="utf-8") as handle:
        for finding in sample:
            handle.write(json.dumps({**finding, "grade": ""}) + "\n")


def score(out: str, host: str | None, split: str, seed: int = 7) -> dict[str, Any]:
    """Scores the runs: recall of the known bugs, findings per file, time, probes and the cache.

    Args:
        out: The output directory with `cases.json` and `runs.jsonl`.
        host: The model server for the judge, or None to score by line only.
        split: The split scored, `eval` normally.
        seed: The seed of the precision sample.

    Returns:
        The document written to `score.json`.
    """
    out_dir = Path(out)
    with (out_dir / "cases.json").open(encoding="utf-8") as handle:
        cases = selected(json.load(handle)["cases"], split)
    runs = load_runs(str(out_dir / "runs.jsonl"))
    client = fs.Client(host) if host else None
    per_case, methods = [], {}
    for case in cases:
        source = case_source(out_dir, case["sha"], "before")
        row: dict[str, Any] = {
            "sha": case["sha"],
            "path": case["path"],
            "lines": case["lines_before"],
        }
        for method in METHODS:
            scan = runs.get((case["sha"], "before", method))
            if scan is None:
                continue
            findings = scan["findings"]
            judged = [judge(client, case, source, f) for f in findings] if client else None
            row[method] = recall_flags(case, findings, judged)
            row[method]["findings"] = len(findings)
            if method == METHOD_FANOUT:
                row[KEY_ROUND1] = recall_flags(case, scan["candidates"], None)
                serious = [f for f in findings if f.get("severity") in ("high", "medium")]
                row[KEY_MEDIUM] = recall_flags(case, serious, None)
                row[KEY_MEDIUM]["findings"] = len(serious)
            row[method + "_summary"] = scan["summary"]
        per_case.append(row)
    for method in METHODS:
        methods[method] = method_stats(method, [r for r in per_case if method in r])
    methods[KEY_ROUND1] = {
        "recall_line": rate([r[KEY_ROUND1]["line"] for r in per_case if KEY_ROUND1 in r]),
        "candidates_per_file": mean(
            [r[KEY_SUMMARY]["candidates"] for r in per_case if KEY_SUMMARY in r]
        ),
    }
    methods[KEY_MEDIUM] = {
        "recall_line": rate([r[KEY_MEDIUM]["line"] for r in per_case if KEY_MEDIUM in r]),
        "findings_per_file": mean([r[KEY_MEDIUM]["findings"] for r in per_case if KEY_MEDIUM in r]),
    }
    after = after_findings(cases, runs)
    methods["after_findings"] = {m: sum(1 for f in after if f["method"] == m) for m in METHODS}
    precision_sample(after, seed, out_dir)
    document = {"split": split, "judge": bool(client), "methods": methods, "cases": per_case}
    with (out_dir / "score.json").open("w", encoding="utf-8") as handle:
        json.dump(document, handle, indent=1)
    return document


def main(argv: list[str] | None = None) -> int:
    """Parses the command line and runs one of `cases`, `run` and `score`."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = parser.add_subparsers(dest="command", required=True)
    host = os.environ.get("DREAMFERENCE_VLLM_HOST") or fs.DEFAULT_HOST
    p_cases = sub.add_parser(CMD_CASES)
    p_cases.add_argument("--out", required=True)
    p_cases.add_argument("--repo", default=".")
    p_cases.add_argument("--rev", default="origin/main")
    p_cases.add_argument("--limit", type=int, default=110)
    p_cases.add_argument("--host", default=host)
    p_cases.add_argument(
        "--by-subject", action="store_true", help="no model: keep commits by their words"
    )
    p_run = sub.add_parser(CMD_RUN)
    p_run.add_argument("--out", required=True)
    p_run.add_argument("--host", default=host)
    p_run.add_argument("--concurrency", type=int, default=8)
    p_run.add_argument("--split", default=SPLIT_EVAL, choices=(SPLIT_EVAL, SPLIT_DEV, SPLIT_ALL))
    p_run.add_argument("--before-only", action="store_true")
    p_score = sub.add_parser("score")
    p_score.add_argument("--out", required=True)
    p_score.add_argument("--host", default=host)
    p_score.add_argument("--split", default=SPLIT_EVAL, choices=(SPLIT_EVAL, SPLIT_DEV, SPLIT_ALL))
    p_score.add_argument("--no-judge", action="store_true")
    args = parser.parse_args(argv)
    if args.command == CMD_CASES:
        classify = None if args.by_subject else model_classifier(fs.Client(args.host))
        document = mine(args.repo, args.rev, args.out, args.limit, classify)
        print(json.dumps(document["stages"]))  # noqa: T201 - the tool's stages report
        print(  # noqa: T201 - the tool's split report
            f"{sum(c['split'] == SPLIT_EVAL for c in document['cases'])} eval, "
            f"{sum(c['split'] == SPLIT_DEV for c in document['cases'])} dev"
        )
    elif args.command == CMD_RUN:
        sides = ("before",) if args.before_only else ("before", "after")
        print(  # noqa: T201 - the tool's run report
            f"{run(args.out, args.host, args.concurrency, args.split, sides)} scans"
        )
    else:
        document = score(args.out, None if args.no_judge else args.host, args.split)
        print(json.dumps(document["methods"], indent=1))  # noqa: T201 - the tool's score report
    return 0


if __name__ == "__main__":
    sys.exit(main())
