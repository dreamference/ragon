# ruff: noqa: PLR2004 - field kinds, levels and byte widths are the spec's literals
"""The Python writer (LOGGING §5.4, §6.2, §7.5): a call copies into a bounded queue; a thread writes.

A log call costs a copy, never a lock on a file or a pipe; a full queue drops and counts. A writer
never raises into the code that called it and never falls back to a second sink.
"""

import atexit
import queue
import threading
import time
from collections.abc import Callable
from typing import Final

from dreamference.config.log_field import LogField
from dreamference.config.log_level_spec import LogLevelSpec
from dreamference.config.log_line_formatter import LogLineFormatter
from dreamference.config.log_redactor import LogRedactor
from dreamference.config.log_sampler import LogSampler
from dreamference.config.log_sink import LogSink

QUEUE_SIZE: Final[int] = 8192
LEVELS: Final[tuple[str, ...]] = ("error", "warn", "info", "debug", "trace")
RETRY_AFTER_S: Final[float] = 1.0
Record = tuple[float, int, str, tuple[LogField, ...]]


class LogWriter:
    """One process's log writer; unconfigured it drops everything, so import-time calls are safe."""

    def __init__(self, clock: Callable[[], float] = time.time) -> None:
        """Create an unconfigured writer.

        Args:
            clock: Wall clock; tests pass a fake one.
        """
        self.clock = clock
        self.component = ""
        self.sink: LogSink | None = None
        self.redactor = LogRedactor()
        self.limit = -1
        self.sampler = LogSampler()
        self.queue: queue.Queue[Record | None] = queue.Queue(QUEUE_SIZE)
        self.queue_drops = 0
        self.io_drops = 0
        self.retry_at = 0.0
        self.thread: threading.Thread | None = None
        self._start_lock = threading.Lock()
        self._atexit = False

    def configure(
        self,
        component: str,
        spec: LogLevelSpec,
        sink: LogSink,
        redactor: LogRedactor,
        default: str = "info",
    ) -> None:
        """Choose the component, its level and its sink; may be called again.

        Args:
            component: The component name.
            spec: The parsed `RAGON_LOG` / `log_level`.
            sink: Where lines go.
            redactor: The process's redactor.
            default: The level when the spec names none.
        """
        level = spec.overrides.get(component) or spec.default or default
        self.component = component
        self.limit = LEVELS.index(level) if level in LEVELS else (-1 if level == "off" else 2)
        self.sampler = LogSampler(verbose=self.limit >= 3)
        self.redactor = redactor
        old, self.sink = self.sink, sink
        if old is not None and old is not sink:
            old.close()

    def enabled(self, level: int) -> bool:
        """Return whether a line at this level (0 error .. 4 trace) is written."""
        return level <= self.limit

    def error(self, message: str, *fields: LogField | tuple[LogField, ...]) -> None:
        """Log at ERROR; `message` is a literal."""
        self._emit(0, message, fields)

    def warn(self, message: str, *fields: LogField | tuple[LogField, ...]) -> None:
        """Log at WARN; `message` is a literal."""
        self._emit(1, message, fields)

    def info(self, message: str, *fields: LogField | tuple[LogField, ...]) -> None:
        """Log at INFO; `message` is a literal."""
        self._emit(2, message, fields)

    def debug(self, message: str, *fields: LogField | tuple[LogField, ...]) -> None:
        """Log at DEBUG; `message` is a literal."""
        self._emit(3, message, fields)

    def trace(self, message: str, *fields: LogField | tuple[LogField, ...]) -> None:
        """Log at TRACE; `message` is a literal."""
        self._emit(4, message, fields)

    def _emit(self, level: int, message: str, fields: tuple[object, ...]) -> None:
        if level > self.limit or self.sink is None:
            return
        try:
            flat = tuple(
                item for part in fields for item in (part if isinstance(part, tuple) else (part,))
            )
            self._start()
            self.queue.put_nowait((self.clock(), level, message, flat))
        except queue.Full:
            self.queue_drops += 1
        except Exception:  # noqa: BLE001 - a log call never fails the work
            self.io_drops += 1

    def _start(self) -> None:
        if self.thread is not None and self.thread.is_alive():
            return
        with self._start_lock:
            if self.thread is None or not self.thread.is_alive():
                self.thread = threading.Thread(
                    target=self._run, name="ragon-log-writer", daemon=True
                )
                self.thread.start()
                if not self._atexit:
                    atexit.register(self.finish)
                    self._atexit = True

    def _run(self) -> None:
        while True:
            try:
                record = self.queue.get(timeout=1.0)
            except queue.Empty:
                self.tick(self.clock())
                continue
            try:
                if record is None:
                    return
                self.process(record)
            except Exception:  # noqa: BLE001 - a log call never fails the work
                self.io_drops += 1
            finally:
                self.queue.task_done()

    def tick(self, now: float) -> None:
        """Write the notes a finished second leaves, without a new record."""
        self._notes(now, int(now))

    def process(self, record: Record) -> None:
        """Sample, format and write one record (the writer thread's work; tests call it directly)."""
        epoch, level, message, fields = record
        self._notes(epoch, int(epoch))
        ms = next(
            (float(f.value) for f in fields if f.key == "ms" and isinstance(f.value, int | float)),
            None,
        )
        if self.sampler.admit(level, LogLineFormatter.message(message), ms):
            self._write(epoch, level, message, fields)

    def _notes(self, epoch: float, second: int) -> None:
        for level, message, site, numbers in self.sampler.roll(second):
            fields: list[LogField] = []
            if site:
                fields.append(LogField("quoted", "site", site))
            fields.extend(LogField("count", key, value) for key, value in numbers.items())
            self._write(epoch, level, message, tuple(fields))

    def _write(self, epoch: float, level: int, message: str, fields: tuple[LogField, ...]) -> None:
        sink = self.sink
        if sink is None:
            return
        if epoch < self.retry_at:
            self.io_drops += 1
            return
        for kind, count, extra in (
            ("queue", self._take("queue_drops"), ()),
            ("io", self._take("io_drops"), ("io",)),
        ):
            if count:
                notice = (
                    LogField("count", "count", count),
                    *(LogField("enum", "reason", e, ("io",)) for e in extra),
                )
                if not self._put(sink, epoch, 1, "log lines dropped", notice):
                    setattr(self, f"{kind}_drops", getattr(self, f"{kind}_drops") + count)
                    self.retry_at = epoch + RETRY_AFTER_S
                    self.io_drops += 1
                    return
        if not self._put(sink, epoch, level, message, fields):
            self.io_drops += 1
            self.retry_at = epoch + RETRY_AFTER_S

    def _take(self, name: str) -> int:
        value = getattr(self, name)
        setattr(self, name, 0)
        return int(value)

    def _put(
        self, sink: LogSink, epoch: float, level: int, message: str, fields: tuple[LogField, ...]
    ) -> bool:
        line = LogLineFormatter.format(epoch, self.component, level, message, fields, self.redactor)
        try:
            sink.write(line)
        except Exception:  # noqa: BLE001 - a log call never fails the work
            return False
        return True

    def flush(self, timeout: float = 5.0) -> None:
        """Wait until every queued record is processed."""
        if self.thread is None or not self.thread.is_alive():
            while True:
                try:
                    record = self.queue.get_nowait()
                except queue.Empty:
                    return
                if record is not None:
                    self.process(record)
                self.queue.task_done()
        deadline = time.monotonic() + timeout
        while self.queue.unfinished_tasks and time.monotonic() < deadline:
            time.sleep(0.005)

    def finish(self) -> None:
        """Flush and write the notes of the last second, at exit."""
        try:
            self.flush()
            self.tick(self.clock() + 11)
        except Exception:  # noqa: BLE001, S110 - a log call never fails the work
            pass
