"""The packages import on Windows: no POSIX-only module is needed at import time."""

import subprocess
import sys

from dreamference.config import file_lock
from dreamference.config.file_lock import FileLock

BLOCK = "import sys\nfor name in ('fcntl', 'pwd', 'grp', 'termios', 'resource'):\n    sys.modules[name] = None\n"


def test_packages_import_without_posix_modules():
    code = (
        BLOCK
        + "import dreamference.runner, dreamference.node, dreamference.cli, dreamference.night_shift"
    )
    done = subprocess.run([sys.executable, "-c", code], capture_output=True, text=True, check=False)
    assert done.returncode == 0, done.stderr[-800:]


def test_lock_without_any_locking_module_says_so(monkeypatch, tmp_path):
    monkeypatch.setattr(file_lock, "fcntl", None)
    monkeypatch.setattr(file_lock, "msvcrt", None)
    with (tmp_path / "x.lock").open("a") as handle:
        for call in (lambda: FileLock.lock(handle), lambda: FileLock.unlock(handle)):
            try:
                call()
            except RuntimeError as error:
                assert "fcntl" in str(error)
            else:
                raise AssertionError("expected RuntimeError")


def test_lock_uses_msvcrt_when_fcntl_is_absent(monkeypatch, tmp_path):
    calls = []

    class Fake:
        LK_LOCK, LK_NBLCK, LK_UNLCK = 1, 2, 0

        @staticmethod
        def locking(fd, mode, size):
            calls.append(mode)

    monkeypatch.setattr(file_lock, "fcntl", None)
    monkeypatch.setattr(file_lock, "msvcrt", Fake)
    with (tmp_path / "x.lock").open("a") as handle:
        FileLock.lock(handle)
        FileLock.lock(handle, wait=False)
        FileLock.unlock(handle)
    assert calls == [1, 2, 0]


def test_lock_on_linux_is_exclusive(tmp_path):
    path = tmp_path / "x.lock"
    with path.open("a") as first, path.open("a") as second:
        FileLock.lock(first)
        try:
            FileLock.lock(second, wait=False)
        except BlockingIOError:
            pass
        else:
            raise AssertionError("second lock should block")
        FileLock.unlock(first)
        FileLock.lock(second, wait=False)
