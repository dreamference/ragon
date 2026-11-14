"""One adapter training run's directory and stage state (docs/dev/adapter-train-plan.md).

A run lives under `~/.local/share/dreamference/adapters/train/<name>/`, never in the repository
it learns from. `state.json` records the finished stages, the hosts the person consented to send
data to and everything that left the node; `train-config.json` keeps the recipe so the run can
be reproduced. A stage class from a later lane is found by name: `STAGE_CLASSES` says which
module and class a stage runs, and which lane brings it while the module is missing.
"""

import json
import re
from pathlib import Path
from typing import Any, Final

STAGES: Final[tuple[str, ...]] = ("data", "generate", "train", "eval", "install")
RUN_NAME: Final[re.Pattern[str]] = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$")
STATE_FILE: Final[str] = "state.json"
CONFIG_FILE: Final[str] = "train-config.json"
# stage: (module, class, the lane that brings it). A stage class has `run(run: AdapterTrainRun)
# -> int`; the data stage is in this package already.
STAGE_CLASSES: Final[dict[str, tuple[str, str, str]]] = {
    "data": ("dreamference.hardware.adapter_train_tasks", "AdapterTrainTasks", "adt-data"),
    "generate": ("dreamference.hardware.adapter_train_generate", "AdapterTrainGenerate", "adt-generate"),
    "train": ("dreamference.hardware.adapter_train_trainer", "AdapterTrainTrainer", "adt-train-export"),
    "eval": ("dreamference.hardware.adapter_train_eval", "AdapterTrainEval", "adt-eval"),
    "install": ("dreamference.hardware.adapter_train_install", "AdapterTrainInstall", "adt-train-export"),
}  # fmt: skip
# The estimate the pilot replaces (adapter-train-plan.md, "GPU hours"): trajectories kept,
# tokens each, the per-epoch cap, and the sustained rate on one 5090.
KEPT_TRAJECTORIES_MAX: Final[int] = 250
TOKENS_PER_TRAJECTORY: Final[int] = 20_000
TOKENS_PER_EPOCH_CAP: Final[int] = 3_000_000
DEFAULT_TOKENS_PER_SECOND: Final[float] = 225.0
DEFAULT_EPOCHS: Final[int] = 2


class AdapterTrainRun:
    """The directory and stage state of one named run."""

    def __init__(self, name: str) -> None:
        """Bind a run to its directory (nothing is created yet).

        Args:
            name: The adapter's name.

        Raises:
            ValueError: When the name is not a plain file name.
        """
        if not RUN_NAME.match(name):
            raise ValueError(
                f"{name!r} is not a usable run name: letters, digits, '.', '_' and '-', "
                "starting with a letter or digit"
            )
        self.name = name

    @classmethod
    def root(cls) -> Path:
        """The folder holding every run.

        Returns:
            Path: `~/.local/share/dreamference/adapters/train`, under the current home.
        """
        return Path("~/.local/share/dreamference/adapters/train").expanduser()

    @property
    def directory(self) -> Path:
        """The run's own directory.

        Returns:
            Path: `<root>/<name>`.
        """
        return self.root() / self.name

    def state(self) -> dict[str, Any]:
        """Read the stage state; an empty one when the run has none.

        Returns:
            dict[str, Any]: `done` (stage names), `consented` (hosts), `left_node` (records).
        """
        path = self.directory / STATE_FILE
        try:
            loaded = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            loaded = {}
        if not isinstance(loaded, dict):
            loaded = {}
        return {
            "done": [s for s in loaded.get("done", []) if s in STAGES],
            "consented": [str(h) for h in loaded.get("consented", [])],
            "left_node": list(loaded.get("left_node", [])),
        }

    def _write(self, name: str, content: dict[str, Any]) -> None:
        self.directory.mkdir(parents=True, exist_ok=True, mode=0o700)
        (self.directory / name).write_text(json.dumps(content, indent=2) + "\n", encoding="utf-8")

    def mark_done(self, stage: str) -> None:
        """Record a finished stage.

        Args:
            stage: One of `STAGES`.
        """
        state = self.state()
        if stage not in state["done"]:
            state["done"].append(stage)
        self._write(STATE_FILE, state)

    def record_consent(self, host: str) -> None:
        """Record that the person typed this host's name.

        Args:
            host: The trainer host.
        """
        state = self.state()
        if host not in state["consented"]:
            state["consented"].append(host)
        self._write(STATE_FILE, state)

    def record_upload(self, host: str, what: str) -> None:
        """Record something that left the node, for the run's report.

        Args:
            host: Where it went.
            what: What it was (a file name and size, say).
        """
        state = self.state()
        state["left_node"].append({"host": host, "what": what})
        self._write(STATE_FILE, state)

    def write_config(self, config: dict[str, Any]) -> None:
        """Keep the recipe the run was started with.

        Args:
            config: The hyperparameters and options.
        """
        self._write(CONFIG_FILE, config)

    def next_stage(self) -> str | None:
        """The first stage not done.

        Returns:
            str | None: Its name, or None when all are done.
        """
        done = self.state()["done"]
        return next((s for s in STAGES if s not in done), None)

    @classmethod
    def estimate(
        cls,
        training_tasks: int,
        epochs: int = DEFAULT_EPOCHS,
        tokens_per_second: float | None = None,
        budget_hours: float | None = None,
    ) -> dict[str, float]:
        """Estimate tokens and GPU hours (the plan's default figures, until a pilot measures).

        Args:
            training_tasks: Candidate tasks outside the hold-out.
            epochs: Passes over the data.
            tokens_per_second: A measured rate, or None for the default.
            budget_hours: When set, trims the tokens to fit it.

        Returns:
            dict[str, float]: `tokens`, `tokens_per_second`, `gpu_hours`.
        """
        rate = tokens_per_second or DEFAULT_TOKENS_PER_SECOND
        kept = min(training_tasks, KEPT_TRAJECTORIES_MAX)
        tokens = float(epochs * min(kept * TOKENS_PER_TRAJECTORY, TOKENS_PER_EPOCH_CAP))
        if budget_hours is not None:
            tokens = min(tokens, budget_hours * 3600 * rate)
        return {"tokens": tokens, "tokens_per_second": rate, "gpu_hours": tokens / rate / 3600}
