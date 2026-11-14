"""The eval stage of `ragon-admin adapter train` (docs/dev/adapter-train-plan.md, "The eval").

Five checks compare the served base with the base plus the adapter, on the same node:

- E1: the hold-out tasks' resolve rate, with the generate stage's own runner and grading, so the
  train and eval numbers cannot drift apart. "Gain" is at least two more tasks resolved.
- E2: the Python control slice, when its images are present, else skipped with a note; at most one
  task lost.
- E3: single-stream decode speed (`scripts/decode_speed.py`), with the adapter's own drafter and
  with the base drafter; a warning above 5% loss, nothing refused (decision D3).
- E4: the outputs are identical with speculation on and off.
- E5: `ragon-admin audit egress` against the server with the adapter selected passes.

The stage writes `eval/report.json` and `eval/report.md` in the run's directory. Every server and
agent call goes through a method that a test replaces.
"""

import json
import shutil
import statistics
import subprocess
import sys
import tempfile
import urllib.request
from collections.abc import Callable, Sequence
from pathlib import Path
from typing import Any, Final

REPORT_JSON: Final[str] = "report.json"
REPORT_MD: Final[str] = "report.md"
EVAL_DIR: Final[str] = "eval"
GAIN_MIN: Final[int] = 2
CONTROL_LOSS_MAX: Final[int] = 1
SPEED_LOSS_WARN_PCT: Final[float] = 5.0
CONFIG_FILE: Final[str] = "train-config.json"
SKIPPED: Final[str] = "skipped"
BASE: Final[str] = "base"
ADAPTER: Final[str] = "adapter"
HOLDOUT: Final[str] = "holdout"
BASE_DRAFTER: Final[str] = "adapter_base_drafter"
WARNINGS: Final[str] = "warnings"
# The hosts a run's train-config.json names under `eval_hosts`.
HOST_KEYS: Final[tuple[str, ...]] = (
    "base",
    "adapter",
    "adapter_speculation_off",
    BASE_DRAFTER,
)
EXACTNESS_PROMPTS: Final[tuple[str, ...]] = (
    "Write a Python function that reverses a linked list.",
    "Explain what a mutex is in two sentences.",
    "List the first ten prime numbers.",
    "Write a Rust function that returns the nth Fibonacci number.",
)
EXACTNESS_MAX_TOKENS: Final[int] = 200
REQUEST_TIMEOUT_S: Final[int] = 600
DECODE_TIMEOUT_S: Final[int] = 1800
DECODE_RUNS: Final[int] = 3


