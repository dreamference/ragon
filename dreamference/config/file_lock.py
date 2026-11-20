"""Advisory whole-file locks that import on every platform: `fcntl` on POSIX, `msvcrt` on Windows."""

from typing import IO, Any

try:  # POSIX only
    import fcntl
except ImportError:  # Windows
    fcntl = None  # type: ignore[assignment]

try:  # Windows only
    import msvcrt
except ImportError:  # POSIX
    msvcrt = None  # type: ignore[assignment]


class FileLock:
    """Locks and unlocks an open file; imported by code that must load on Windows too.

    A node still runs only on Linux, but the client build imports every package, so the
    module-level `import fcntl` of the callers moved here. On a platform with neither module
    the calls raise a `RuntimeError` that says so.
    """

    @classmethod
    def lock(cls, handle: IO[Any], wait: bool = True) -> None:
        """Takes an exclusive lock on the file.

        Args:
            handle: An open file.
            wait: Block until the lock is free; when False raise `BlockingIOError` if it is held.

        Raises:
            BlockingIOError: `wait` is False and another process holds the lock.
            RuntimeError: The platform has neither `fcntl` nor `msvcrt`.
        """
        if fcntl is not None:
            fcntl.flock(handle, fcntl.LOCK_EX | (0 if wait else fcntl.LOCK_NB))
            return
        if msvcrt is None:
            raise RuntimeError("file locking needs fcntl (POSIX) or msvcrt (Windows)")
        mode = msvcrt.LK_LOCK if wait else msvcrt.LK_NBLCK
        try:
            handle.seek(0)
            msvcrt.locking(handle.fileno(), mode, 1)
        except OSError as error:
            if wait:
                raise
            raise BlockingIOError(str(error)) from error

    @classmethod
    def unlock(cls, handle: IO[Any]) -> None:
        """Releases the lock taken by `lock`.

        Args:
            handle: The file that was locked.

        Raises:
            RuntimeError: The platform has neither `fcntl` nor `msvcrt`.
        """
        if fcntl is not None:
            fcntl.flock(handle, fcntl.LOCK_UN)
            return
        if msvcrt is None:
            raise RuntimeError("file locking needs fcntl (POSIX) or msvcrt (Windows)")
        handle.seek(0)
        msvcrt.locking(handle.fileno(), msvcrt.LK_UNLCK, 1)
