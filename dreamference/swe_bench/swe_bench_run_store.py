"""One run's files.

See specs/DREAMFERENCE_RAGON_SWE_BENCH.md §4:

```text
runs/<run>/manifest.json       what was measured; written once
runs/<run>/instances/<id>.json one instance's state, replaced whole
runs/<run>/logs/<id>.jsonl     ragon exec's events
runs/<run>/predictions.jsonl   appended, one line per finished instance
runs/<run>/eval/<n>/           grading n: the harness's logs, and what it was graded with
runs/<run>/eval-<variant>/<n>/ the same for another series (`drop-test-hunks`: test files left out)
runs/<run>/report.md           written by `report`
```
"""

import json
import os
import re
import threading
from pathlib import Path
from typing import Any, Final

from dreamference.swe_bench import swe_bench_settings
from dreamference.swe_bench.swe_bench_scratch import SweBenchScratch

COMMAND_EXECUTION_TEXT: Final[str] = "command_execution"
COMPACTED_TEXT: Final[str] = "compacted"
COMPACTION_GAP_SECONDS: Final[int] = 2
ITEM_COMPLETED_TEXT: Final[str] = "item.completed"
MCP_TOOL_CALL_TEXT: Final[str] = "mcp_tool_call"
NEWLINE_BYTE: Final[bytes] = b"\n"
TURN_COMPLETED_TEXT: Final[str] = "turn.completed"

# Statuses after which an instance's agent phase is over and its prediction is written.
FINISHED_STATUSES: Final[tuple] = ("done", "empty", "stalled", "timeout", "error")

_PREDICTIONS_LOCK: Final[threading.Lock] = threading.Lock()

# A command that asks the index something. Naming the binary is not enough: in the first run
# with the index, the one command that mentioned it was `ls /opt/ragon-code/bin`.
RAGON_CODE_QUERY: Final[re.Pattern] = re.compile(
    r"\bragon-code\s+(def|refs|callers|callees|impl|impact|show|outline|search|status)\b"
)

# The same questions asked through the tools the launcher gives the model
# (`code_def`, `code_search`, … served by `ragon-code mcp`).
RAGON_CODE_TOOL: Final[re.Pattern] = re.compile(
    r"^code_(def|refs|callers|callees|impl|impact|show|outline|search|status)$"
)