class AdapterTrainEval:
    """Runs E1 to E5 for one training run and writes the report."""

    def __init__(  # noqa: PLR0913 - every input of the stage, named
        self,
        out_dir: Path,
        hosts: dict[str, str],
        holdout_runner: Callable[[str, str], dict[str, Any]] | None = None,
        control_runner: Callable[[str], dict[str, Any]] | None = None,
        speed_runner: Callable[[str], float] | None = None,
        completer: Callable[[str, str], str] | None = None,
        auditor: Callable[[str], int] | None = None,
        prompts: Sequence[str] = EXACTNESS_PROMPTS,
    ) -> None:
        """Set up the stage.

        Args:
            out_dir: The training run's directory; the report goes to its `eval/` folder.
            hosts: Server URLs by role: `base`, `adapter`, and optionally `adapter_speculation_off`
                and `adapter_base_drafter`.
            holdout_runner: `(host, label) -> {"resolved": [task ids], "attempted": [task ids]}`,
                the generate stage's runner and grading over the hold-out tasks; required for E1.
            control_runner: `(host) -> {"resolved": [...], "attempted": [...]}` over the Python
                control slice; None skips E2 with a note.
            speed_runner: `(host) -> tokens per second`; `scripts/decode_speed.py` by default.
            completer: `(host, prompt) -> text` at temperature 0; the server's chat endpoint by default.
            auditor: `(host) -> exit code` of the egress audit; `ragon-admin audit egress` by default.
            prompts: The prompts of the exactness check.

        Raises:
            ValueError: When the base or adapter host is missing.
        """
        for key in (BASE, ADAPTER):
            if not hosts.get(key):
                raise ValueError(f"the eval stage needs the {key} server's host")
        self.out_dir = Path(out_dir)
        self.hosts = hosts
        self.holdout_runner = holdout_runner
        self.control_runner = control_runner
        self.speed_runner = speed_runner or self.decode_speed
        self.completer = completer or self.complete
        self.auditor = auditor or self.audit_egress
        self.prompts = tuple(prompts)

    @classmethod
    def run(cls, run: Any) -> int:  # noqa: ANN401 - AdapterTrainRun, from the CLI lane
        """The stage as `adapter train` runs it.

        Args:
            run: The `AdapterTrainRun`; its `train-config.json` names `eval_hosts`.

        Returns:
            int: 0 when the report was written and E4 and E5 passed, else 1.
        """
        config_path = Path(run.directory) / CONFIG_FILE
        try:
            config = json.loads(config_path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            config = {}
        hosts = {k: str(v) for k, v in (config.get("eval_hosts") or {}).items() if k in HOST_KEYS}
        if not hosts.get(BASE) or not hosts.get(ADAPTER):
            print(  # noqa: T201 - the CLI's voice
                "❌ The eval stage needs the base and the adapter served: name them under "
                "`eval_hosts` in the run's train-config.json (the install stage serves the adapter)."
            )
            return 1
        runner = cls.generate_holdout_runner(Path(run.directory), config)
        stage = cls(Path(run.directory), hosts, holdout_runner=runner)
        report = stage.execute()
        print(f"✅ Eval report: {stage.report_path(REPORT_MD)}")  # noqa: T201
        return 0 if report["passed"] else 1

    @classmethod
    def generate_holdout_runner(
        cls, out_dir: Path, config: dict[str, Any]
    ) -> Callable[[str, str], dict[str, Any]] | None:
        """The hold-out runner built on the generate stage, or None when it cannot be built.

        Args:
            out_dir: The run's directory.
            config: The run's `train-config.json`.

        Returns:
            Callable | None: `(host, label) -> {"resolved", "attempted"}`.
        """
        repo = config.get("repo")
        if not repo:
            return None

        def runner(host: str, label: str) -> dict[str, Any]:
            from dreamference.hardware.adapter_train_generate import AdapterTrainGenerate
            from dreamference.hardware.adapter_train_tasks import AdapterTrainTasks

            _, holdout = AdapterTrainTasks.collect(Path(repo))
            stage = AdapterTrainGenerate(
                Path(repo),
                out_dir / EVAL_DIR / label,
                [],
                holdout,
                host,
                test_cmd=config.get("test_cmd"),
                attempts=int(config.get("eval_attempts", 2)),
            )
            return cls.grade_holdout(stage)

        return runner

    @classmethod
    def grade_holdout(cls, stage: Any) -> dict[str, Any]:  # noqa: ANN401 - an AdapterTrainGenerate
        """Attempts and grades every hold-out task with a generate stage.

        A task is resolved when any attempt passes; tasks without a FAIL_TO_PASS target are not
        attempted.

        Args:
            stage: An `AdapterTrainGenerate` built with the hold-out tasks.

        Returns:
            dict[str, Any]: `resolved` and `attempted` task ids.
        """
        stage.prepare_clone()
        if not stage.detect_all():
            return {"resolved": [], "attempted": [], "stopped": True}
        resolved: list[str] = []
        attempted: list[str] = []
        for spec, split in stage.tasks:
            if split != HOLDOUT:
                continue
            grading = stage.load_state(spec["id"])["detect"]
            if not grading.get("fail_to_pass"):
                continue
            attempted.append(spec["id"])
            for number in range(1, stage.attempts + 1):
                record = stage.attempt(spec, grading, number)
                if record is None:
                    return {"resolved": resolved, "attempted": attempted, "stopped": True}
                if record["passed"]:
                    resolved.append(spec["id"])
                    break
        return {"resolved": resolved, "attempted": attempted}

    # -- the stage --------------------------------------------------------------------------

    def execute(self) -> dict[str, Any]:
        """Runs E1 to E5 and writes the report.

        Returns:
            dict[str, Any]: The report: one entry per check, `warnings`, and `passed` (E4 and E5
            passed, and E1 and E2 ran or were skipped with a note).
        """
        report: dict[str, Any] = {
            "hosts_checked": sorted(self.hosts),
            "E1": self.check_holdout(),
            "E2": self.check_control(),
            "E3": self.check_speed(),
            "E4": self.check_exactness(),
            "E5": self.check_egress(),
        }
        report["warnings"] = [
            w for key in ("E1", "E2", "E3") for w in report[key].get(WARNINGS, [])
        ]
        report["make_default"] = bool(report["E1"].get("gain")) and not report["E2"].get(
            "regressed"
        )
        report["passed"] = bool(report["E4"]["passed"] and report["E5"]["passed"])
        directory = self.out_dir / EVAL_DIR
        directory.mkdir(parents=True, exist_ok=True)
        self.report_path(REPORT_JSON).write_text(
            json.dumps(report, indent=2) + "\n", encoding="utf-8"
        )
        self.report_path(REPORT_MD).write_text(self.markdown(report), encoding="utf-8")
        return report

    def report_path(self, name: str) -> Path:
        """A file of the report.

        Args:
            name: `report.json` or `report.md`.

        Returns:
            Path: Inside `<run>/eval/`.
        """
        return self.out_dir / EVAL_DIR / name

    def check_holdout(self) -> dict[str, Any]:
        """E1: the hold-out resolve rate, base against adapter.

        Returns:
            dict[str, Any]: Both resolved lists, the counts and `gain` (at least two more).
        """
        if self.holdout_runner is None:
            return {
                "status": SKIPPED,
                "note": "no repository to grade the hold-out in",
                "gain": False,
            }
        base = self.holdout_runner(self.hosts[BASE], BASE)
        adapter = self.holdout_runner(self.hosts[ADAPTER], ADAPTER)
        difference = len(adapter["resolved"]) - len(base["resolved"])
        entry: dict[str, Any] = {
            "status": "ran",
            "base_resolved": len(base["resolved"]),
            "adapter_resolved": len(adapter["resolved"]),
            "attempted": len(base["attempted"]),
            "difference": difference,
            "gain": difference >= GAIN_MIN,
            "warnings": [],
        }
        if not entry["gain"]:
            entry["warnings"].append(
                f"E1: no gain ({difference:+d}, {GAIN_MIN} needed): not made the default"
            )
        return entry

    def check_control(self) -> dict[str, Any]:
        """E2: the Python control slice; at most one task lost.

        Returns:
            dict[str, Any]: The counts and `regressed`, or a skipped note.
        """
        if self.control_runner is None:
            return {
                "status": SKIPPED,
                "note": "the Python control slice's images are not present on this node",
                "regressed": False,
            }
        base = self.control_runner(self.hosts[BASE])
        adapter = self.control_runner(self.hosts[ADAPTER])
        lost = sorted(set(base["resolved"]) - set(adapter["resolved"]))
        entry: dict[str, Any] = {
            "status": "ran",
            "base_resolved": len(base["resolved"]),
            "adapter_resolved": len(adapter["resolved"]),
            "lost": lost,
            "regressed": len(lost) > CONTROL_LOSS_MAX,
            "warnings": [],
        }
        if entry["regressed"]:
            entry["warnings"].append(
                f"E2: {len(lost)} control tasks lost, at most {CONTROL_LOSS_MAX}"
            )
        return entry

    def check_speed(self) -> dict[str, Any]:
        """E3: single-stream decode speed, with the base-drafter row.

        Returns:
            dict[str, Any]: Tokens per second per row, the loss in percent and a warning above 5%.
        """
        rows = {
            BASE: self.speed_runner(self.hosts[BASE]),
            ADAPTER: self.speed_runner(self.hosts[ADAPTER]),
        }
        if self.hosts.get(BASE_DRAFTER):
            rows[BASE_DRAFTER] = self.speed_runner(self.hosts[BASE_DRAFTER])
        base = rows[BASE]
        loss = {
            name: round((base - tps) / base * 100.0, 2) if base > 0 else 0.0
            for name, tps in rows.items()
            if name != BASE
        }
        entry: dict[str, Any] = {
            "status": "ran",
            "tokens_per_second": rows,
            "loss_pct": loss,
            "warnings": [],
        }
        for name, pct in loss.items():
            if pct > SPEED_LOSS_WARN_PCT:
                entry["warnings"].append(
                    f"E3: {name} decodes {pct:.1f}% slower than the base (warning above "
                    f"{SPEED_LOSS_WARN_PCT:.0f}%; nothing refused)"
                )
        if BASE_DRAFTER not in rows:
            entry["note"] = "no base-drafter server named: that row is not recorded"
        return entry

    def check_exactness(self) -> dict[str, Any]:
        """E4: identical outputs with speculation on and off.

        Returns:
            dict[str, Any]: `passed` and the prompts that differed; skipped (not passed) when no
            speculation-off server is named.
        """
        off = self.hosts.get("adapter_speculation_off")
        if not off:
            return {"status": SKIPPED, "passed": False, "note": "no speculation-off server named"}
        differing = [
            prompt
            for prompt in self.prompts
            if self.completer(self.hosts[ADAPTER], prompt) != self.completer(off, prompt)
        ]
        return {
            "status": "ran",
            "passed": not differing,
            "differing": differing,
            "prompts": len(self.prompts),
        }

    def check_egress(self) -> dict[str, Any]:
        """E5: the egress audit with the adapter selected.

        Returns:
            dict[str, Any]: `passed` and the audit's exit code.
        """
        code = self.auditor(self.hosts[ADAPTER])
        return {"status": "ran", "passed": code == 0, "exit_code": code}

    # -- calls to servers and tools ---------------------------------------------------------

    @classmethod
    def complete(cls, host: str, prompt: str) -> str:
        """One greedy chat completion from a server.

        Args:
            host: The server's URL.
            prompt: The user message.

        Returns:
            str: The reply text.
        """
        models = json.load(urllib.request.urlopen(f"{host}/v1/models", timeout=30))
        body = json.dumps(
            {
                "model": models["data"][0]["id"],
                "messages": [{"role": "user", "content": prompt}],
                "temperature": 0,
                "max_tokens": EXACTNESS_MAX_TOKENS,
            }
        ).encode()
        request = urllib.request.Request(
            f"{host}/v1/chat/completions", body, {"Content-Type": "application/json"}
        )
        reply = json.load(urllib.request.urlopen(request, timeout=REQUEST_TIMEOUT_S))
        return str(reply["choices"][0]["message"].get("content") or "")

    @classmethod
    def decode_speed(cls, host: str) -> float:
        """Single-stream decode tokens per second, through `scripts/decode_speed.py`.

        Args:
            host: The server's URL.

        Returns:
            float: The median over the script's prompt kinds.

        Raises:
            RuntimeError: When the checkout's script is missing or the run fails.
        """
        script = Path(__file__).resolve().parents[2] / "scripts" / "decode_speed.py"
        if not script.is_file():
            raise RuntimeError(
                "scripts/decode_speed.py is not in this install: E3 needs a checkout"
            )
        scratch = tempfile.mkdtemp(prefix="adapter-eval-")
        out = Path(scratch) / "decode.json"
        try:
            done = subprocess.run(
                [
                    sys.executable,
                    str(script),
                    "--host",
                    host,
                    "--runs",
                    str(DECODE_RUNS),
                    "--out",
                    str(out),
                ],
                capture_output=True,
                text=True,
                timeout=DECODE_TIMEOUT_S,
                check=False,
            )
            if done.returncode != 0:
                raise RuntimeError(
                    f"decode_speed.py failed: {done.stdout[-300:]}{done.stderr[-300:]}"
                )
            kinds = json.loads(out.read_text(encoding="utf-8"))["kinds"]
        finally:
            shutil.rmtree(scratch, ignore_errors=True)
        return float(statistics.median(float(k["decode_tps"]) for k in kinds.values()))

    @classmethod
    def audit_egress(cls, host: str) -> int:
        """`ragon-admin audit egress` with the model server named.

        Args:
            host: The server serving the adapter.

        Returns:
            int: The audit's exit code.
        """
        from dreamference.audit.egress_audit import EgressAudit

        return int(EgressAudit.run(vllm_host=host))

    @classmethod
    def markdown(cls, report: dict[str, Any]) -> str:
        """The report as text.

        Args:
            report: `execute()`'s result.

        Returns:
            str: Markdown, one line per check.
        """
        lines = ["# Adapter eval", ""]
        for key in ("E1", "E2", "E3", "E4", "E5"):
            entry = report[key]
            detail = ", ".join(f"{k}={v}" for k, v in entry.items() if k != WARNINGS)
            lines.append(f"- {key}: {detail}")
        lines += [
            "",
            f"Make the adapter the default: {report['make_default']}",
            f"Passed: {report['passed']}",
        ]
        lines += [f"Warning: {w}" for w in report["warnings"]]
        return "\n".join(lines) + "\n"
