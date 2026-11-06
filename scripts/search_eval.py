#!/usr/bin/env python3
"""Scores web search against a fixed set of developer queries, by class.

    search_eval.py score --results FILE [--set FILE] [--json]
    search_eval.py live  --out FILE [--set FILE] [--binary ragon-search] [--delay SECONDS] [--json]

The set is `search_eval/queries.json`: about two hundred queries in four classes
(documentation, error message, release facts, navigational), each with the hosts or URL
prefixes that count as a correct result. A bare host (`docs.python.org`) accepts that host
and its subdomains; an entry with a path (`pypi.org/project/requests`) accepts URLs under
that prefix. A query is a hit at k when one of the first k results is accepted.

`score` is offline: it reads saved results, one JSON object per line with `id` or `query`
and `results` (a list of objects with a `url`, as `ragon-search --json` prints), and never
touches the network. `live` runs `ragon-search --json` for every query, saves the lines to
`--out` so the same run can be scored again, then scores them. The tests use fixtures only.
"""

import argparse
import json
import subprocess
import sys
import time
from pathlib import Path
from typing import Any, Final
from urllib.parse import urlsplit

DEFAULT_SET: Final[Path] = Path(__file__).resolve().parent / "search_eval" / "queries.json"
CUTOFFS: Final[tuple[int, ...]] = (1, 5)
SEARCH_TIMEOUT_SECONDS: Final[int] = 90
DEFAULT_DELAY_SECONDS: Final[float] = 2.0
SCHEME_MARK: Final[str] = "//"
SCORE_COMMAND: Final[str] = "score"


def load_set(path: Path) -> list[dict[str, Any]]:
    """Reads the query set and checks that every entry is well formed.

    Args:
        path: The queries file.

    Returns:
        The query entries, each with id, class, query and accept.

    Raises:
        ValueError: An entry lacks a field, has an empty accept list or repeats an id.
    """
    queries = json.loads(path.read_text(encoding="utf-8"))["queries"]
    seen: set[str] = set()
    for entry in queries:
        for key in ("id", "class", "query", "accept"):
            if not entry.get(key):
                raise ValueError(f"query set entry {entry.get('id', '?')} lacks {key}")
        if entry["id"] in seen:
            raise ValueError(f"query set repeats id {entry['id']}")
        seen.add(entry["id"])
    return queries


def _host_and_path(url: str) -> tuple[str, str]:
    """Splits a URL into a lower-case host without `www.` and a path without a trailing slash."""
    parts = urlsplit(url if SCHEME_MARK in url else SCHEME_MARK + url)
    host = (parts.hostname or "").lower()
    if host.startswith("www."):
        host = host[4:]
    return host, parts.path.rstrip("/")


def accepts(accept: list[str], url: str) -> bool:
    """Tells whether a result URL matches one of a query's accepted hosts or prefixes.

    Args:
        accept: Hosts (`docs.python.org`) or host-and-path prefixes (`pypi.org/project/requests`).
        url: The result URL.

    Returns:
        True when the URL is on an accepted host or under an accepted prefix.
    """
    host, path = _host_and_path(url)
    for entry in accept:
        want_host, want_path = _host_and_path(entry)
        if not (host == want_host or host.endswith("." + want_host)):
            continue
        if not want_path or path == want_path or path.startswith(want_path + "/"):
            return True
    return False


def first_hit_rank(entry: dict[str, Any], urls: list[str]) -> int | None:
    """Returns the 1-based rank of the first accepted result, or None."""
    for rank, url in enumerate(urls, 1):
        if accepts(entry["accept"], url):
            return rank
    return None


def result_urls(record: dict[str, Any]) -> list[str]:
    """Reads the URLs of a saved result line, in rank order."""
    return [r["url"] for r in record.get("results") or [] if isinstance(r, dict) and r.get("url")]


def load_results(path: Path, queries: list[dict[str, Any]]) -> dict[str, dict[str, Any]]:
    """Reads saved results and keys them by query id.

    Args:
        path: A JSONL file; each line names its query by `id` or by `query` text.
        queries: The query set, used to map query text to ids.

    Returns:
        The saved records keyed by id; lines naming no known query are ignored.
    """
    by_text = {e["query"]: e["id"] for e in queries}
    ids = set(by_text.values())
    records: dict[str, dict[str, Any]] = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        if not line.strip():
            continue
        record = json.loads(line)
        key = record.get("id") if record.get("id") in ids else by_text.get(record.get("query"))
        if key:
            records[key] = record
    return records


