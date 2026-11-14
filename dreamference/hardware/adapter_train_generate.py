"""The generate stage of adapter training: graded, on-policy trajectories from a repository's history.

Each task is one commit of the person's repository: a dict, or an object, with its `commit`,
`parent`, `message` and `tests`. The stage runs in four steps, every one resumable from the state it leaves under
`<out>/generate/`:

1. **Detect.** At the parent, with the commit's touched test files checked out, every test file runs
   once through the test command; then again at the commit. A file that fails at the parent and
   passes at the commit is a FAIL_TO_PASS target; one that passes at both is the touched suite,
   which an attempt must not break. A task with no FAIL_TO_PASS file is dropped and listed.
2. **Refuse** when fewer than `min_gradable` training tasks are gradable: the recipe cannot grade
   what it trains on (the adapters recipe plan; docs/dev/adapters.md).
3. **Attempt.** Best-of-k: each attempt is one Night Shift attempt (`NightShiftTaskRun`, its own
   worktree, its memory-capped scope, the sandboxed test run, `DREAMFERENCE_VLLM_HOST` naming the
   served base) of `ragon exec` on the commit message, told to run only the touched test files that
   exist at the parent: the FAIL_TO_PASS files stay hidden, as in SWE-bench. An attempt passes when, with the commit's
   test files put back, every FAIL_TO_PASS file and every touched-suite file passes.
4. **Render.** The best passing attempt's session rollout is turned into chat messages as
   ragon-serve turns a Responses request into them (`ragon-engine/src/serve/api.cpp`,
   `parse_responses_request`), rendered with the served chat template, and tokenized with the served
   tokenizer, the assistant's tokens masked in. `train.jsonl`, `holdout.jsonl` and `source.json`
   are rebuilt from the state every time, never appended to.

Attempts run in a bare scratch clone of the repository with every ref deleted, so no branch or
worktree lands in the person's checkout and the commit an attempt is meant to write is not one
`git log --all` away.

Two things the sessions do not record are absent from the rendering, and marked so in every record
and in `source.json`: the tool specifications the model was offered (Codex does not write them to the
rollout) and the model's reasoning (not persisted either).
"""

from __future__ import annotations

import hashlib
import json
import os
import shlex
import shutil
import time
from collections.abc import Callable, Sequence
from datetime import datetime, timedelta
from pathlib import Path
from typing import Any, Final

from dreamference.hardware.model_downloader import ModelDownloader
from dreamference.hardware.model_matrix_registry import DEFAULT_MODEL_ALIAS, ModelMatrixRegistry
from dreamference.night_shift.night_shift_settings import NightShiftSettings
from dreamference.night_shift.night_shift_task_run import NO_HOOKS, NightShiftTaskRun
from dreamference.runner.safe_git import SafeGit
from dreamference.vllm_server.chat_template_patcher import ChatTemplatePatcher

DEFAULT_ATTEMPTS: Final[int] = 2
MIN_GRADABLE: Final[int] = 50
# Where a test file goes in `--test-cmd`; without it the file is appended.
TEST_PLACEHOLDER: Final[str] = "{test}"
# The grading unit when the test command cannot take a file (`cargo test`, `npm test`).
WHOLE_SUITE: Final[str] = "*"
STATE_VERSION: Final[int] = 1
# The engine whose recipe carries the chat-template patches ragon-serve applies itself.
TEMPLATE_RECIPE_ENGINE: Final[str] = "sglang"
# Attempt statuses that are not an attempt's end: the window closed or the run was stopped.
UNFINISHED: Final[tuple[str, ...]] = ("interrupted", "cancelled")
ABSENT: Final[str] = "absent"
# Task splits.
TRAIN: Final[str] = "train"
HOLDOUT: Final[str] = "holdout"
# The state key of a task's grading spec.
DETECT: Final[str] = "detect"
# A test command that takes test files appended, one grading unit each.
PYTEST: Final[str] = "pytest"
CANDIDATE: Final[str] = "candidate"
# `timed_out` of a test run the window's end cut short.
WINDOW_CLOSED: Final[str] = "window"
# Rollout lines (`RolloutItem`) and Responses items, as Codex writes them.
COMPACTED: Final[str] = "compacted"
SESSION_META: Final[str] = "session_meta"
TURN_CONTEXT: Final[str] = "turn_context"
RESPONSE_ITEM: Final[str] = "response_item"
MESSAGE: Final[str] = "message"
FUNCTION_CALL: Final[str] = "function_call"
CUSTOM_TOOL_CALL: Final[str] = "custom_tool_call"
TOOL_OUTPUTS: Final[tuple[str, ...]] = ("function_call_output", "custom_tool_call_output")
REASONING: Final[str] = "reasoning"
# The effort that switches thinking off.
EFFORT_NONE: Final[str] = "none"
# Chat roles and message fields.
SYSTEM: Final[str] = "system"
DEVELOPER: Final[str] = "developer"
ASSISTANT: Final[str] = "assistant"
TOOL_CALLS: Final[str] = "tool_calls"
REASONING_CONTENT: Final[str] = "reasoning_content"
NEWLINE: Final[str] = "\n"
# The custom tool's one parameter, as ragon-serve offers it (`kCustomToolInput`).
CUSTOM_TOOL_INPUT: Final[str] = "input"


