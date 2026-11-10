"""The release's asset-name gate (scripts/release_asset_check.py)."""

import importlib.util
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parent.parent


@pytest.fixture(scope="module")
def check():
    spec = importlib.util.spec_from_file_location(
        "release_asset_check", str(ROOT / "scripts" / "release_asset_check.py")
    )
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _setup(tmp_path: Path, previous: list[str], present: list[str]) -> list[str]:
    (tmp_path / "prev.txt").write_text("\n".join(previous) + "\n")
    art = tmp_path / "art"
    (art / "deb").mkdir(parents=True)
    for name in present:
        (art / name).write_text("x")
    return [
        "--version",
        "0.9.3",
        "--previous-version",
        "0.9.2",
        "--previous-assets",
        str(tmp_path / "prev.txt"),
        "--files",
        str(art),
    ]


def test_version_is_substituted_and_a_complete_set_passes(check, tmp_path, capsys):
    args = _setup(
        tmp_path,
        ["ragon-0.9.2-x86_64.tar.gz", "SHA256SUMS"],
        ["ragon-0.9.3-x86_64.tar.gz", "SHA256SUMS"],
    )
    assert check.main(args) == 0


def test_a_missing_asset_fails_and_is_named(check, tmp_path, capsys):
    args = _setup(tmp_path, ["a-0.9.2.dmg", "b-0.9.2.deb"], ["b-0.9.3.deb"])
    assert check.main(args) == 1
    assert "a-0.9.3.dmg" in capsys.readouterr().out


def test_allow_fewer_prints_which_and_passes(check, tmp_path, capsys):
    args = _setup(tmp_path, ["a-0.9.2.dmg"], [])
    assert check.main([*args, "--allow-fewer"]) == 0
    assert "a-0.9.3.dmg" in capsys.readouterr().out


def test_extra_files_count(check, tmp_path):
    args = _setup(tmp_path, ["SHA256SUMS", "SHA256SUMS.sig"], [])
    assert check.main([*args, "--extra", "SHA256SUMS", "x/SHA256SUMS.sig"]) == 0


def test_extra_assets_in_the_new_release_are_fine(check):
    assert check.missing_assets(["a-1.0"], ["a-1.1", "new"], "1.0", "1.1") == []
