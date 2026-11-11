"""The file sink (LOGGING §3.2, §6.1): `<component>.log` with rotation that two processes can share."""

import gzip
import os
import time
from collections.abc import Callable
from pathlib import Path
from typing import Final

try:  # POSIX only; the Windows rules of §6.1 are a later phase
    import fcntl
except ImportError:  # pragma: no cover
    fcntl = None  # type: ignore[assignment]

MAX_BYTES: Final[int] = 10 * 1024 * 1024
KEEP_FILES: Final[int] = 5
MAX_AGE_S: Final[int] = 14 * 24 * 3600
REOPEN_CHECK_S: Final[float] = 1.0


class LogFileSink:
    """Appends whole lines to one component's file and rotates it at 10 MiB.

    Each process opens with `O_APPEND`; the rotating process holds an advisory lock on
    `<component>.lock`; a process that did not rotate notices the new inode within a second and
    reopens. `.1.log` stays plain until the next rotation, which compresses it.
    """

    def __init__(
        self,
        directory: Path,
        component: str,
        max_bytes: int = MAX_BYTES,
        clock: Callable[[], float] = time.time,
    ) -> None:
        """Set the sink up; the file is opened on the first write.

        Args:
            directory: The `logs/` directory.
            component: The component name.
            max_bytes: The size at which the file rotates.
            clock: Wall clock, for the once-a-second inode check and the age rule.
        """
        self.directory = Path(directory)
        self.component = component
        self.max_bytes = max_bytes
        self.clock = clock
        self.path = self.directory / f"{component}.log"
        self._fd: int | None = None
        self._identity: tuple[int, int] | None = None
        self._checked = 0.0

    def _open(self) -> None:
        self.close()
        self.directory.mkdir(parents=True, exist_ok=True, mode=0o700)
        fd = os.open(
            self.path, os.O_WRONLY | os.O_APPEND | os.O_CREAT | getattr(os, "O_CLOEXEC", 0), 0o600
        )
        stat = os.fstat(fd)
        self._fd, self._identity = fd, (stat.st_dev, stat.st_ino)
        self._checked = self.clock()

    def close(self) -> None:
        """Close the open file."""
        if self._fd is not None:
            try:
                os.close(self._fd)
            finally:
                self._fd = None

    def _current(self) -> bool:
        try:
            stat = Path(self.path).stat()
        except OSError:
            return False
        return (stat.st_dev, stat.st_ino) == self._identity

    def write(self, line: str) -> None:
        """Write one line with one `write` call; raises OSError when the disk refuses.

        Args:
            line: The line without its newline.
        """
        now = self.clock()
        if self._fd is None or (now - self._checked >= REOPEN_CHECK_S and not self._current()):
            self._open()
        self._checked = now if self._fd is not None else self._checked
        assert self._fd is not None
        os.write(self._fd, (line + "\n").encode("utf-8", "replace"))
        if os.fstat(self._fd).st_size >= self.max_bytes:
            self.rotate()

    def _own(self, suffix: str) -> Path:
        return self.directory / f"{self.component}.{suffix}"

    def rotate(self) -> bool:
        """Rotate under the lock; return whether this process did it."""
        lock_fd = os.open(self.directory / f"{self.component}.lock", os.O_RDWR | os.O_CREAT, 0o600)
        try:
            if fcntl is not None:
                try:
                    fcntl.flock(lock_fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
                except OSError:
                    self._open()
                    return False
            try:
                if self.path.stat().st_size < self.max_bytes:
                    self._open()  # another process rotated first
                    return False
            except OSError:
                self._open()
                return False
            self._shift()
            self._open()
            return True
        finally:
            os.close(lock_fd)

    def _shift(self) -> None:
        last = KEEP_FILES - 1
        self._own(f"{last}.log.gz").unlink(missing_ok=True)
        for number in range(last - 1, 1, -1):
            source = self._own(f"{number}.log.gz")
            if source.exists():
                source.rename(self._own(f"{number + 1}.log.gz"))
        plain = self._own("1.log")
        if plain.exists():
            target = self._own("2.log.gz")
            temporary = self._own("2.log.gz.tmp")
            with Path(plain).open("rb") as source_file, gzip.open(temporary, "wb") as out:
                while chunk := source_file.read(1 << 20):
                    out.write(chunk)
            temporary.rename(target)
            plain.unlink()
        self.path.rename(plain)
        cutoff = self.clock() - MAX_AGE_S
        for old in self.directory.glob(f"{self.component}.[0-9]*"):
            try:
                if old.stat().st_mtime < cutoff:
                    old.unlink()
            except OSError:
                continue
