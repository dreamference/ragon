#!/usr/bin/env python3
"""The 1.0 development gate's slice: a seeded sample of the Verified 357, stratified by repository.

The slice is drawn from the committed list ``docs/dev/swe-bench/verified357-all.txt``. Each
repository gets a share of the slice proportional to its size (largest remainder, at least one
instance when the repository has any), and its instances are drawn with ``random.Random(seed)``
from the sorted ids. The output is sorted by id, so the same list, size and seed give the same
bytes on every machine. The result is a ``--subset`` file for ``ragon-admin swe-bench run``::

    .venv/bin/python scripts/swe_bench_slice.py --out docs/dev/swe-bench/gate-slice-100.txt
"""

from __future__ import annotations

import argparse
import random
import sys
from collections import defaultdict
from pathlib import Path
from typing import Final

REPO_ROOT: Final[Path] = Path(__file__).resolve().parent.parent
DEFAULT_LIST: Final[Path] = REPO_ROOT / "docs" / "dev" / "swe-bench" / "verified357-all.txt"
DEFAULT_SIZE: Final[int] = 100
DEFAULT_SEED: Final[int] = 1000


def read_ids(path: Path) -> list[str]:
    """Return the instance ids of a list file, comments and blank lines dropped, sorted."""
    ids = {
        line.strip()
        for line in path.read_text(encoding="utf-8").splitlines()
        if line.strip() and not line.lstrip().startswith("#")
    }
    return sorted(ids)


def repo_of(instance_id: str) -> str:
    """Return the repository part of an id: ``owner__repo`` of ``owner__repo-1234``."""
    return instance_id.rsplit("-", 1)[0]


def allocate(sizes: dict[str, int], total: int) -> dict[str, int]:
    """Split ``total`` over repositories in proportion to ``sizes`` (largest remainder)."""
    population = sum(sizes.values())
    if not 0 < total <= population:
        raise ValueError(f"slice size {total} must be between 1 and {population}")
    quota = {repo: total * n / population for repo, n in sizes.items()}
    share = {repo: min(sizes[repo], max(1, int(q))) for repo, q in quota.items()}
    order = sorted(sizes, key=lambda r: (-(quota[r] - int(quota[r])), r))
    while sum(share.values()) < total:
        for repo in order:
            if sum(share.values()) >= total:
                break
            if share[repo] < sizes[repo]:
                share[repo] += 1
    while sum(share.values()) > total:
        biggest = max((r for r in share if share[r] > 1), key=lambda r: (share[r] - quota[r], r))
        share[biggest] -= 1
    return share


def draw(ids: list[str], total: int, seed: int) -> list[str]:
    """Return ``total`` ids, stratified by repository, reproducible from ``seed``."""
    groups: dict[str, list[str]] = defaultdict(list)
    for instance_id in sorted(ids):
        groups[repo_of(instance_id)].append(instance_id)
    share = allocate({repo: len(members) for repo, members in groups.items()}, total)
    rng = random.Random(seed)
    picked: list[str] = []
    for repo in sorted(groups):
        picked.extend(rng.sample(groups[repo], share[repo]))
    return sorted(picked)


def render(picked: list[str], source: str, seed: int) -> str:
    """Return the list file text: a short header, then one id per line."""
    header = (
        f"# Gate slice: {len(picked)} instances of {source}, seed {seed}, stratified by repository.\n"
        "# Written by scripts/swe_bench_slice.py; the same list, size and seed give the same bytes.\n"
    )
    return header + "\n".join(picked) + "\n"


def main(argv: list[str] | None = None) -> int:
    """Write the slice to ``--out`` (or stdout)."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--list", type=Path, default=DEFAULT_LIST)
    parser.add_argument("--size", type=int, default=DEFAULT_SIZE)
    parser.add_argument("--seed", type=int, default=DEFAULT_SEED)
    parser.add_argument("--out", type=Path, default=None)
    args = parser.parse_args(argv)
    ids = read_ids(args.list)
    text = render(draw(ids, args.size, args.seed), args.list.name, args.seed)
    if args.out is None:
        sys.stdout.write(text)
    else:
        args.out.write_text(text, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
