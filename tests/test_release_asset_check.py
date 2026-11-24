"""The release's asset-name gate (scripts/release_asset_check.py)."""

import importlib.util
import subprocess
from pathlib import Path

import pytest
import yaml

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


def test_rc_names_are_accepted_in_the_pep440_and_deb_spellings(check):
    # The builders write a pre-release as 0.9.9rc1 in the Python files and 0.9.9~rc.1 in the deb.
    previous = [
        "dreamference-0.9.8.1-py3-none-any.whl",
        "dreamference-0.9.8.1.tar.gz",
        "ragon_0.9.8.1_arm64.deb",
    ]
    new = [
        "dreamference-0.9.9rc1-py3-none-any.whl",
        "dreamference-0.9.9rc1.tar.gz",
        "ragon_0.9.9~rc.1_arm64.deb",
    ]
    assert check.missing_assets(previous, new, "0.9.8.1", "0.9.9-rc.1") == []


def test_rc_names_in_the_raw_spelling_are_still_accepted(check):
    previous = ["dreamference-0.9.8.1.tar.gz", "ragon_0.9.8.1_arm64.deb"]
    new = ["dreamference-0.9.9-rc.1.tar.gz", "ragon_0.9.9-rc.1_arm64.deb"]
    assert check.missing_assets(previous, new, "0.9.8.1", "0.9.9-rc.1") == []


def test_a_truly_missing_rc_asset_is_still_reported(check):
    previous = ["dreamference-0.9.8.1-py3-none-any.whl", "ragon_0.9.8.1_arm64.deb", "a-0.9.8.1.dmg"]
    new = ["dreamference-0.9.9rc1-py3-none-any.whl", "ragon_0.9.9~rc.1_arm64.deb"]
    assert check.missing_assets(previous, new, "0.9.8.1", "0.9.9-rc.1") == ["a-0.9.9-rc.1.dmg"]


def test_a_deb_is_not_accepted_under_the_pep440_spelling(check):
    # The spellings are per file kind: a .deb named 0.9.9rc1 is not the deb the maker writes.
    previous = ["ragon_0.9.8.1_arm64.deb"]
    new = ["ragon_0.9.9rc1_arm64.deb"]
    assert check.missing_assets(previous, new, "0.9.8.1", "0.9.9-rc.1") == [
        "ragon_0.9.9-rc.1_arm64.deb"
    ]


def test_a_full_release_keeps_its_single_spelling(check):
    previous = ["dreamference-0.9.8.1-py3-none-any.whl", "ragon_0.9.8.1_arm64.deb"]
    assert (
        check.missing_assets(
            previous,
            ["dreamference-0.9.9-py3-none-any.whl", "ragon_0.9.9_arm64.deb"],
            "0.9.8.1",
            "0.9.9",
        )
        == []
    )
    assert check.missing_assets(
        previous, ["dreamference-0.9.9rc1-py3-none-any.whl"], "0.9.8.1", "0.9.9"
    ) == [
        "dreamference-0.9.9-py3-none-any.whl",
        "ragon_0.9.9_arm64.deb",
    ]


def test_a_pre_release_deb_is_accepted_under_the_name_the_host_publishes(check):
    # The deb is built as 0.9.9~rc.1; the release job renames it to 0.9.9.rc.1 before the check, and
    # GitHub publishes it under that name.
    previous = ["ragon_0.9.8.1_arm64.deb"]
    new = ["ragon_0.9.9.rc.1_arm64.deb"]
    assert check.missing_assets(previous, new, "0.9.8.1", "0.9.9-rc.1") == []


def test_a_pre_release_deb_from_a_previous_pre_release_is_accepted_when_published(check):
    # rc.2 compares with rc.1 as GitHub lists it (dotted), not as the maker wrote it.
    previous = ["ragon_0.9.9.rc.1_arm64.deb"]
    new = ["ragon_0.9.9.rc.2_arm64.deb"]
    assert check.missing_assets(previous, new, "0.9.9-rc.1", "0.9.9-rc.2") == []


def test_a_missing_published_deb_is_still_reported(check):
    previous = ["ragon_0.9.8.1_arm64.deb", "a-0.9.8.1.dmg"]
    new = ["ragon_0.9.9.rc.1_arm64.deb"]
    assert check.missing_assets(previous, new, "0.9.8.1", "0.9.9-rc.1") == ["a-0.9.9-rc.1.dmg"]


def test_the_rename_step_names_a_tilde_deb_as_the_host_publishes_it(tmp_path):
    # Runs the release job's "Name the files as the host publishes them" step as GitHub runs it.
    step = next(
        s
        for s in yaml.safe_load((ROOT / ".github" / "workflows" / "release.yml").read_text())[
            "jobs"
        ]["release"]["steps"]
        if s.get("name") == "Name the files as the host publishes them"
    )
    deb_dir = tmp_path / "artifacts" / "desktop-bundles"
    deb_dir.mkdir(parents=True)
    (deb_dir / "ragon_0.9.9~rc.1_arm64.deb").write_bytes(b"deb")
    (tmp_path / "artifacts" / "wheel.whl").write_bytes(b"whl")
    subprocess.run(
        ["bash", "-eo", "pipefail", "-c", step["run"]],
        cwd=tmp_path,
        check=True,
        capture_output=True,
    )
    names = sorted(p.name for p in (tmp_path / "artifacts").rglob("*") if p.is_file())
    assert names == ["ragon_0.9.9.rc.1_arm64.deb", "wheel.whl"]