class SweBenchRunStore:
    """Reads and writes a run directory."""

    def __init__(self, name: str) -> None:
        """Init.

        Args:
        name: The run's name.
        """
        self.name = name
        self.directory: Path = swe_bench_settings.RESULTS_DIR / "runs" / name

    @classmethod
    def runs(cls) -> list[str]:
        """Runs.

        Returns:
        List[str]: The names of the runs on disk, oldest first: by their manifest, written once
        when the run starts (the directory's own time moves with every report and gate file).
        """
        root = swe_bench_settings.RESULTS_DIR / "runs"
        if not root.is_dir():
            return []
        manifests = [entry / "manifest.json" for entry in root.iterdir()]
        started = sorted(
            (manifest for manifest in manifests if manifest.is_file()),
            key=lambda manifest: manifest.stat().st_mtime,
        )
        return [manifest.parent.name for manifest in started]

    @classmethod
    def latest(cls) -> str | None:
        """Latest.

        Returns:
        Optional[str]: The most recently started run, if any.
        """
        runs = cls.runs()
        return runs[-1] if runs else None

    # -- manifest ------------------------------------------------------------------------------

    @property
    def manifest_path(self) -> Path:
        """The run's manifest."""
        return self.directory / "manifest.json"

    def manifest(self) -> dict[str, Any] | None:
        """Manifest.

        Returns:
        Optional[Dict[str, Any]]: The manifest, or None when the run does not exist.
        """
        return self._read(self.manifest_path)

    def write_manifest(self, manifest: dict[str, Any]) -> None:
        """Writes the manifest of a new run. It is never edited afterwards.

        Args:
            manifest: The manifest (§6.4).

        Raises:
            FileExistsError: When the run already has one.
        """
        if self.manifest_path.exists():
            raise FileExistsError(self.manifest_path)
        for sub in ("instances", "logs", "eval"):
            (self.directory / sub).mkdir(parents=True, exist_ok=True)
        self._write(self.manifest_path, manifest)

    # -- instances -----------------------------------------------------------------------------

    def state(self, instance_id: str) -> dict[str, Any] | None:
        """State.

        Args:
            instance_id: The instance.

        Returns:
            Optional[Dict[str, Any]]: Its state, or None when it has not started.
        """
        return self._read(self.directory / "instances" / f"{instance_id}.json")

    def write_state(self, instance_id: str, state: dict[str, Any]) -> None:
        """Replaces an instance's state.

        Args:
            instance_id: The instance.
            state: Its state; `status` is one of `running`, `done`, `empty`, `stalled`,
                `timeout`, `error`.
        """
        self._write(self.directory / "instances" / f"{instance_id}.json", state)

    def states(self) -> dict[str, dict[str, Any]]:
        """States.

        Returns:
        Dict[str, Dict[str, Any]]: Every instance state on disk, by instance id.
        """
        directory = self.directory / "instances"
        if not directory.is_dir():
            return {}
        found = {}
        for path in sorted(directory.glob("*.json")):
            state = self._read(path)
            if state is not None:
                found[path.stem] = state
        return found

    def finished(self) -> list[str]:
        """Finished.

        Returns:
        List[str]: Instances whose prediction is written, in the order they finished.
        """
        return [prediction["instance_id"] for prediction in self.predictions()]

    def log_path(self, instance_id: str) -> Path:
        """Log path.

        Args:
            instance_id: The instance.

        Returns:
            Path: Where `ragon exec`'s events for it are appended.
        """
        return self.directory / "logs" / f"{instance_id}.jsonl"

    def log_stats(self, instance_id: str, start: int = 0, end: int | None = None) -> dict[str, int]:
        """Counts what the agent did, from `ragon exec`'s events.

        Its commands and tool calls, how many of them asked `ragon-code` something (as a shell
        command or as a `code_*` tool), and the tokens of every turn.

        Args:
            instance_id: The instance.
            start: The byte offset in the log to count from (the refine arm's second step starts
                at the offset its state records).
            end: The byte offset to count up to; None for the end of the log.

        Returns:
            Dict[str, int]: `commands`, `ragon_code_calls`, `input_tokens`,
            `cached_input_tokens`, `output_tokens`; zeros when there is no log.
        """
        stats = {
            "commands": 0,
            "ragon_code_calls": 0,
            "input_tokens": 0,
            "cached_input_tokens": 0,
            "output_tokens": 0,
        }
        try:
            data = self.log_path(instance_id).read_bytes()[start:end]
            lines = data.decode(errors="replace").splitlines()
        except OSError:
            return stats
        for line in lines:
            try:
                event = json.loads(line)
            except ValueError:
                continue  # the first line of every log is not JSON
            if not isinstance(event, dict):
                continue
            self._count_event(stats, event)
        return stats

    @staticmethod
    def _count_event(stats: dict[str, int], event: dict[str, Any]) -> None:
        """Adds one `ragon exec` event to the counts: a command, a tool call or a finished turn's tokens."""
        item = event.get("item") or {}
        if event.get("type") == ITEM_COMPLETED_TEXT and item.get("type") == COMMAND_EXECUTION_TEXT:
            stats["commands"] += 1
            if RAGON_CODE_QUERY.search(str(item.get("command", ""))):
                stats["ragon_code_calls"] += 1
        elif event.get("type") == ITEM_COMPLETED_TEXT and item.get("type") == MCP_TOOL_CALL_TEXT:
            # A tool call is something the agent did, like a command, and counts as one.
            stats["commands"] += 1
            if RAGON_CODE_TOOL.match(str(item.get("tool", ""))):
                stats["ragon_code_calls"] += 1
        elif event.get("type") == TURN_COMPLETED_TEXT:
            usage = event.get("usage") or {}
            for key in ("input_tokens", "cached_input_tokens", "output_tokens"):
                stats[key] += int(usage.get(key) or 0)

    def compactions(self, instance_id: str) -> dict[str, Any] | None:
        """The compactions of an instance's sessions.

        Read from the rollouts kept in its scratch `codex-home`
        (compaction survey §1, §3.5): one per `compacted` record. A
        compaction's time is the gap between its record and the last earlier record more than
        2 s before it, which covers the compaction request's prefill and the summary's decode
        (an upper bound by a second or two).

        Args:
            instance_id: The instance.

        Returns:
            Optional[Dict[str, Any]]: `count` and `seconds` (one duration per compaction, None
            where no earlier record was found); None when no rollout is on disk.
        """
        from datetime import datetime

        # The container wrote the scratch: nothing below its root is followed.
        scratch = self.directory / "scratch" / instance_id
        paths = SweBenchScratch.files(
            scratch, "codex-home/sessions", "rollout*.jsonl", recursive=True
        )
        if not paths:
            return None
        seconds: list[float | None] = []
        for path in paths:
            earlier: list[float] = []
            try:
                lines = SweBenchScratch.read_text(scratch, path).splitlines()
            except OSError:
                continue
            for line in lines:
                try:
                    record = json.loads(line)
                except ValueError:
                    continue  # a line still being written
                if not isinstance(record, dict):
                    continue
                try:
                    stamp: float | None = datetime.fromisoformat(
                        str(record["timestamp"]).replace("Z", "+00:00")
                    ).timestamp()
                except (ValueError, KeyError):
                    stamp = None  # counted, but not timed
                if record.get("type") == COMPACTED_TEXT:
                    before = (
                        None
                        if stamp is None
                        else next(
                            (t for t in reversed(earlier) if stamp - t > COMPACTION_GAP_SECONDS),
                            None,
                        )
                    )
                    seconds.append(None if before is None or stamp is None else stamp - before)
                if stamp is not None:
                    earlier.append(stamp)
        return {"count": len(seconds), "seconds": seconds}

    # -- predictions ---------------------------------------------------------------------------

    @property
    def predictions_path(self) -> Path:
        """The run's predictions file, in the harness's format."""
        return self.directory / "predictions.jsonl"

    def predictions(self) -> list[dict[str, Any]]:
        """Predictions.

        Returns:
        List[Dict[str, Any]]: The predictions written so far. A torn last line (the run was
        killed mid-write) is ignored, so its instance simply runs again.
        """
        try:
            lines = self.predictions_path.read_text().splitlines()
        except OSError:
            return []
        predictions = []
        for line in lines:
            try:
                record = json.loads(line)
            except ValueError:
                continue
            if isinstance(record, dict) and record.get("instance_id"):
                predictions.append(record)
        return predictions

    def append_prediction(self, instance_id: str, model_name: str, patch: str) -> None:
        """Appends one prediction.

        An instance whose agent failed or changed nothing is written with an empty patch, so it is
        unresolved and never missing from the denominator.

        Args:
            instance_id: The instance.
            model_name: The run's `model_name_or_path`.
            patch: The collected patch; may be empty.
        """
        line = json.dumps(
            {"instance_id": instance_id, "model_name_or_path": model_name, "model_patch": patch}
        )
        with _PREDICTIONS_LOCK:
            torn = False
            try:
                with Path(self.predictions_path).open("rb") as handle:
                    handle.seek(-1, os.SEEK_END)
                    torn = handle.read(1) != NEWLINE_BYTE
            except OSError:
                pass
            with Path(self.predictions_path).open("a") as handle:
                handle.write(("\n" if torn else "") + line + "\n")
                handle.flush()
                os.fsync(handle.fileno())

    # -- gradings ------------------------------------------------------------------------------

    @classmethod
    def eval_directory_name(cls, variant: str | None = None) -> str:
        """Eval directory name.

        Args:
            variant: A grading series other than the plain one (`drop-test-hunks`), or None.

        Returns:
            str: The directory under the run that holds the series: `eval`, or `eval-<variant>`.
        """
        return f"eval-{variant}" if variant else "eval"

    def gradings(self, variant: str | None = None) -> list[int]:
        """Gradings.

        Args:
            variant: The grading series; None for the plain one.

        Returns:
            List[int]: The grading numbers on disk, ascending.
        """
        directory = self.directory / self.eval_directory_name(variant)
        if not directory.is_dir():
            return []
        return sorted(
            int(entry.name)
            for entry in directory.iterdir()
            if entry.is_dir() and entry.name.isdigit()
        )

    def grading_dir(self, number: int, variant: str | None = None) -> Path:
        """Grading dir.

        Args:
            number: The grading number.
            variant: The grading series; None for the plain one.

        Returns:
            Path: Its directory, where the harness is run.
        """
        return self.directory / self.eval_directory_name(variant) / str(number)

    def grading(self, number: int, variant: str | None = None) -> dict[str, Any]:
        """Grading.

        Args:
            number: The grading number.
            variant: The grading series; None for the plain one.

        Returns:
            Dict[str, Any]: `{"grader": {...}, "results": {instance id: {...}}}`; empty parts
            when nothing is recorded yet.
        """
        record = self._read(self.grading_dir(number, variant) / "grading.json") or {}
        record.setdefault("grader", {})
        record.setdefault("results", {})
        return record

    def write_grading(
        self, number: int, record: dict[str, Any], variant: str | None = None
    ) -> None:
        """Write grading.

        Args:
        number: The grading number.
        record: The grading record, as `grading()` returns it.
        variant: The grading series; None for the plain one.
        """
        self._write(self.grading_dir(number, variant) / "grading.json", record)

    # -- files ---------------------------------------------------------------------------------

    @classmethod
    def _read(cls, path: Path) -> dict[str, Any] | None:
        try:
            record = json.loads(path.read_text())
        except (OSError, ValueError):
            return None
        return record if isinstance(record, dict) else None

    @classmethod
    def _write(cls, path: Path, record: dict[str, Any]) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        staging = path.with_name(f".{path.name}.{os.getpid()}.{threading.get_ident()}.tmp")
        staging.write_text(json.dumps(record, indent=2, ensure_ascii=False) + "\n")
        Path(staging).replace(path)
