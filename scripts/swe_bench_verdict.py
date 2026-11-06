#!/usr/bin/env python3
r"""The 1.0 development gate's verdict: is the candidate run not worse than the baseline run?

The rule (release scope D1, written before each run): on the slice, paired per instance,
the candidate is **not worse** than the baseline when a one-sided test rejects "the candidate
resolves at least ``margin`` instances per 100 fewer" at ``alpha`` (default: margin 3 per 100,
alpha 0.05). The statistic is the paired normal approximation on the per-instance difference
``d = candidate - baseline`` (each 0 or 1, so ``d`` is -1, 0 or 1): ``z = (mean(d) + m) / se``
with ``m = margin / 100`` and ``se`` from the sample variance of ``d``; ``p = 1 - Phi(z)``.
With no discordant pair ``se`` is 0 and the verdict is a pass when ``m > 0``.

A run is one or several reports (a directory is every ``grading.json`` under it, a file is
one report); an instance's result is the one in the **latest** report that holds it (reports
are read in the order given, files inside a directory in path order), so a regrade or a shard
overrides an earlier report. A report is a grading record (``{"results": {id: {"resolved":
bool}}}``) or a report summary (``{"resolved_ids": [...]}`` with optional ``instances``).

Every slice instance must be present in both runs: anything missing (not graded) is an error,
never a silent failure, and the exit code says so. Instances outside the slice are ignored and
counted. Exit codes: 0 pass, 1 fail, 2 mismatch or unreadable input. Usage::

    .venv/bin/python scripts/swe_bench_verdict.py --slice docs/dev/swe-bench/<list>.txt \\
        --baseline runs/base-a runs/base-b --candidate runs/cand [--margin 3] [--alpha 0.05]
"""

from __future__ import annotations

import argparse
import json
import math
import sys
from pathlib import Path
from typing import Any, Final

DEFAULT_MARGIN: Final[float] = 3.0
DEFAULT_ALPHA: Final[float] = 0.05
PER: Final[int] = 100
PASS: Final[int] = 0
FAIL: Final[int] = 1
ERROR: Final[int] = 2


class VerdictError(Exception):
    """Input that cannot be judged: unreadable, empty, or the sets do not match."""


def read_slice(path: Path) -> list[str]:
    """The slice's instance ids, in file order, without comments, blanks or repeats."""
    ids: list[str] = []
    for line in path.read_text().splitlines():
        line = line.split("#", 1)[0].strip()
        if line and line not in ids:
            ids.append(line)
    if not ids:
        raise VerdictError(f"the slice {path} lists no instance")
    return ids


def report_files(source: Path) -> list[Path]:
    """The report files of one source: the file itself, or every grading.json under a directory."""
    if source.is_dir():
        files = sorted(source.rglob("grading.json"))
        if not files:
            raise VerdictError(f"no grading.json under {source}")
        return files
    if not source.is_file():
        raise VerdictError(f"{source} does not exist")
    return [source]


def report_results(path: Path) -> dict[str, bool]:
    """One report's per-instance verdicts (`True` resolved)."""
    try:
        data: Any = json.loads(path.read_text())
    except (OSError, ValueError) as error:
        raise VerdictError(f"{path} is not readable JSON: {error}") from error
    if not isinstance(data, dict):
        raise VerdictError(f"{path} is not a JSON object")
    if isinstance(data.get("results"), dict):
        return {str(i): bool((r or {}).get("resolved")) for i, r in data["results"].items()}
    resolved = data.get("resolved_ids", data.get("resolved"))
    if isinstance(resolved, list):
        graded = data.get("instances")
        everyone = [str(i) for i in graded] if isinstance(graded, list) else []
        out = dict.fromkeys(everyone, False)
        out.update({str(i): True for i in resolved})
        return out
    raise VerdictError(f"{path} holds neither `results` nor `resolved_ids`")


def load_run(sources: list[Path]) -> dict[str, bool]:
    """A run's union of reports: the latest report holding an instance decides it."""
    merged: dict[str, bool] = {}
    for source in sources:
        for path in report_files(source):
            merged.update(report_results(path))
    return merged


