"""A pytest run from a folder without `tests/conftest.py` still never sees the real home."""

import os
import subprocess
import sys
import textwrap
from pathlib import Path

CHECKOUT = Path(__file__).resolve().parent.parent


def test_a_suite_outside_tests_runs_in_a_scratch_home(tmp_path):
    probe_dir = CHECKOUT / "scripts" / f"_home_probe_{os.getpid()}"
    out = tmp_path / "home.txt"
    probe_dir.mkdir()
    try:
        (probe_dir / "test_probe.py").write_text(
            textwrap.dedent(
                f"""
                from pathlib import Path

                def test_home():
                    Path({str(out)!r}).write_text(str(Path.home()))
                """
            )
        )
        env = dict(os.environ, HOME=str(tmp_path / "real"))
        done = subprocess.run(
            [sys.executable, "-m", "pytest", "-q", "-p", "no:cacheprovider", str(probe_dir)],
            cwd=CHECKOUT,
            env=env,
            capture_output=True,
            text=True,
        )
    finally:
        for item in probe_dir.iterdir():
            if item.is_file():
                item.unlink()
            else:
                import shutil

                shutil.rmtree(item, ignore_errors=True)
        probe_dir.rmdir()
    assert done.returncode == 0, done.stdout[-800:]
    seen = out.read_text()
    assert seen != str(tmp_path / "real") and "root-home" in seen
