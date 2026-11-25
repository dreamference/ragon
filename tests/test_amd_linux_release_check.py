"""scripts/amd_linux_release_check.sh against a fake ssh, a fake docker and a fake sysfs tree.

The script is the release operator's check on the AMD Strix Halo box. Here ssh runs the remote
program on this machine, docker is a stub that answers like the ragon-serve-hip image, and /dev
and /sys are a small tree. Nothing touches a GPU, Docker or the network.
"""

import os
import shutil
import stat
import subprocess
from pathlib import Path

import pytest

SCRIPT = Path(__file__).resolve().parents[1] / "scripts" / "amd_linux_release_check.sh"
EXPECTED = (SCRIPT.parent / "amd_linux_release_check.ids").read_text().strip()
DIGEST = "sha256:" + "ab" * 32

FAKE_SSH = '#!/bin/bash\nshift\nexec bash -c "$1"\n'

# docker pull ok; docker info ok; docker run ... ctest -> a summary; docker run ... ragon-run -> ids.
FAKE_DOCKER = """#!/bin/bash
case "$1" in
  pull) echo "$2"; exit 0 ;;
  info) exit 0 ;;
  run)
    case "$*" in
      *" ctest "*) echo "100% tests passed, 0 tests failed out of 23"; exit "${FAKE_CTEST_RC:-0}" ;;
      *ragon-run*) echo "drafter head: sparse" >&2; cat "$FAKE_IDS"; exit "${FAKE_SMOKE_RC:-0}" ;;
    esac ;;
esac
exit 3
"""


def _exe(path: Path, text: str) -> None:
    path.write_text(text)
    path.chmod(path.stat().st_mode | stat.S_IXUSR)


@pytest.fixture
def box(tmp_path):
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    _exe(bin_dir / "ssh", FAKE_SSH)
    _exe(bin_dir / "docker", FAKE_DOCKER)
    fs = tmp_path / "fs"
    (fs / "dev" / "dri").mkdir(parents=True)
    (fs / "dev" / "kfd").write_text("")
    node = fs / "sys" / "class" / "kfd" / "kfd" / "topology" / "nodes" / "1"
    node.mkdir(parents=True)
    (node / "properties").write_text("gfx_target_version 110501\n")
    card = fs / "sys" / "class" / "drm" / "card0" / "device"
    card.mkdir(parents=True)
    (card / "mem_info_vram_total").write_text("68719476736\n")
    (card / "mem_info_gtt_total").write_text("33519071232\n")
    ids = tmp_path / "ids"
    ids.write_text(EXPECTED + "\n")
    env = {
        **os.environ,
        "PATH": f"{bin_dir}:{os.environ['PATH']}",
        "AMD_LINUX_FS": str(fs),
        "AMD_LINUX_GROUPS": "users video render docker",
        "FAKE_IDS": str(ids),
    }
    return env, ids, tmp_path


def _run(env, tmp_path, *extra):
    args = [
        "bash",
        str(SCRIPT),
        "--host",
        "box",
        "--workdir",
        str(tmp_path),
        "--image",
        f"ghcr.io/x/ragon-serve-hip@{DIGEST}",
        "--gpu-lock",
        str(tmp_path / "gpu.lock"),
        *extra,
    ]
    return subprocess.run(args, env=env, capture_output=True, text=True, timeout=60, check=False)


def test_passes_and_prints_one_line(box):
    env, _, tmp = box
    out = _run(env, tmp)
    assert out.returncode == 0, out.stdout + out.stderr
    lines = out.stdout.strip().splitlines()
    assert len(lines) == 1
    assert lines[0].startswith("AMD_LINUX_RELEASE_CHECK PASS ")
    assert "ctest=ok(passed 100% / 23)" in lines[0] or "ctest=ok(" in lines[0]
    assert "smoke=ok(64 ids)" in lines[0] and "quick=ok(" in lines[0]


def test_wrong_id_fails_non_zero(box):
    env, ids, tmp = box
    parts = EXPECTED.split(",")
    parts[17] = str(int(parts[17]) + 1)
    ids.write_text(",".join(parts) + "\n")
    out = _run(env, tmp)
    assert out.returncode != 0
    assert (
        "AMD_LINUX_RELEASE_CHECK FAIL" in out.stdout
        and "smoke=FAIL(ids differ at 18)" in out.stdout
    )


def test_ctest_failure_fails(box):
    env, _, tmp = box
    out = _run({**env, "FAKE_CTEST_RC": "8"}, tmp)
    assert out.returncode != 0 and "ctest=FAIL(" in out.stdout


def test_missing_gpu_device_fails_the_quick_check(box):
    env, _, tmp = box
    (tmp / "fs" / "dev" / "kfd").unlink()
    out = _run(env, tmp)
    assert out.returncode != 0 and "quick=FAIL(kfd)" in out.stdout


def test_held_gpu_lock_fails_without_running(box):
    env, _, tmp = box
    if shutil.which("flock") is None:
        pytest.skip("flock missing")
    holder = subprocess.Popen(["flock", str(tmp / "gpu.lock"), "sleep", "30"])
    try:
        out = subprocess.run(
            [
                "bash",
                str(SCRIPT),
                "--host",
                "box",
                "--workdir",
                str(tmp),
                "--image",
                f"x@{DIGEST}",
                "--gpu-lock",
                str(tmp / "gpu.lock"),
            ],
            env={**env, "AMD_LINUX_LOCK_WAIT": "1"},
            capture_output=True,
            text=True,
            timeout=60,
            check=False,
        )
    finally:
        holder.kill()
        holder.wait()
    assert out.returncode != 0 and "the GPU lock is held" in out.stdout


def test_placeholder_digest_in_the_registry_is_refused(box):
    env, _, tmp = box
    out = subprocess.run(
        ["bash", str(SCRIPT), "--host", "box", "--workdir", str(tmp)],
        env=env,
        capture_output=True,
        text=True,
        timeout=60,
        check=False,
    )
    assert out.returncode != 0
    assert "placeholder digest" in out.stdout or "no ragon-serve-hip image pinned" in out.stdout
