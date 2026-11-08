"""A failed index build is retried, then the instance is left out and the run goes on."""

from pathlib import Path
from unittest import mock

from dreamference.swe_bench.swe_bench_runner import SweBenchRunner

RECORD = {"cached": False, "seconds": 1.0}


def _build(tmp_path: Path, results: dict[str, list], pending: list[str]):
    calls: list[str] = []
    sleeps: list[int] = []

    def ensure(row, image, arm):
        calls.append(row["instance_id"])
        return results[row["instance_id"]].pop(0)

    store = mock.Mock(directory=tmp_path)
    manifest = {
        "code_index": "universal",
        "images": {i: {"image": f"img-{i}"} for i in pending},
    }
    rows = {i: {"instance_id": i} for i in pending}
    with (
        mock.patch.object(SweBenchRunner, "sleep", staticmethod(sleeps.append)),
        mock.patch("dreamference.swe_bench.swe_bench_runner.SweBenchCodeIndex") as index,
        mock.patch("dreamference.swe_bench.swe_bench_runner.SweBenchDocker") as docker,
        mock.patch("dreamference.swe_bench.swe_bench_runner.SweBenchHarness"),
    ):
        index.ensure.side_effect = ensure
        index.ensure_runtime.return_value = "abc"
        index.container_arguments.return_value = {}
        docker.ensure_image.return_value = True
        docker.image_digest.return_value = "d"
        indexes = SweBenchRunner._build_indexes(store, manifest, pending, rows, mock.Mock(), False)
    return indexes, calls, sleeps


def test_transient_failure_is_retried_and_succeeds(tmp_path, capsys):
    pending = ["a", "b"]
    indexes, calls, sleeps = _build(
        tmp_path, {"a": [None, None, dict(RECORD)], "b": [dict(RECORD)]}, pending
    )
    assert sorted(indexes) == ["a", "b"]
    assert calls == ["a", "a", "a", "b"]
    assert sleeps == [45, 45]


def test_persistent_failure_excludes_the_instance_and_goes_on(tmp_path, capsys):
    pending = ["a", "b"]
    indexes, calls, sleeps = _build(
        tmp_path, {"a": [None, None, None], "b": [dict(RECORD)]}, pending
    )
    assert list(indexes) == ["b"]
    assert calls == ["a", "a", "a", "b"] and sleeps == [45, 45]
    out = capsys.readouterr().out
    assert out.count("a has no index after 3 tries") == 1


def test_no_retry_when_it_works_first_time(tmp_path):
    indexes, _calls, sleeps = _build(tmp_path, {"a": [dict(RECORD)]}, ["a"])
    assert list(indexes) == ["a"] and sleeps == []