def paired(
    slice_ids: list[str], baseline: dict[str, bool], candidate: dict[str, bool]
) -> dict[str, Any]:
    """The paired counts over the slice; raises when either run lacks a slice instance."""
    missing = {
        "baseline": [i for i in slice_ids if i not in baseline],
        "candidate": [i for i in slice_ids if i not in candidate],
    }
    if missing["baseline"] or missing["candidate"]:
        parts = [
            f"{who} lacks {len(ids)}: {', '.join(ids[:5])}" for who, ids in missing.items() if ids
        ]
        raise VerdictError("the runs do not cover the slice (" + "; ".join(parts) + ")")
    both = sum(baseline[i] and candidate[i] for i in slice_ids)
    only_base = sum(baseline[i] and not candidate[i] for i in slice_ids)
    only_cand = sum(candidate[i] and not baseline[i] for i in slice_ids)
    return {
        "n": len(slice_ids),
        "both": both,
        "baseline_only": only_base,
        "candidate_only": only_cand,
        "neither": len(slice_ids) - both - only_base - only_cand,
        "baseline_resolved": both + only_base,
        "candidate_resolved": both + only_cand,
        "outside_slice": len((set(baseline) | set(candidate)) - set(slice_ids)),
    }


def verdict(counts: dict[str, Any], margin: float, alpha: float) -> dict[str, Any]:
    """The one-sided paired non-inferiority test on the counts."""
    n = counts["n"]
    b, c = counts["baseline_only"], counts["candidate_only"]
    mean = (c - b) / n
    m = margin / PER
    variance = ((b + c) - (c - b) ** 2 / n) / (n - 1) if n > 1 else 0.0
    se = math.sqrt(max(variance, 0.0) / n)
    if se == 0.0:
        z = math.inf if mean + m > 0 else -math.inf
        p = 0.0 if z > 0 else 1.0
    else:
        z = (mean + m) / se
        p = 0.5 * math.erfc(z / math.sqrt(2))
    return {
        **counts,
        "difference": mean,
        "margin": m,
        "alpha": alpha,
        "z": z,
        "p": p,
        "passed": p < alpha,
    }


def render(result: dict[str, Any]) -> str:
    """The verdict as text, the statistic first."""
    word = "PASS" if result["passed"] else "FAIL"
    return "\n".join(
        [
            f"{word}: paired one-sided p = {result['p']:.4f} "
            f"({'<' if result['passed'] else '>='} alpha {result['alpha']}), "
            f"z = {result['z']:.3f}, difference {result['difference'] * PER:+.1f} per {PER} "
            f"against a margin of -{result['margin'] * PER:g} per {PER}",
            f"slice {result['n']}: baseline {result['baseline_resolved']}, "
            f"candidate {result['candidate_resolved']}; both {result['both']}, "
            f"baseline only {result['baseline_only']}, candidate only {result['candidate_only']}, "
            f"neither {result['neither']}; {result['outside_slice']} outside the slice ignored",
        ]
    )


def main(argv: list[str] | None = None) -> int:
    """Judge two runs on a slice; print the verdict."""
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--slice", required=True, type=Path, help="the slice's instance-id list")
    parser.add_argument(
        "--baseline", required=True, nargs="+", type=Path, help="report files or run directories"
    )
    parser.add_argument(
        "--candidate", required=True, nargs="+", type=Path, help="report files or run directories"
    )
    parser.add_argument("--margin", type=float, default=DEFAULT_MARGIN, help="instances per 100")
    parser.add_argument("--alpha", type=float, default=DEFAULT_ALPHA)
    parser.add_argument("--json", action="store_true", help="print the verdict as JSON")
    args = parser.parse_args(argv)
    try:
        slice_ids = read_slice(args.slice)
        counts = paired(slice_ids, load_run(args.baseline), load_run(args.candidate))
    except VerdictError as error:
        print(f"❌ {error}", file=sys.stderr)  # noqa: T201
        return ERROR
    result = verdict(counts, args.margin, args.alpha)
    if args.json:
        print(json.dumps(result, indent=2, default=str))  # noqa: T201
    else:
        print(render(result))  # noqa: T201
    return PASS if result["passed"] else FAIL


if __name__ == "__main__":
    raise SystemExit(main())
