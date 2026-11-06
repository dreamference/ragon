"""The gate slice: stratified, seeded, byte-for-byte reproducible, and the committed list is its output."""

from __future__ import annotations

import importlib.util
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location(
    "swe_bench_slice", ROOT / "scripts" / "swe_bench_slice.py"
)
assert SPEC is not None and SPEC.loader is not None
slice_mod = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(slice_mod)

LIST = ROOT / "docs" / "dev" / "swe-bench" / "verified357-all.txt"
COMMITTED = ROOT / "docs" / "dev" / "swe-bench" / "gate-slice-100.txt"


def test_the_slice_is_reproducible_and_seed_dependent():
    ids = slice_mod.read_ids(LIST)
    assert len(ids) == 357
    first = slice_mod.draw(ids, 100, 1000)
    assert first == slice_mod.draw(ids, 100, 1000)
    assert len(first) == len(set(first)) == 100
    assert first != slice_mod.draw(ids, 100, 1001)


def test_the_slice_is_stratified_by_repository():
    ids = slice_mod.read_ids(LIST)
    picked = slice_mod.draw(ids, 100, 1000)
    repos = {slice_mod.repo_of(i) for i in ids}
    assert {slice_mod.repo_of(i) for i in picked} == repos
    big = max(repos, key=lambda r: sum(slice_mod.repo_of(i) == r for i in ids))
    share = sum(slice_mod.repo_of(i) == big for i in picked)
    assert abs(share - 100 * sum(slice_mod.repo_of(i) == big for i in ids) / 357) < 1.5
    assert set(picked) <= set(ids)


def test_allocation_sums_to_the_size():
    sizes = {"a": 206, "b": 5, "c": 1}
    for total in (3, 10, 100, 212):
        assert sum(slice_mod.allocate(sizes, total).values()) == total


def test_the_committed_list_is_the_default_output(tmp_path):
    out = tmp_path / "slice.txt"
    assert slice_mod.main(["--out", str(out)]) == 0
    assert out.read_bytes() == COMMITTED.read_bytes()