def score(queries: list[dict[str, Any]], records: dict[str, dict[str, Any]]) -> dict[str, Any]:
    """Computes hit rates at 1 and 5, overall and by class.

    A query with no saved record, or whose search failed, counts as a miss and is listed
    under `missing` or `failed`, so a run that stopped half way cannot look good.

    Args:
        queries: The query set.
        records: Saved results by query id.

    Returns:
        A dict with `total`, `overall`, `by_class` (each: n, hit_at_1, hit_at_5, rates)
        and the lists `missing`, `failed` and `misses` (ids with no hit in the top five).
    """
    classes: dict[str, dict[str, Any]] = {}
    missing: list[str] = []
    failed: list[str] = []
    misses: list[str] = []
    for entry in queries:
        bucket = classes.setdefault(entry["class"], {"n": 0, **{f"hit_at_{k}": 0 for k in CUTOFFS}})
        bucket["n"] += 1
        record = records.get(entry["id"])
        if record is None:
            missing.append(entry["id"])
            continue
        if record.get("error"):
            failed.append(entry["id"])
            continue
        rank = first_hit_rank(entry, result_urls(record))
        for k in CUTOFFS:
            if rank is not None and rank <= k:
                bucket[f"hit_at_{k}"] += 1
        if rank is None or rank > max(CUTOFFS):
            misses.append(entry["id"])
    overall = {"n": sum(b["n"] for b in classes.values())}
    for k in CUTOFFS:
        overall[f"hit_at_{k}"] = sum(b[f"hit_at_{k}"] for b in classes.values())
    for bucket in [*classes.values(), overall]:
        for k in CUTOFFS:
            bucket[f"rate_at_{k}"] = (
                round(bucket[f"hit_at_{k}"] / bucket["n"], 4) if bucket["n"] else 0.0
            )
    return {
        "total": len(queries),
        "overall": overall,
        "by_class": classes,
        "missing": missing,
        "failed": failed,
        "misses": misses,
    }


def render(result: dict[str, Any]) -> str:
    """Formats a score as a plain table."""
    lines = [f"{'class':<16}{'n':>5}{'hit@1':>9}{'hit@5':>9}"]
    rows = [*sorted(result["by_class"].items()), ("all", result["overall"])]
    for name, b in rows:
        lines.append(f"{name:<16}{b['n']:>5}{b['rate_at_1']:>9.1%}{b['rate_at_5']:>9.1%}")
    lines.append(
        f"missing {len(result['missing'])}, failed {len(result['failed'])}, no hit in top 5: {len(result['misses'])}"
    )
    return "\n".join(lines)


def run_search(binary: str, query: str) -> dict[str, Any]:
    """Runs one `ragon-search --json` and returns its parsed output, or an error record."""
    try:
        done = subprocess.run(
            [binary, "--json", "-n", str(max(CUTOFFS)), query],
            capture_output=True,
            text=True,
            timeout=SEARCH_TIMEOUT_SECONDS,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        return {"error": type(exc).__name__, "results": []}
    if done.returncode != 0:
        return {"error": f"exit {done.returncode}: {done.stderr.strip()[:200]}", "results": []}
    try:
        data = json.loads(done.stdout)
    except ValueError:
        return {"error": "output is not JSON", "results": []}
    return {
        "results": [
            {"url": r.get("url"), "engine": r.get("engine")} for r in data.get("results", [])
        ]
    }


def main(argv: list[str] | None = None) -> int:
    """Entry point.

    Args:
        argv: Arguments; the process arguments when None.

    Returns:
        The exit status.
    """
    parser = argparse.ArgumentParser(description="Score web search against the fixed query set.")
    sub = parser.add_subparsers(dest="command", required=True)
    for name in (SCORE_COMMAND, "live"):
        p = sub.add_parser(name)
        p.add_argument("--set", type=Path, default=DEFAULT_SET)
        p.add_argument("--json", action="store_true", help="print the score as JSON")
    sub.choices[SCORE_COMMAND].add_argument("--results", type=Path, required=True)
    live = sub.choices["live"]
    live.add_argument("--out", type=Path, required=True, help="where to save the results (JSONL)")
    live.add_argument("--binary", default="ragon-search")
    live.add_argument("--delay", type=float, default=DEFAULT_DELAY_SECONDS)
    args = parser.parse_args(argv)

    queries = load_set(args.set)
    if args.command == SCORE_COMMAND:
        records = load_results(args.results, queries)
    else:
        with args.out.open("w", encoding="utf-8") as handle:
            for entry in queries:
                record = {
                    "id": entry["id"],
                    "query": entry["query"],
                    **run_search(args.binary, entry["query"]),
                }
                handle.write(json.dumps(record) + "\n")
                handle.flush()
                time.sleep(args.delay)
        records = load_results(args.out, queries)
    result = score(queries, records)
    sys.stdout.write((json.dumps(result, indent=1) if args.json else render(result)) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