class AdapterTrainGenerate:
    """Runs the generate stage for one training run's directory."""

    def __init__(  # noqa: PLR0913 - every input of the stage, named
        self,
        repo: Path,
        out_dir: Path,
        train: Sequence[object],
        holdout: Sequence[object],
        model_host: str,
        ragon_bin: str = "ragon",
        test_cmd: str | None = None,
        attempts: int = DEFAULT_ATTEMPTS,
        min_gradable: int = MIN_GRADABLE,
        settings: NightShiftSettings | None = None,
        until: float | None = None,
        model_key: str = DEFAULT_MODEL_ALIAS,
        chat_template: Path | None = None,
        tokenizer: Path | None = None,
    ) -> None:
        """Set up the stage.

        Args:
            repo: The person's checkout the tasks were collected from.
            out_dir: The training run's directory (`~/.local/share/dreamference/adapters/train/<name>`).
            train: The training tasks (dicts, or objects, with `commit`, `parent`, `message`,
                `tests`).
            holdout: The hold-out tasks, the same shape; attempted and graded too, never trained on.
            model_host: The served base every `ragon exec` is pointed at (`DREAMFERENCE_VLLM_HOST`).
            ragon_bin: The `ragon` executable.
            test_cmd: The test command; `{test}` stands for one test file, else the file is
                appended. None detects one per task as Night Shift does.
            attempts: k, the attempts per task.
            min_gradable: The fewest gradable training tasks the stage goes on with.
            settings: Night Shift settings (timeouts, memory cap, test sandbox); the user's
                `[night]` table if None.
            until: `time.time()` at which the stage stops (the window's end); None for no limit.
            model_key: The served model's registry key (ask the server: `model_key_for_served_id`).
            chat_template: A chat template to render with instead of the served model's.
            tokenizer: A `tokenizer.json` to tokenize with instead of the served model's.

        Raises:
            ValueError: Without a model host, or with fewer than one attempt.
        """
        if not model_host:
            raise ValueError("the generate stage needs the served base's host (model_host)")
        if attempts < 1:
            raise ValueError("attempts must be at least 1")
        self.repo = Path(repo)
        self.out_dir = Path(out_dir)
        self.gen_dir = self.out_dir / "generate"
        self.clone = self.gen_dir / "repo"
        self.tasks: list[tuple[dict[str, Any], str]] = [
            (self.task_spec(task), "train") for task in train
        ] + [(self.task_spec(task), "holdout") for task in holdout]
        self.model_host = model_host
        self.ragon_bin = ragon_bin
        self.test_cmd = test_cmd
        self.attempts = attempts
        self.min_gradable = min_gradable
        self.settings = settings or NightShiftSettings(NightShiftSettings.read_table())
        self.until = until
        self.model_key = model_key
        self.chat_template = chat_template
        self.tokenizer = tokenizer

    # -- the stage ------------------------------------------------------------------------------

    def run(self) -> dict[str, Any]:
        """Runs every step not yet done and rebuilds the outputs.

        Returns:
            dict[str, Any]: `status` (`done`, `stopped` or `refused`) and the counts.
        """
        self.prepare_clone()
        if not self.detect_all():
            return self._summary("stopped")
        gradable = [
            spec
            for spec, split in self.tasks
            if split == TRAIN and self.load_state(spec["id"])[DETECT].get("fail_to_pass")
        ]
        if len(gradable) < self.min_gradable:
            return self._summary(
                "refused",
                reason=f"{len(gradable)} gradable training tasks, fewer than {self.min_gradable}",
            )
        if not self.attempt_all():
            return self._summary("stopped")
        self.pack()
        return self._summary("done")

    def detect_all(self) -> bool:
        """Detects every task not yet detected.

        Returns:
            bool: False when the stage stopped before every task was detected.
        """
        for spec, _split in self.tasks:
            state = self.load_state(spec["id"])
            if DETECT in state:
                continue
            found = None if self.stopped() else self.detect(spec)
            if found is None:
                return False
            state[DETECT] = found
            self.save_state(spec["id"], state)
        return True

    def attempt_all(self) -> bool:
        """Makes every attempt of every gradable task not yet made.

        Returns:
            bool: False when the stage stopped before every attempt was made.
        """
        for spec, _split in self.tasks:
            state = self.load_state(spec["id"])
            if not state[DETECT].get("fail_to_pass"):
                continue
            made = state.setdefault("attempts", {})
            for number in range(1, self.attempts + 1):
                if str(number) in made:
                    continue
                outcome = None if self.stopped() else self.attempt(spec, state[DETECT], number)
                if outcome is None:
                    return False
                made[str(number)] = outcome
                self.save_state(spec["id"], state)
        return True

    def stopped(self) -> bool:
        """Whether the stage's time is up.

        Returns:
            bool: True once `until` has passed.
        """
        return self.until is not None and time.time() >= self.until

    def prepare_clone(self) -> None:
        """Makes the scratch clone the attempts run in, once: bare, every ref deleted."""
        if (self.clone / "HEAD").is_file():
            return
        partial = self.clone.with_name("repo.partial")
        shutil.rmtree(partial, ignore_errors=True)
        partial.parent.mkdir(parents=True, exist_ok=True)
        code, output = self._git(
            None, "clone", "--bare", "--quiet", "--no-hardlinks", str(self.repo), str(partial)
        )
        if code != 0:
            raise RuntimeError(f"cloning {self.repo} failed: {output.strip()[-300:]}")
        self._git(partial, "config", "gc.auto", "0")
        self._git(partial, "remote", "remove", "origin")
        _, refs = self._git(partial, "for-each-ref", "--format=%(refname)")
        for ref in refs.split():
            self._git(partial, "update-ref", "-d", ref)
        partial.rename(self.clone)

    # -- step 1: which tests a commit makes pass ------------------------------------------------

    def detect(self, spec: dict[str, Any]) -> dict[str, Any] | None:
        """Finds a task's FAIL_TO_PASS files and touched suite.

        Args:
            spec: The task (`task_spec`).

        Returns:
            dict[str, Any] | None: The grading spec (`fail_to_pass`, `pass_to_pass`, `tests`,
            `test_command`, `dropped`); None when the window closed before it was known.
        """
        runner = self._task_run(spec, 0)
        worktree = self.gen_dir / "worktrees" / f"{spec['id']}-detect"
        self._remove_worktree(worktree, None)
        worktree.parent.mkdir(parents=True, exist_ok=True)
        code, output = self._git(
            self.clone, "worktree", "add", "-q", "--detach", str(worktree), spec["parent"]
        )
        if code != 0:
            return self._dropped(f"the parent cannot be checked out: {output.strip()[-200:]}")
        try:
            tests = [
                test
                for test in spec["tests"]
                if self._git(self.clone, "cat-file", "-e", f"{spec['commit']}:{test}")[0] == 0
            ]
            if not tests:
                return self._dropped("no touched test file exists at the commit")
            self._git(worktree, "checkout", "-q", spec["commit"], "--", *tests)
            command, source = self.test_command(worktree, spec["parent"])
            if not command:
                return self._dropped(f"no test command ({source})")
            units = tests if self.takes_files(command) else [WHOLE_SUITE]
            before = self._run_units(runner, worktree, command, units, f"{spec['id']}-parent")
            if before is None:
                return None
            self._git(worktree, "checkout", "-q", "-f", "--detach", spec["commit"])
            self._git(worktree, "clean", "-fdq")
            after = self._run_units(runner, worktree, command, units, f"{spec['id']}-commit")
            if after is None:
                return None
        finally:
            self._remove_worktree(worktree, None)
        fail_to_pass = [unit for unit in units if not before[unit] and after[unit]]
        return {
            "test_command": command,
            "test_source": source,
            "tests": tests,
            "fail_to_pass": fail_to_pass,
            "pass_to_pass": [unit for unit in units if before[unit] and after[unit]],
            "parent": before,
            "commit": after,
            "dropped": None
            if fail_to_pass
            else "no touched test fails at the parent and passes at the commit",
        }

    def test_command(self, worktree: Path, parent: str) -> tuple[str | None, str]:
        """The command that decides pass or fail: `--test-cmd`, else Night Shift's detection.

        Args:
            worktree: A checkout of the task's parent.
            parent: The parent commit.

        Returns:
            tuple[str | None, str]: The command (None if nothing matched) and where it came from.
        """
        if self.test_cmd:
            return self.test_cmd, "--test-cmd"
        return NightShiftTaskRun.detect_test_command(
            worktree, None, self.settings.test, self.repo, parent
        )

    @classmethod
    def takes_files(cls, command: str) -> bool:
        """Whether one test file can be named to the command, so a file is the grading unit.

        Args:
            command: The test command.

        Returns:
            bool: True with the `{test}` placeholder or a pytest command.
        """
        return TEST_PLACEHOLDER in command or PYTEST in command

    @classmethod
    def unit_command(cls, command: str, unit: str) -> str:
        """The command line that runs one grading unit.

        Args:
            command: The test command.
            unit: A test file, or `WHOLE_SUITE`.

        Returns:
            str: The command with the file in the placeholder's place, or appended.
        """
        return cls.units_command(command, [unit])

    @classmethod
    def units_command(cls, command: str, units: Sequence[str]) -> str:
        """The command line that runs several grading units at once.

        Args:
            command: The test command.
            units: Test files, or `[WHOLE_SUITE]`.

        Returns:
            str: The command with the files in the placeholder's place, or appended.
        """
        if WHOLE_SUITE in units:
            return command.replace(TEST_PLACEHOLDER, "").strip()
        quoted = " ".join(shlex.quote(unit) for unit in units)
        if TEST_PLACEHOLDER in command:
            return command.replace(TEST_PLACEHOLDER, quoted)
        return f"{command} {quoted}"

    # -- step 3: one attempt ----------------------------------------------------------------------

    def attempt(
        self, spec: dict[str, Any], grading: dict[str, Any], number: int
    ) -> dict[str, Any] | None:
        """Runs and grades one attempt of a task.

        Args:
            spec: The task (`task_spec`).
            grading: The task's grading spec (`detect`).
            number: The attempt's number, from 1.

        Returns:
            dict[str, Any] | None: The attempt's record (`status`, `passed`, `grades`, `rollout`);
            None when the window closed or the run was stopped before it ended.
        """
        graded = list(grading["fail_to_pass"]) + list(grading["pass_to_pass"])
        runner = self._task_run(spec, number, test=self.agent_test_command(spec, grading))
        status = runner.run()
        outcome = runner.outcome or {"status": status, "result": {}}
        record: dict[str, Any] = {
            "attempt": number,
            "status": outcome["status"],
            "session": runner.session,
            "passed": False,
        }
        try:
            if outcome["status"] in UNFINISHED:
                return None
            if outcome["status"] != CANDIDATE or outcome["result"].get("timed_out"):
                return record
            self._git(runner.worktree, "checkout", "-q", spec["commit"], "--", *grading["tests"])
            grades = self._run_units(
                runner,
                runner.worktree,
                grading["test_command"],
                graded,
                f"{spec['id']}-a{number}-grade",
            )
            if grades is None:
                return None
            record["grades"] = grades
            record["passed"] = all(grades.values())
            if record["passed"]:
                record.update(self._keep_rollout(runner.session, f"{spec['id']}-a{number}"))
            return record
        finally:
            self._remove_worktree(runner.worktree, runner.branch)

    def agent_test_command(self, spec: dict[str, Any], grading: dict[str, Any]) -> str:
        """The test command the agent is told to run, and Night Shift runs after it.

        It names only the touched test files that exist at the parent: a FAIL_TO_PASS file the
        commit added is the hidden grading target, as in SWE-bench, and is neither shown nor
        run before grading. Without such a file it is the whole suite.

        Args:
            spec: The task.
            grading: The task's grading spec.

        Returns:
            str: The command line.
        """
        command = grading["test_command"]
        if WHOLE_SUITE in grading["fail_to_pass"]:
            return self.units_command(command, [WHOLE_SUITE])
        present = [
            test
            for test in grading["tests"]
            if self._git(self.clone, "cat-file", "-e", f"{spec['parent']}:{test}")[0] == 0
        ]
        return self.units_command(command, present or [WHOLE_SUITE])

    # -- step 4: rendering and the outputs ------------------------------------------------------

    def pack(self) -> dict[str, Any]:
        """Rebuilds `train.jsonl`, `holdout.jsonl`, `source.json` and the stage report from the state.

        Returns:
            dict[str, Any]: The `source.json` written.
        """
        template, tokenizer_path, digests = self.served_assets()
        from tokenizers import Tokenizer

        tokenizer = Tokenizer.from_file(str(tokenizer_path))
        written: dict[str, list[dict[str, Any]]] = {"train": [], "holdout": []}
        dropped: list[dict[str, Any]] = []
        for spec, split in self.tasks:
            state = self.load_state(spec["id"])
            grading = state.get("detect") or {}
            if not grading.get("fail_to_pass"):
                reason = grading.get("dropped") or "not detected yet"
                dropped.append(
                    {"task": spec["id"], "split": split, "stage": "detect", "reason": reason}
                )
                continue
            record = self._record(spec, split, grading, state.get("attempts") or {})
            rendered, problem = self._render_best(state.get("attempts") or {}, template, tokenizer)
            if rendered is not None:
                record.update(rendered)
            elif split == TRAIN:
                dropped.append(
                    {"task": spec["id"], "split": split, "stage": "attempts", "reason": problem}
                )
                continue
            else:
                record["unrendered"] = problem
            written[split].append(record)
        for split, records in written.items():
            self._write_atomically(
                self.out_dir / f"{split}.jsonl",
                "".join(json.dumps(record) + "\n" for record in records),
            )
        source = self._source(written, dropped, digests)
        self._write_atomically(self.out_dir / "source.json", json.dumps(source, indent=2) + "\n")
        self._write_atomically(
            self.gen_dir / "report.json", json.dumps({"dropped": dropped}, indent=2) + "\n"
        )
        return source

    def served_assets(self) -> tuple[str, Path, dict[str, str]]:
        """The chat template the served model renders with, its tokenizer, and their digests.

        The template is the snapshot's own with the chat-template patches ragon-serve applies
        itself (the same pair the SGLang recipe carries, `ragon-engine/src/tokenizer`).

        Returns:
            tuple[str, Path, dict[str, str]]: The template, the `tokenizer.json`, and the sha256 of
            each.

        Raises:
            FileNotFoundError: When the served model's snapshot is not on this machine.
            ValueError: When a patch no longer applies to the snapshot's template.
        """
        if self.chat_template is not None and self.tokenizer is not None:
            template = self.chat_template.read_text()
            tokenizer = self.tokenizer
        else:
            snapshot = self.snapshot_dir(self.model_key)
            overrides = ModelMatrixRegistry.get_launch_overrides(
                self.model_key, TEMPLATE_RECIPE_ENGINE
            )
            template, failed = ChatTemplatePatcher.apply(
                (self.chat_template or snapshot / "chat_template.jinja").read_text(),
                overrides.get("chat_template_patches") or [],
            )
            if failed:
                raise ValueError(f"chat template patches no longer apply: {failed}")
            tokenizer = self.tokenizer or snapshot / "tokenizer.json"
        digests = {
            "chat_template_sha256": hashlib.sha256(template.encode()).hexdigest(),
            "tokenizer_sha256": hashlib.sha256(Path(tokenizer).read_bytes()).hexdigest(),
        }
        return template, Path(tokenizer), digests

    @classmethod
    def snapshot_dir(cls, model_key: str) -> Path:
        """The served model's snapshot in the Hugging Face cache, at the recipe's revision.

        Args:
            model_key: The model's registry key.

        Returns:
            Path: The snapshot folder.

        Raises:
            FileNotFoundError: When it is not downloaded.
        """
        repo_id = ModelMatrixRegistry.resolve_hf_repo(model_key)
        revision = ModelMatrixRegistry.get_launch_overrides(model_key, TEMPLATE_RECIPE_ENGINE).get(
            "revision"
        )
        snapshots = (
            ModelDownloader.get_hf_cache_dir()
            / f"models--{repo_id.replace('/', '--')}"
            / "snapshots"
        )
        candidates = [snapshots / revision] if revision else sorted(snapshots.glob("*"))
        for folder in candidates:
            if (folder / "chat_template.jinja").is_file() and (folder / "tokenizer.json").is_file():
                return folder
        raise FileNotFoundError(f"the served model's snapshot is not in {snapshots}")

    @classmethod
    def rollout_messages(cls, path: Path) -> tuple[list[dict[str, Any]], dict[str, Any]]:
        """A session rollout as the chat messages and template arguments the server rendered.

        Mirrors `parse_responses_request` in ragon-serve: the base instructions and every
        developer message become one leading system message, a custom tool call is the
        one-parameter function call it was offered as, the items of one assistant turn are merged,
        and tool outputs are tool messages. Arguments are objects, as the template iterates them.

        Args:
            path: The rollout (`$CODEX_HOME/sessions/…/rollout-…-<thread>.jsonl`).

        Returns:
            tuple[list[dict[str, Any]], dict[str, Any]]: The messages, and `reasoning_effort` or
            `enable_thinking` when the session set an effort.

        Raises:
            ValueError: For a compacted session, whose earlier turns the model no longer saw.
        """
        instructions: str | None = None
        effort: str | None = None
        items: list[dict[str, Any]] = []
        for line in Path(path).read_text().splitlines():
            try:
                entry = json.loads(line)
            except ValueError:
                continue
            kind, payload = entry.get("type"), entry.get("payload") or {}
            if kind == COMPACTED:
                raise ValueError("the session was compacted")
            if kind == SESSION_META:
                base = payload.get("base_instructions")
                instructions = base.get("text") if isinstance(base, dict) else base
            elif kind == TURN_CONTEXT:
                mode = (payload.get("collaboration_mode") or {}).get("settings") or {}
                effort = mode.get("reasoning_effort") or payload.get("effort") or effort
            elif kind == RESPONSE_ITEM and isinstance(payload, dict):
                items.append(payload)
        messages = [{"role": "system", "content": instructions}] if instructions else []
        messages += [message for message in map(cls.chat_message, items) if message]
        arguments: dict[str, Any] = {}
        if effort == EFFORT_NONE:
            arguments["enable_thinking"] = False
        elif isinstance(effort, str) and effort:
            arguments["reasoning_effort"] = effort
        return cls.one_system(cls.merge_turns(messages)), arguments

    @classmethod
    def chat_message(cls, item: dict[str, Any]) -> dict[str, Any] | None:
        """One Responses item as a chat message, or None to drop it (ragon-serve's `normalize_item`).

        Args:
            item: A rollout's `response_item` payload.

        Returns:
            dict[str, Any] | None: The message.
        """
        kind = item.get("type", "message")
        call_id = item.get("call_id") or item.get("id") or ""
        if kind == CUSTOM_TOOL_CALL:
            source = item.get("input")
            call = {CUSTOM_TOOL_INPUT: source if isinstance(source, str) else ""}
            return cls._tool_call(call_id, item.get("name", ""), call)
        if kind == FUNCTION_CALL:
            return cls._tool_call(
                call_id, item.get("name", ""), cls.arguments_of(item.get("arguments"))
            )
        if kind in TOOL_OUTPUTS:
            return {
                "role": "tool",
                "tool_call_id": item.get("call_id", ""),
                "content": cls.text_of(item.get("output", "")),
            }
        if kind == REASONING:
            text = cls._reasoning_text(item)
            return {"role": "assistant", "reasoning_content": text} if text else None
        if kind != MESSAGE:
            return None
        role = item.get("role", "user")
        message: dict[str, Any] = {"role": "system" if role == DEVELOPER else role}
        content = item.get("content")
        if content is not None:
            if isinstance(content, list):
                message["content"] = [
                    {
                        "type": "text",
                        "text": part.get("text") if isinstance(part.get("text"), str) else "",
                    }
                    if isinstance(part, dict) and part.get("type") in ("input_text", "output_text")
                    else part
                    for part in content
                ]
            else:
                message["content"] = content
        return message

    @classmethod
    def arguments_of(cls, raw: object) -> dict[str, Any]:
        """A function call's arguments as an object; `{}` when they are not one (as ragon-serve).

        Args:
            raw: The call's `arguments`, a JSON string as Codex records them.

        Returns:
            dict[str, Any]: The arguments.
        """
        if isinstance(raw, dict):
            return raw
        if not isinstance(raw, str):
            return {}
        try:
            parsed = json.loads(raw)
        except ValueError:
            return {}
        return parsed if isinstance(parsed, dict) else {}

    @classmethod
    def merge_turns(cls, messages: list[dict[str, Any]]) -> list[dict[str, Any]]:
        """Merges consecutive assistant messages into one turn, as ragon-serve does.

        Args:
            messages: The messages, one per item.

        Returns:
            list[dict[str, Any]]: The messages with each assistant turn whole.
        """
        merged: list[dict[str, Any]] = []
        for message in messages:
            if message["role"] != ASSISTANT or not merged or merged[-1]["role"] != ASSISTANT:
                merged.append(dict(message))
                continue
            previous = merged[-1]
            if not cls._empty(message):
                if cls._empty(previous):
                    previous["content"] = message["content"]
                elif isinstance(previous["content"], str) and isinstance(message["content"], str):
                    previous["content"] = f"{previous['content']}\n\n{message['content']}"
                else:
                    previous["content"] = cls._parts(previous["content"]) + cls._parts(
                        message["content"]
                    )
            if TOOL_CALLS in message:
                previous["tool_calls"] = (
                    list(previous.get("tool_calls") or []) + message["tool_calls"]
                )
            if REASONING_CONTENT in message:
                earlier = previous.get("reasoning_content")
                previous["reasoning_content"] = (
                    f"{earlier}\n{message['reasoning_content']}"
                    if earlier
                    else message["reasoning_content"]
                )
        return merged

    @classmethod
    def one_system(cls, messages: list[dict[str, Any]]) -> list[dict[str, Any]]:
        """Joins every system chunk into one leading system message, blank-line separated.

        Args:
            messages: The merged messages.

        Returns:
            list[dict[str, Any]]: The messages the template is given.
        """
        chunks: list[str] = []
        others: list[dict[str, Any]] = []
        for message in messages:
            if message["role"] != SYSTEM:
                others.append(message)
                continue
            content = message.get("content")
            if isinstance(content, str):
                chunks.append(content)
            elif isinstance(content, list):
                chunks += [
                    part["text"]
                    for part in content
                    if isinstance(part, dict) and isinstance(part.get("text"), str)
                ]
        return ([{"role": "system", "content": "\n\n".join(chunks)}] if chunks else []) + others

    @classmethod
    def render(
        cls,
        messages: list[dict[str, Any]],
        template: str,
        tokenizer: Any,  # noqa: ANN401 - a `tokenizers.Tokenizer`, imported only when rendering
        arguments: dict[str, Any] | None = None,
    ) -> dict[str, Any]:
        """Renders a conversation in the chat template, tokenized, with the assistant's tokens masked.

        An assistant turn is the text its conversation adds after the generation prompt, up to and
        including its end-of-turn token: what the model wrote. Each stretch is tokenized on its own,
        so a prompt token never straddles the boundary into the model's output.

        Args:
            messages: The chat messages (`rollout_messages`).
            template: The served chat template.
            tokenizer: The served tokenizer.
            arguments: Extra template arguments (`reasoning_effort`, `enable_thinking`).

        Returns:
            dict[str, Any]: `text`, `input_ids`, `assistant_mask` (1 for the model's tokens) and
            `tokens`.

        Raises:
            ValueError: When the template does not render the conversation as a growing prefix,
                or the conversation has no assistant turn.
        """
        extra = arguments or {}
        full = cls.render_text(messages, template, False, extra)
        spans: list[tuple[int, int]] = []
        for index, message in enumerate(messages):
            if message["role"] != ASSISTANT:
                continue
            prompt = cls.render_text(messages[:index], template, True, extra)
            through = cls.render_text(messages[: index + 1], template, False, extra)
            if not through.startswith(prompt) or not full.startswith(through):
                raise ValueError(
                    "the template does not render the conversation as a growing prefix"
                )
            end = len(through)
            while end > len(prompt) and through[end - 1] == NEWLINE:
                end -= 1
            spans.append((len(prompt), end))
        if not spans:
            raise ValueError("the conversation has no assistant turn")
        ids: list[int] = []
        mask: list[int] = []
        position = 0
        for start, end in [*spans, (len(full), len(full))]:
            for text, flag in ((full[position:start], 0), (full[start:end], 1)):
                if text:
                    piece = tokenizer.encode(text, add_special_tokens=False).ids
                    ids += piece
                    mask += [flag] * len(piece)
            position = end
        if position < len(full):
            piece = tokenizer.encode(full[position:], add_special_tokens=False).ids
            ids += piece
            mask += [0] * len(piece)
        return {"text": full, "input_ids": ids, "assistant_mask": mask, "tokens": len(ids)}

    @classmethod
    def render_text(
        cls,
        messages: list[dict[str, Any]],
        template: str,
        add_generation_prompt: bool,
        arguments: dict[str, Any],
    ) -> str:
        """Renders messages with the chat template, as `apply_chat_template(tokenize=False)` does.

        Args:
            messages: The messages.
            template: The chat template.
            add_generation_prompt: Whether to end with the assistant's generation prompt.
            arguments: Extra template arguments.

        Returns:
            str: The rendered text.
        """
        from transformers.utils.chat_template_utils import render_jinja_template

        rendered: Any = render_jinja_template(
            conversations=[messages],
            chat_template=template,
            add_generation_prompt=add_generation_prompt,
            **arguments,
        )
        if isinstance(rendered, tuple):
            rendered = rendered[0]
        return rendered[0] if isinstance(rendered, list) else str(rendered)

    # -- task records and state ---------------------------------------------------------------

    @classmethod
    def task_spec(cls, task: object) -> dict[str, Any]:
        """A task as a plain record, from a dict or an object with its fields.

        Args:
            task: The task.

        Returns:
            dict[str, Any]: `id`, `commit`, `parent`, `text`, `tests`, `committed_at`.
        """
        field: Callable[[str], Any] = (
            task.get if isinstance(task, dict) else (lambda name: getattr(task, name, None))
        )
        commit = str(field("commit"))
        return {
            "id": commit[:12],
            "commit": commit,
            "parent": str(field("parent")),
            "text": str(field("message") or field("text") or ""),
            "tests": [str(test) for test in field("tests") or ()],
            "committed_at": field("committed_at"),
        }

    def load_state(self, task_id: str) -> dict[str, Any]:
        """A task's state, or an empty one.

        Args:
            task_id: The task's id.

        Returns:
            dict[str, Any]: The state.
        """
        path = self.gen_dir / "state" / f"{task_id}.json"
        try:
            state = json.loads(path.read_text())
        except (OSError, ValueError):
            return {"version": STATE_VERSION}
        return state if isinstance(state, dict) else {"version": STATE_VERSION}

    def save_state(self, task_id: str, state: dict[str, Any]) -> None:
        """Writes a task's state atomically.

        Args:
            task_id: The task's id.
            state: The state.
        """
        self._write_atomically(
            self.gen_dir / "state" / f"{task_id}.json", json.dumps(state, indent=2)
        )

    # -- helpers ------------------------------------------------------------------------------

    def _task_run(
        self, spec: dict[str, Any], number: int, test: str | None = None
    ) -> NightShiftTaskRun:
        record = {
            "id": spec["id"],
            "repo": str(self.clone),
            "base": spec["parent"],
            "branch": f"adapter-train/{spec['id']}",
            "task": spec["text"],
            "test": test,
            "status": "running",
            "history": [],
            "attempts": 0,
            "nudges": 0,
            "session": None,
            "result": None,
        }
        tasks = self.gen_dir / "tasks"
        tasks.mkdir(parents=True, exist_ok=True)
        if not (tasks / f"{spec['id']}.json").is_file():
            self._write_atomically(tasks / f"{spec['id']}.json", json.dumps(record))
        deadline = time.time() + self.settings.task_timeout_s
        if self.until is not None:
            deadline = min(deadline, self.until)
        return NightShiftTaskRun(
            self.gen_dir,
            record,
            self.settings,
            self.ragon_bin,
            deadline,
            model_host=self.model_host,
            window_end=self.until,
            attempt=number,
        )

    def _run_units(
        self, runner: NightShiftTaskRun, worktree: Path, command: str, units: list[str], name: str
    ) -> dict[str, bool] | None:
        runner.worktree = worktree
        results: dict[str, bool] = {}
        for index, unit in enumerate(units):
            runner.test_log_path = self.gen_dir / "logs" / f"{name}-{index}.test.log"
            runner.test_log_path.parent.mkdir(parents=True, exist_ok=True)
            result = runner._run_tests(self.unit_command(command, unit))
            if result.get("refused"):
                raise RuntimeError(NightShiftTaskRun.REFUSED_UNSEALED)
            if result.get("timed_out") == WINDOW_CLOSED:
                return None
            results[unit] = result.get("exit_code") == 0
        return results

    def _keep_rollout(self, session: str | None, name: str) -> dict[str, Any]:
        found = self._find_rollout(session) if session else None
        if found is None:
            return {"rollout": None}
        kept = self.gen_dir / "rollouts" / f"{name}.jsonl"
        kept.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(found, kept)
        items = sum(1 for line in kept.read_text().splitlines() if self.is_item(line))
        return {"rollout": str(kept.relative_to(self.out_dir)), "items": items}

    @classmethod
    def is_item(cls, line: str) -> bool:
        """Whether a rollout line is a conversation item (`response_item`).

        Args:
            line: One line of a rollout.

        Returns:
            bool: True for an item.
        """
        try:
            entry = json.loads(line)
        except ValueError:
            return False
        return isinstance(entry, dict) and entry.get("type") == RESPONSE_ITEM

    @classmethod
    def _find_rollout(cls, session: str) -> Path | None:
        sessions = Path(os.environ.get("CODEX_HOME") or Path.home() / ".ragon") / "sessions"
        today = datetime.now()
        for day in (today, today - timedelta(days=1)):
            folder = sessions / day.strftime("%Y/%m/%d")
            found = sorted(folder.glob(f"rollout-*-{session}.jsonl"))
            if found:
                return found[-1]
        found = sorted(sessions.rglob(f"rollout-*-{session}.jsonl")) if sessions.is_dir() else []
        return found[-1] if found else None

    def _render_best(
        self,
        attempts: dict[str, Any],
        template: str,
        tokenizer: Any,  # noqa: ANN401
    ) -> tuple[dict[str, Any] | None, str]:
        passing = sorted(
            (
                record
                for record in attempts.values()
                if record.get("passed") and record.get("rollout")
            ),
            key=lambda record: (record.get("items", 0), record["attempt"]),
        )
        if not passing:
            return None, "no attempt passed"
        problems = []
        for record in passing:
            try:
                messages, arguments = self.rollout_messages(self.out_dir / record["rollout"])
                rendered = self.render(messages, template, tokenizer, arguments)
            except Exception as error:  # noqa: BLE001 - a template's raise or a broken rollout: try the next one
                problems.append(f"attempt {record['attempt']}: {error}")
                continue
            rendered.update({"attempt": record["attempt"], "tools": ABSENT, "reasoning": ABSENT})
            return rendered, ""
        return None, "; ".join(problems)

    def _record(
        self, spec: dict[str, Any], split: str, grading: dict[str, Any], attempts: dict[str, Any]
    ) -> dict[str, Any]:
        return {
            "task": spec["id"],
            "commit": spec["commit"],
            "parent": spec["parent"],
            "split": split,
            "prompt": spec["text"],
            "test_command": grading["test_command"],
            "tests": grading["tests"],
            "fail_to_pass": grading["fail_to_pass"],
            "pass_to_pass": grading["pass_to_pass"],
            "resolved": sum(1 for record in attempts.values() if record.get("passed")),
            "attempts": len(attempts),
        }

    def _source(
        self,
        written: dict[str, list[dict[str, Any]]],
        dropped: list[dict[str, Any]],
        digests: dict[str, str],
    ) -> dict[str, Any]:
        specs = [spec for spec, _ in self.tasks]
        dated = sorted(specs, key=lambda spec: str(spec.get("committed_at") or ""))
        return {
            "version": STATE_VERSION,
            "repository": self.repo.name,
            "commits": {
                "first": dated[0]["commit"] if dated else None,
                "last": dated[-1]["commit"] if dated else None,
            },
            "counts": {
                "candidates": {
                    split: sum(1 for _, s in self.tasks if s == split)
                    for split in ("train", "holdout")
                },
                "gradable": {
                    split: sum(
                        1
                        for spec, s in self.tasks
                        if s == split
                        and (self.load_state(spec["id"]).get("detect") or {}).get("fail_to_pass")
                    )
                    for split in ("train", "holdout")
                },
                "train": len(written["train"]),
                "holdout": len(written["holdout"]),
                "holdout_resolved": sum(1 for record in written["holdout"] if record["resolved"]),
                "dropped": len(dropped),
            },
            "k": self.attempts,
            "grading_unit": "test file",
            "test_command": self.test_cmd,
            "model": self.model_key,
            **digests,
            "tools": ABSENT,
            "reasoning": ABSENT,
        }

    def _summary(self, status: str, reason: str | None = None) -> dict[str, Any]:
        detected = [self.load_state(spec["id"]).get("detect") for spec, _ in self.tasks]
        summary: dict[str, Any] = {
            "status": status,
            "tasks": len(self.tasks),
            "detected": sum(1 for found in detected if found),
            "gradable": sum(1 for found in detected if found and found.get("fail_to_pass")),
        }
        if reason:
            summary["reason"] = reason
            dropped = [
                {
                    "task": spec["id"],
                    "split": split,
                    "stage": "detect",
                    "reason": found.get("dropped"),
                }
                for (spec, split), found in zip(self.tasks, detected, strict=True)
                if found and not found.get("fail_to_pass")
            ]
            self._write_atomically(
                self.gen_dir / "report.json",
                json.dumps({"refused": reason, "dropped": dropped}, indent=2) + "\n",
            )
        return summary

    @classmethod
    def _dropped(cls, reason: str) -> dict[str, Any]:
        return {
            "fail_to_pass": [],
            "pass_to_pass": [],
            "tests": [],
            "test_command": None,
            "dropped": reason,
        }

    def _remove_worktree(self, worktree: Path, branch: str | None) -> None:
        if worktree.is_dir():
            self._git(self.clone, "worktree", "remove", "--force", str(worktree))
            shutil.rmtree(worktree, ignore_errors=True)
        self._git(self.clone, "worktree", "prune")
        if branch:
            self._git(self.clone, "branch", "-D", branch)

    @classmethod
    def _git(cls, cwd: Path | None, *args: str) -> tuple[int, str]:
        # Worktrees are the agent's to write: no config-named program and no hook runs (SafeGit).
        result = SafeGit.run([*NO_HOOKS, *args], cwd)
        return result.returncode, result.stdout + result.stderr

    @classmethod
    def _tool_call(cls, call_id: str, name: str, arguments: dict[str, Any]) -> dict[str, Any]:
        return {
            "role": "assistant",
            "tool_calls": [
                {
                    "id": call_id,
                    "type": "function",
                    "function": {"name": name, "arguments": arguments},
                }
            ],
        }

    @classmethod
    def text_of(cls, parts: object) -> str:
        """The text of a string or a list of content parts, concatenated.

        Args:
            parts: A string or a list of parts.

        Returns:
            str: The text.
        """
        if isinstance(parts, str):
            return parts
        if not isinstance(parts, list):
            return ""
        return "".join(
            part["text"]
            for part in parts
            if isinstance(part, dict) and isinstance(part.get("text"), str)
        )

    @classmethod
    def _reasoning_text(cls, item: dict[str, Any]) -> str:
        for key in ("summary", "content"):
            texts = [
                entry["text"]
                for entry in item.get(key) or []
                if isinstance(entry, dict) and isinstance(entry.get("text"), str) and entry["text"]
            ]
            if texts:
                return "\n".join(texts)
        return ""

    @classmethod
    def _empty(cls, message: dict[str, Any]) -> bool:
        content = message.get("content")
        return content is None or content == ""

    @classmethod
    def _parts(cls, content: object) -> list[Any]:
        if isinstance(content, list):
            return list(content)
        if isinstance(content, str) and content:
            return [{"type": "text", "text": content}]
        return []

    @classmethod
    def _write_atomically(cls, path: Path, text: str) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        partial = path.with_name(f"{path.name}.partial")
        partial.write_text(text)
        partial.replace(path)
