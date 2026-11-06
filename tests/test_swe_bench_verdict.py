"""The 1.0 gate's verdict script: pass, fail, unions of reports and mismatched sets."""

import importlib.util
import json
from pathlib import Path

import pytest

SCRIPT = Path(__file__).resolve().parent.parent / "scripts" / "swe_bench_verdict.py"
spec = importlib.util.spec_from_file_location("swe_bench_verdict", SCRIPT)
verdict = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verdict)

IDS = [f"repo__r-{n}" for n in range(100)]


def write(path: Path, resolved: set[str], ids=IDS) -> Path:
    """A grading record over `ids`."""
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps({"results": {i: {"resolved": i in resolved} for i in ids}}))
    return path


def slice_file(tmp_path: Path, ids=IDS) -> Path:
    path = tmp_path / "slice.txt"
    path.write_text("# a slice\n" + "\n".join(ids) + "\n")
    return path


def run(tmp_path, base, cand, *extra):
    return verdict.main(
        [
            "--slice", str(slice_file(tmp_path)),
            "--baseline", str(base),
            "--candidate", str(cand),
            *extra,
        ]
    )  # fmt: skip


def test_equal_runs_pass(tmp_path, capsys):
    resolved = set(IDS[:50])
    base = write(tmp_path / "b.json", resolved)
    cand = write(tmp_path / "c.json", resolved)
    assert run(tmp_path, base, cand) == verdict.PASS
    assert capsys.readouterr().out.startswith("PASS")


def test_clearly_worse_candidate_fails(tmp_path, capsys):
    base = write(tmp_path / "b.json", set(IDS[:60]))
    cand = write(tmp_path / "c.json", set(IDS[:40]))
    assert run(tmp_path, base, cand) == verdict.FAIL
    assert capsys.readouterr().out.startswith("FAIL")


def test_small_loss_inside_the_margin_with_noise_still_decided_by_the_statistic(tmp_path):
    counts = {
        "n": 100, "both": 50, "baseline_only": 5, "candidate_only": 3, "neither": 42,
        "baseline_resolved": 55, "candidate_resolved": 53, "outside_slice": 0,
    }  # fmt: skip
    result = verdict.verdict(counts, margin=3, alpha=0.05)
    assert result["difference"] == pytest.approx(-0.02)
    assert not result["passed"]  # -2 against -3 is not significantly above the margin
    better = verdict.verdict({**counts, "baseline_only": 0, "candidate_only": 10}, 3, 0.05)
    assert better["passed"] and better["p"] < 0.05


def test_no_discordant_pair_is_a_pass_and_a_full_margin_loss_a_fail():
    base = {"n": 100, "both": 10, "baseline_only": 0, "candidate_only": 0, "neither": 90}
    assert verdict.verdict(base, 3, 0.05)["passed"]
    lost = {**base, "both": 0, "baseline_only": 3, "neither": 97}
    assert not verdict.verdict(lost, 3, 0.05)["passed"]


def test_a_runs_several_reports_are_unioned_and_the_latest_wins(tmp_path):
    base = write(tmp_path / "b.json", set(IDS[:50]))
    first = write(tmp_path / "c" / "eval" / "1" / "grading.json", set(IDS[:50]), IDS[:60])
    # shard two covers the rest; a regrade of the first ten flips them to unresolved
    write(tmp_path / "c" / "eval" / "2" / "grading.json", set(), IDS[:10])
    write(tmp_path / "c" / "eval2" / "grading.json", set(IDS[:50]), IDS[60:])
    assert first.exists()
    merged = verdict.load_run([tmp_path / "c"])
    assert len(merged) == 100
    assert not any(merged[i] for i in IDS[:10]) and all(merged[i] for i in IDS[10:50])
    assert run(tmp_path, base, tmp_path / "c") == verdict.FAIL


def test_resolved_ids_summaries_are_read(tmp_path):
    path = tmp_path / "r.json"
    path.write_text(json.dumps({"instances": IDS, "resolved_ids": IDS[:30]}))
    results = verdict.report_results(path)
    assert sum(results.values()) == 30 and len(results) == 100


def test_mismatched_sets_are_an_error_naming_the_missing(tmp_path, capsys):
    base = write(tmp_path / "b.json", set(IDS[:50]))
    cand = write(tmp_path / "c.json", set(IDS[:50]), IDS[:90])
    assert run(tmp_path, base, cand) == verdict.ERROR
    assert "candidate lacks 10" in capsys.readouterr().err


def test_extra_instances_outside_the_slice_are_ignored_and_counted(tmp_path):
    extra = [*IDS, "other__o-1"]
    base = write(tmp_path / "b.json", set(IDS[:50]), extra)
    cand = write(tmp_path / "c.json", set(IDS[:50]), IDS)
    counts = verdict.paired(
        verdict.read_slice(slice_file(tmp_path)), verdict.load_run([base]), verdict.load_run([cand])
    )
    assert counts["outside_slice"] == 1 and counts["n"] == 100


def test_unreadable_and_empty_inputs_are_errors(tmp_path):
    (tmp_path / "bad.json").write_text("{nope")
    ok = write(tmp_path / "ok.json", set())
    assert run(tmp_path, tmp_path / "bad.json", ok) == verdict.ERROR
    assert run(tmp_path, tmp_path / "missing.json", ok) == verdict.ERROR
    (tmp_path / "empty").mkdir()
    assert run(tmp_path, tmp_path / "empty", ok) == verdict.ERROR
