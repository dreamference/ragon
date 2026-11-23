"""Tests for the generate stage of adapter training (`AdapterTrainGenerate`).

The agent is a stand-in (the `FAKE_RAGON` pattern of test_night_shift.py): it writes a change and a
session rollout as `ragon exec` does, and never reaches a model. The chat template and the tokenizer
are small ones written here, shaped as the served model's.
"""

from __future__ import annotations

import json
import subprocess
import sys
import textwrap
import time
from pathlib import Path
from typing import Final

import pytest

from dreamference.hardware.adapter_train_generate import WHOLE_SUITE, AdapterTrainGenerate
from dreamference.night_shift.night_shift_settings import NightShiftSettings
from dreamference.night_shift.night_shift_task_run import NightShiftTaskRun

MODEL_HOST = "http://127.0.0.1:18999"

# The arguments of a call, as a dict, for the `arguments_of` cases: the JSON string, the dict
# already parsed, and a value that is neither (`arguments_of` returns `{}` for it, as ragon-serve).
ARGUMENTS_JSON: Final[str] = json.dumps({"cmd": "ls"})
ARGUMENTS_DICT: Final[dict] = {"cmd": "ls"}
ARGUMENTS_NONE: Final[None] = None

# A stand-in for `ragon exec`. Each call takes the next behaviour of $FAKE_PLAN (`good` fixes the
# bug, `bad` writes a wrong fix, `none` changes nothing), writes a rollout under
# $CODEX_HOME/sessions as Codex does, and logs its arguments and the variables it was given.
FAKE_RAGON = textwrap.dedent("""\
    #!{python}
    import datetime, json, os, sys, time, uuid
    args = sys.argv[1:]
    if args[0] == "sandbox":
        command = args[args.index("--") + 1:]
        os.execvp(command[0], command)
    calls = os.environ["FAKE_RAGON_CALLS"]
    with open(calls, "a") as log:
        log.write(json.dumps({{"args": args, "env": {{name: os.environ.get(name) for name in
            ("DREAMFERENCE_VLLM_HOST", "DREAMFERENCE_RAGON_ADAPTER")}}}}) + "\\n")
    count = sum(1 for _ in open(calls))
    plan = os.environ.get("FAKE_PLAN", "good").split(",")
    behaviour = plan[min(count, len(plan)) - 1]
    cwd = args[args.index("-C") + 1]
    out = args[args.index("-o") + 1]
    session = str(uuid.uuid4())
    print(json.dumps({{"type": "thread.started", "thread_id": session}}), flush=True)
    if behaviour == "hang":
        time.sleep(30)
    body = {{"good": "a + b", "bad": "a * b"}}.get(behaviour)
    if body:
        with open(os.path.join(cwd, "calc.py"), "w") as handle:
            handle.write(f"def add(a, b):\\n    return {{body}}\\n")
    home = os.environ.get("CODEX_HOME") or os.path.join(os.environ["HOME"], ".ragon")
    now = datetime.datetime.now()
    folder = os.path.join(home, "sessions", now.strftime("%Y/%m/%d"))
    os.makedirs(folder, exist_ok=True)
    items = [
        {{"type": "message", "role": "developer",
          "content": [{{"type": "input_text", "text": "Sandbox: workspace-write."}}]}},
        {{"type": "message", "role": "user", "content": [{{"type": "input_text", "text": args[-1]}}]}},
        {{"type": "function_call", "name": "exec_command", "call_id": "c1",
          "arguments": json.dumps({{"cmd": "cat calc.py"}})}},
        {{"type": "function_call_output", "call_id": "c1", "output": "def add(a, b):\\n    return a - b\\n"}},
        {{"type": "custom_tool_call", "name": "apply_patch", "call_id": "c2", "input": "*** Begin Patch"}},
        {{"type": "custom_tool_call_output", "call_id": "c2", "output": [{{"type": "input_text", "text": "Done."}}]}},
        {{"type": "message", "role": "assistant", "content": [{{"type": "output_text", "text": "Fixed add."}}]}},
    ][: 7 if behaviour != "none" else 2]
    meta = {{"id": session, "base_instructions": {{"text": "You are ragon."}}}}
    mode = {{"collaboration_mode": {{"settings": {{"reasoning_effort": "high"}}}}}}
    lines = [{{"type": "session_meta", "payload": meta}}, {{"type": "turn_context", "payload": mode}}]
    lines += [{{"type": "response_item", "payload": item}} for item in items]
    with open(os.path.join(folder, f"rollout-{{now:%Y-%m-%dT%H-%M-%S}}-{{session}}.jsonl"), "w") as handle:
        handle.write("".join(json.dumps(line) + "\\n" for line in lines))
    with open(out, "w") as handle:
        handle.write("Fixed add.\\n\\nFiles changed: calc.py" if body else "Nothing needed changing.")
    print(json.dumps({{"type": "turn.completed"}}), flush=True)
""")

# Shaped as the served Qwen template: a system message first, `<think>` blocks, tool calls whose
# arguments are iterated, and a generation prompt that the assistant's turn starts with.
TEMPLATE = (
    "{% for m in messages %}"
    "{% if m.role == 'assistant' %}"
    "<|im_start|>assistant\n<think>\n{{ m.reasoning_content or '' }}\n</think>\n\n"
    "{% if m.content is string %}{{ m.content }}"
    "{% elif m.content %}{% for part in m.content %}{{ part.text }}{% endfor %}{% endif %}"
    "{% for call in m.tool_calls or [] %}"
    "<tool_call>{{ call.function.name }}"
    "{% for key, value in call.function.arguments|items %} {{ key }}={{ value }}{% endfor %}"
    "</tool_call>"
    "{% endfor %}<|im_end|>\n"
    "{% else %}"
    "<|im_start|>{{ m.role }}\n"
    "{% if m.content is string %}{{ m.content }}"
    "{% else %}{% for part in m.content %}{{ part.text }}{% endfor %}{% endif %}<|im_end|>\n"
    "{% endif %}"
    "{% endfor %}"
    "{% if add_generation_prompt %}<|im_start|>assistant\n<think>\n{% endif %}"
)


def git(cwd: Path, *args: str) -> str:
    return subprocess.run(
        ["git", "-C", str(cwd), *args], capture_output=True, text=True, check=True
    ).stdout.strip()


def commit(repo: Path, message: str, files: dict[str, str]) -> str:
    for name, text in files.items():
        (repo / name).parent.mkdir(parents=True, exist_ok=True)
        (repo / name).write_text(text)
    git(repo, "add", "-A")
    git(repo, "commit", "-q", "-m", message)
    return git(repo, "rev-parse", "HEAD")


def byte_tokenizer(path: Path) -> Path:
    """A byte-level tokenizer with one token per byte: enough to check the mask's spans."""
    from tokenizers import Tokenizer, decoders, models, pre_tokenizers

    alphabet = pre_tokenizers.ByteLevel.alphabet()
    tokenizer = Tokenizer(
        models.BPE(vocab={char: i for i, char in enumerate(sorted(alphabet))}, merges=[])
    )
    tokenizer.pre_tokenizer = pre_tokenizers.ByteLevel(add_prefix_space=False)
    tokenizer.decoder = decoders.ByteLevel()
    tokenizer.save(str(path))
    return path


ADD_TEST = "import sys\nsys.path.insert(0, '.')\nfrom calc import add\nassert add(2, 3) == 5\n"
OTHER_TEST = "import sys\nsys.path.insert(0, '.')\nimport other\nassert other.VALUE == 1\n"


@pytest.fixture
def setup(tmp_path, monkeypatch):
    """A repository whose second commit fixes a bug its test shows, and the stand-in agent."""
    monkeypatch.setattr(NightShiftTaskRun, "USE_SCOPE", False)
    repo = tmp_path / "repo"
    repo.mkdir()
    git(repo, "init", "-q")
    git(repo, "config", "user.email", "train@test")
    git(repo, "config", "user.name", "Train Test")
    base = commit(
        repo,
        "Start",
        {
            "calc.py": "def add(a, b):\n    return a - b\n",
            "other.py": "VALUE = 1\n",
            "tests/test_other.py": OTHER_TEST,
        },
    )
    fix = commit(
        repo,
        "Fix add",
        {
            "calc.py": "def add(a, b):\n    return a + b\n",
            "tests/test_add.py": ADD_TEST,
            "tests/test_other.py": OTHER_TEST + "assert other.VALUE > 0\n",
        },
    )
    docs = commit(
        repo, "Docs", {"README.md": "calc\n", "tests/test_other.py": OTHER_TEST + "# docs\n"}
    )
    fake = tmp_path / "fake-ragon"
    fake.write_text(FAKE_RAGON.format(python=sys.executable))
    fake.chmod(0o755)
    calls = tmp_path / "calls.jsonl"
    monkeypatch.setenv("FAKE_RAGON_CALLS", str(calls))
    template = tmp_path / "chat_template.jinja"
    template.write_text(TEMPLATE)
    tasks = {
        "fix": {
            "commit": fix,
            "parent": base,
            "message": "Fix add",
            "tests": ["tests/test_add.py", "tests/test_other.py"],
        },
        "docs": {
            "commit": docs,
            "parent": fix,
            "message": "Docs",
            "tests": ["tests/test_other.py"],
        },
    }
    return {
        "repo": repo,
        "out": tmp_path / "out",
        "fake": str(fake),
        "calls": calls,
        "tasks": tasks,
        "template": template,
        "tokenizer": byte_tokenizer(tmp_path / "tokenizer.json"),
    }


def stage(setup, train, holdout=(), **kwargs) -> AdapterTrainGenerate:
    options = {
        "ragon_bin": setup["fake"],
        "test_cmd": f"{sys.executable} {{test}}",
        "min_gradable": 1,
        "settings": NightShiftSettings({}),
        "chat_template": setup["template"],
        "tokenizer": setup["tokenizer"],
        **kwargs,
    }
    return AdapterTrainGenerate(setup["repo"], setup["out"], train, holdout, MODEL_HOST, **options)


def calls(setup) -> list[dict]:
    path = setup["calls"]
    return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []


def lines(path: Path) -> list[dict]:
    return [json.loads(line) for line in path.read_text().splitlines()]


def test_detection_finds_fail_to_pass_and_the_touched_suite(setup):
    generate = stage(setup, [setup["tasks"]["fix"]])
    generate.prepare_clone()
    found = generate.detect(AdapterTrainGenerate.task_spec(setup["tasks"]["fix"]))
    assert found["fail_to_pass"] == ["tests/test_add.py"], found
    assert found["pass_to_pass"] == ["tests/test_other.py"]
    assert found["dropped"] is None
    # The checkout the attempts start from is a scratch clone without a single ref.
    assert git(generate.clone, "for-each-ref") == ""
    assert not list((generate.gen_dir / "worktrees").iterdir())


def test_a_task_without_fail_to_pass_is_dropped_with_its_reason(setup):
    generate = stage(setup, [setup["tasks"]["docs"]])
    generate.prepare_clone()
    found = generate.detect(AdapterTrainGenerate.task_spec(setup["tasks"]["docs"]))
    assert found["fail_to_pass"] == []
    assert "fails at the parent" in found["dropped"]


def test_best_of_two_grades_on_fail_to_pass_and_renders_the_passing_rollout(setup, monkeypatch):
    monkeypatch.setenv("FAKE_PLAN", "bad,good")
    result = stage(setup, [setup["tasks"]["fix"], setup["tasks"]["docs"]]).run()
    assert result["status"] == "done"
    made = calls(setup)
    assert len(made) == 2  # k = 2 for the gradable task, none for the dropped one
    for call in made:
        assert call["args"][0] == "exec"
        assert call["env"]["DREAMFERENCE_VLLM_HOST"] == MODEL_HOST
        assert call["env"]["DREAMFERENCE_RAGON_ADAPTER"] == ""  # the base, never an adapter
    state = json.loads(
        (
            setup["out"] / "generate/state" / f"{setup['tasks']['fix']['commit'][:12]}.json"
        ).read_text()
    )
    assert state["attempts"]["1"]["passed"] is False
    assert state["attempts"]["1"]["grades"]["tests/test_add.py"] is False
    assert state["attempts"]["2"]["passed"] is True
    train = lines(setup["out"] / "train.jsonl")
    assert len(train) == 1
    record = train[0]
    assert record["attempt"] == 2
    assert record["fail_to_pass"] == ["tests/test_add.py"]
    assert record["tools"] == "absent" and record["reasoning"] == "absent"
    assert len(record["input_ids"]) == len(record["assistant_mask"]) == record["tokens"]
    # The masked bytes are exactly what the model wrote: its turns up to and including <|im_end|>.
    text = record["text"]
    masked = bytes(
        byte for byte, flag in zip(text.encode(), record["assistant_mask"], strict=True) if flag
    ).decode()
    assert masked == (
        "\n</think>\n\n<tool_call>exec_command cmd=cat calc.py</tool_call><|im_end|>"
        "\n</think>\n\n<tool_call>apply_patch input=*** Begin Patch</tool_call><|im_end|>"
        "\n</think>\n\nFixed add.<|im_end|>"
    )
    assert text.startswith(
        "<|im_start|>system\nYou are ragon.\n\nSandbox: workspace-write.<|im_end|>"
    )
    source = json.loads((setup["out"] / "source.json").read_text())
    assert source["k"] == 2
    assert source["counts"]["train"] == 1
    assert source["counts"]["gradable"] == {"train": 1, "holdout": 0}
    assert source["counts"]["dropped"] == 1
    assert source["tools"] == "absent"
    assert len(source["chat_template_sha256"]) == 64
    # The person's checkout gained no branch and no worktree.
    assert git(setup["repo"], "branch", "--format=%(refname:short)").split() in (
        ["main"],
        ["master"],
    )
    assert len(git(setup["repo"], "worktree", "list").splitlines()) == 1


def test_no_passing_attempt_means_no_training_record(setup, monkeypatch):
    monkeypatch.setenv("FAKE_PLAN", "bad,none")
    stage(setup, [setup["tasks"]["fix"]]).run()
    assert lines(setup["out"] / "train.jsonl") == []
    report = json.loads((setup["out"] / "generate/report.json").read_text())
    assert report["dropped"] == [
        {
            "task": setup["tasks"]["fix"]["commit"][:12],
            "split": "train",
            "stage": "attempts",
            "reason": "no attempt passed",
        }
    ]


def test_holdout_tasks_are_graded_and_kept_with_their_grading(setup, monkeypatch):
    monkeypatch.setenv("FAKE_PLAN", "bad,bad")
    fix = setup["tasks"]["fix"]
    stage(setup, [], [fix], min_gradable=0).run()
    holdout = lines(setup["out"] / "holdout.jsonl")
    assert len(holdout) == 1
    assert holdout[0]["resolved"] == 0
    assert holdout[0]["fail_to_pass"] == ["tests/test_add.py"]
    assert "input_ids" not in holdout[0]


def test_too_few_gradable_tasks_are_refused_before_any_attempt(setup):
    result = stage(setup, [setup["tasks"]["fix"], setup["tasks"]["docs"]], min_gradable=2).run()
    assert result["status"] == "refused"
    assert "1 gradable training tasks" in result["reason"]
    assert calls(setup) == []
    report = json.loads((setup["out"] / "generate/report.json").read_text())
    assert report["dropped"][0]["task"] == setup["tasks"]["docs"]["commit"][:12]
    assert not (setup["out"] / "train.jsonl").exists()


def test_a_stopped_run_resumes_where_it_left_off(setup, monkeypatch):
    monkeypatch.setenv("FAKE_PLAN", "bad,good")
    generate = stage(setup, [setup["tasks"]["fix"]], attempts=1)
    assert generate.run()["status"] == "done"
    assert len(calls(setup)) == 1
    # Raising k runs only the attempt not yet made; detection is not repeated.
    detect_log = sorted((setup["out"] / "generate/logs").glob("*-parent-*.test.log"))
    again = stage(setup, [setup["tasks"]["fix"]], attempts=2)
    assert again.run()["status"] == "done"
    assert len(calls(setup)) == 2
    assert sorted((setup["out"] / "generate/logs").glob("*-parent-*.test.log")) == detect_log
    assert lines(setup["out"] / "train.jsonl")[0]["attempt"] == 2
    # A run whose window has closed does nothing and says so.
    assert stage(setup, [setup["tasks"]["fix"]], attempts=3, until=1.0).run()["status"] == "stopped"
    assert len(calls(setup)) == 2


def test_a_window_closing_mid_attempt_stops_and_the_next_run_redoes_the_attempt(setup, monkeypatch):
    monkeypatch.setenv("FAKE_PLAN", "hang,good,good")
    fix = setup["tasks"]["fix"]
    # Detection first, without a window, so the window below measures the attempt alone.
    first = stage(setup, [fix])
    first.prepare_clone()
    assert first.detect_all()
    result = stage(setup, [fix], until=time.time() + 5).run()
    assert result["status"] == "stopped"
    state = json.loads((setup["out"] / "generate/state" / f"{fix['commit'][:12]}.json").read_text())
    assert state["detect"]["fail_to_pass"] == ["tests/test_add.py"]
    assert state.get("attempts", {}) == {}
    assert not (setup["out"] / "train.jsonl").exists()
    assert stage(setup, [fix]).run()["status"] == "done"
    assert len(calls(setup)) == 3
    assert [record["attempt"] for record in lines(setup["out"] / "train.jsonl")] == [1]


def test_adapter_train_generate_attempt_grading_window_closed_returns_none(setup, monkeypatch):
    """When the grading test run's window closes, attempt returns None (window-closed)."""
    from unittest import mock

    from dreamference.night_shift.night_shift_task_run import NightShiftTaskRun

    fix = setup["tasks"]["fix"]
    generate = stage(setup, [fix], attempts=1)
    spec = AdapterTrainGenerate.task_spec(fix)
    grading = {
        "fail_to_pass": ["tests/test_add.py"],
        "pass_to_pass": ["tests/test_other.py"],
        "tests": ["tests/test_add.py", "tests/test_other.py"],
        "test_command": f"{sys.executable} {{test}}",
    }

    def fake_run(self):
        self.outcome = {"attempt": 1, "status": "candidate", "result": {}}
        return "candidate"

    def fake_run_tests(self, test_command):
        return {"command": test_command, "timed_out": "window"}

    with (
        mock.patch.object(NightShiftTaskRun, "run", fake_run),
        mock.patch.object(NightShiftTaskRun, "_run_tests", fake_run_tests),
    ):
        record = generate.attempt(spec, grading, 1)

    assert record is None
    # The worktree was cleaned up in the finally block.
    assert not (generate.gen_dir / "worktrees" / f"{spec['id']}-a1").exists()


def test_adapter_train_generate_prepare_clone_failure_raises_runtime_error(setup):
    """When the bare clone fails, prepare_clone raises with the git output."""
    from unittest import mock

    from dreamference.runner.safe_git import SafeGit

    generate = stage(setup, [setup["tasks"]["fix"]])
    clone_failed = mock.patch.object(
        SafeGit,
        "run",
        classmethod(
            lambda cls, *a, **kw: subprocess.CompletedProcess(a, 128, "", "fatal: not a repo")
        ),
    )
    with clone_failed, pytest.raises(RuntimeError, match=r"cloning .* failed"):
        generate.prepare_clone()
    # The partial clone is not left behind.
    assert not (generate.gen_dir / "repo.partial").exists()


def test_the_agent_is_not_shown_the_hidden_tests(setup, monkeypatch):
    monkeypatch.setenv("FAKE_PLAN", "good")
    stage(setup, [setup["tasks"]["fix"]], attempts=1).run()
    prompt = calls(setup)[0]["args"][-1]
    assert "tests/test_other.py" in prompt
    assert "test_add" not in prompt


def test_the_model_host_is_required(setup):
    with pytest.raises(ValueError, match="model_host"):
        AdapterTrainGenerate(setup["repo"], setup["out"], [], [], "")


def test_unit_commands():
    assert (
        AdapterTrainGenerate.unit_command("pytest -q", "tests/a b.py") == "pytest -q 'tests/a b.py'"
    )
    assert AdapterTrainGenerate.unit_command("make test T={test}", "t.py") == "make test T=t.py"
    assert AdapterTrainGenerate.unit_command("cargo test", WHOLE_SUITE) == "cargo test"
    assert AdapterTrainGenerate.takes_files("pytest -q") and not AdapterTrainGenerate.takes_files(
        "cargo test"
    )


def test_rollout_messages_mirror_the_servers_request_conversion(tmp_path):
    rollout = tmp_path / "rollout.jsonl"
    entries = [
        {"type": "session_meta", "payload": {"base_instructions": {"text": "Base."}}},
        {
            "type": "turn_context",
            "payload": {"collaboration_mode": {"settings": {"reasoning_effort": "none"}}},
        },
        {
            "type": "response_item",
            "payload": {
                "type": "message",
                "role": "developer",
                "content": [{"type": "input_text", "text": "Dev."}],
            },
        },
        {
            "type": "response_item",
            "payload": {"type": "message", "role": "user", "content": "Do it."},
        },
        {
            "type": "response_item",
            "payload": {"type": "message", "role": "assistant", "content": "Looking."},
        },
        {
            "type": "response_item",
            "payload": {
                "type": "function_call",
                "name": "shell",
                "call_id": "a",
                "arguments": "not json",
            },
        },
        {
            "type": "response_item",
            "payload": {"type": "function_call_output", "call_id": "a", "output": "ok"},
        },
        {"type": "response_item", "payload": {"type": "web_search_call"}},
    ]
    rollout.write_text("".join(json.dumps(entry) + "\n" for entry in entries))
    messages, arguments = AdapterTrainGenerate.rollout_messages(rollout)
    assert arguments == {"enable_thinking": False}
    assert messages == [
        {"role": "system", "content": "Base.\n\nDev."},
        {"role": "user", "content": "Do it."},
        {
            "role": "assistant",
            "content": "Looking.",
            "tool_calls": [
                {"id": "a", "type": "function", "function": {"name": "shell", "arguments": {}}}
            ],
        },
        {"role": "tool", "tool_call_id": "a", "content": "ok"},
    ]
    rollout.write_text(
        rollout.read_text() + json.dumps({"type": "compacted", "payload": {}}) + "\n"
    )
    with pytest.raises(ValueError, match="compacted"):
        AdapterTrainGenerate.rollout_messages(rollout)


def test_adapter_train_generate_chat_message_reasoning_item_yields_assistant_or_none():
    """A reasoning item becomes an assistant message with its text, or is dropped when empty."""
    with_summary = {
        "type": "reasoning",
        "summary": [
            {"type": "summary_text", "text": "First step."},
            {"type": "summary_text", "text": "Second step."},
        ],
    }
    assert AdapterTrainGenerate.chat_message(with_summary) == {
        "role": "assistant",
        "reasoning_content": "First step.\nSecond step.",
    }
    with_content = {
        "type": "reasoning",
        "summary": [{"type": "summary_text", "text": ""}],
        "content": [{"type": "reasoning_text", "text": "Worked it out."}],
    }
    assert AdapterTrainGenerate.chat_message(with_content) == {
        "role": "assistant",
        "reasoning_content": "Worked it out.",
    }
    empty = {"type": "reasoning", "summary": []}
    assert AdapterTrainGenerate.chat_message(empty) is None


def test_snapshot_dir_returns_the_revision_when_named(tmp_path, monkeypatch):
    """The revision named by the sglang recipe selects the snapshot without listing the cache."""
    revision = "16e768e7d0461b0b86e565ecedd08a24eca53e9a"
    model_key = "qwen3.8-27b-minima-nvfp4-dflash2"
    cache_dir = tmp_path / "hf_cache"
    monkeypatch.setenv("HF_HUB_CACHE", str(cache_dir))

    # Build the expected snapshot layout with both marker files present.
    snapshots = cache_dir / "models--minima-ai--mnma_qwen3.8_27b_nvfp4" / "snapshots"
    pinned = snapshots / revision
    pinned.mkdir(parents=True)
    (pinned / "chat_template.jinja").write_text("template")
    (pinned / "tokenizer.json").write_text("{}")

    # Also create a different snapshot that is incomplete (no tokenizer) to verify it is skipped.
    other = snapshots / "other-revision"
    other.mkdir()
    (other / "chat_template.jinja").write_text("template")
    # tokenizer.json intentionally absent

    result = AdapterTrainGenerate.snapshot_dir(model_key)
    assert result == pinned


def test_snapshot_dir_without_revision_globs_the_cache(tmp_path, monkeypatch):
    """Without a revision in the recipe, the first complete snapshot is used."""
    from dreamference.hardware.model_matrix_registry import ModelMatrixRegistry

    model_key = "some-model"
    cache_dir = tmp_path / "hf_cache"
    monkeypatch.setenv("HF_HUB_CACHE", str(cache_dir))
    repo_id = ModelMatrixRegistry.resolve_hf_repo(model_key)
    snapshots = cache_dir / f"models--{repo_id.replace('/', '--')}" / "snapshots"
    good = snapshots / "abc123"
    good.mkdir(parents=True)
    (good / "chat_template.jinja").write_text("template")
    (good / "tokenizer.json").write_text("{}")
    result = AdapterTrainGenerate.snapshot_dir(model_key)
    assert result == good


def test_snapshot_dir_raises_when_no_complete_snapshot(tmp_path, monkeypatch):
    """A FileNotFoundError is raised when no snapshot has both marker files."""
    model_key = "qwen3.8-27b-minima-nvfp4-dflash2"
    revision = "16e768e7d0461b0b86e565ecedd08a24eca53e9a"
    cache_dir = tmp_path / "hf_cache"
    monkeypatch.setenv("HF_HUB_CACHE", str(cache_dir))

    snapshots = cache_dir / "models--minima-ai--mnma_qwen3.8_27b_nvfp4" / "snapshots"
    pinned = snapshots / revision
    pinned.mkdir(parents=True)
    # Only one of the two marker files: incomplete
    (pinned / "chat_template.jinja").write_text("template")

    with pytest.raises(FileNotFoundError, match="snapshot is not in"):
        AdapterTrainGenerate.snapshot_dir(model_key)


def test_served_assets_patches_the_served_snapshot_template_and_hashes_both(tmp_path, monkeypatch):
    """Without named assets, the recipe's patches land on the snapshot's template and each is hashed."""
    from hashlib import sha256

    from dreamference.hardware.model_matrix_registry import (
        DEFAULT_MODEL_ALIAS,
        ModelMatrixRegistry,
    )
    from dreamference.vllm_server.chat_template_patcher import ChatTemplatePatcher

    model_key = DEFAULT_MODEL_ALIAS
    overrides = ModelMatrixRegistry.get_launch_overrides(model_key, "sglang")
    revision = overrides["revision"]
    patches = overrides["chat_template_patches"]
    cache_dir = tmp_path / "hf_cache"
    monkeypatch.setenv("HF_HUB_CACHE", str(cache_dir))

    pinned = (
        cache_dir
        / f"models--{ModelMatrixRegistry.resolve_hf_repo(model_key).replace('/', '--')}"
        / "snapshots"
        / revision
    )
    pinned.mkdir(parents=True)
    template_text = "unpatched"
    for anchor, _replacement in patches:
        template_text += "\n" + anchor
    (pinned / "chat_template.jinja").write_text(template_text)
    tokenizer = pinned / "tokenizer.json"
    tokenizer.write_text("{}")

    generate = AdapterTrainGenerate(
        tmp_path / "repo",
        tmp_path / "out",
        [],
        [],
        MODEL_HOST,
        model_key=model_key,
    )
    template, tokenizer_path, digests = generate.served_assets()
    patched, failed = ChatTemplatePatcher.apply(template_text, patches)
    assert failed == []
    assert template == patched
    assert tokenizer_path == tokenizer
    assert digests == {
        "chat_template_sha256": sha256(patched.encode()).hexdigest(),
        "tokenizer_sha256": sha256(Path(tokenizer).read_bytes()).hexdigest(),
    }


def test_served_assets_with_named_template_and_tokenizer_skips_the_patches(tmp_path):
    """Named assets are used as-is, unpatched, and their bytes are hashed."""
    from hashlib import sha256

    template = tmp_path / "template.jinja"
    template.write_text("named template")
    tokenizer = tmp_path / "tokenizer.json"
    tokenizer.write_text("{}")
    generate = AdapterTrainGenerate(
        tmp_path / "repo",
        tmp_path / "out",
        [],
        [],
        MODEL_HOST,
        chat_template=template,
        tokenizer=tokenizer,
    )
    text, tokenizer_path, digests = generate.served_assets()
    assert text == "named template"
    assert tokenizer_path == tokenizer
    assert digests == {
        "chat_template_sha256": sha256(b"named template").hexdigest(),
        "tokenizer_sha256": sha256(b"{}").hexdigest(),
    }


def test_adapter_train_generate_arguments_of_parses_the_json_and_passes_the_dict_through():
    """A call's arguments: the JSON string is parsed, a dict passes through, a value that is neither becomes `{}`."""
    parsed = AdapterTrainGenerate.arguments_of(ARGUMENTS_JSON)
    assert parsed == {"cmd": "ls"}
    assert AdapterTrainGenerate.arguments_of(ARGUMENTS_DICT) is not None
    assert AdapterTrainGenerate.arguments_of(ARGUMENTS_DICT) == ARGUMENTS_DICT
    assert AdapterTrainGenerate.arguments_of(ARGUMENTS_NONE) == {}


class _ByteTokenizer:
    """One token per byte, in memory: the same shape as `byte_tokenizer`, no file round-trip."""

    def encode(self, text, add_special_tokens=False):
        return type("Ids", (), {"ids": [ord(c) for c in text]})()


RENDER_MESSAGES: Final[list[dict[str, object]]] = [
    {"role": "user", "content": "Do it."},
    {"role": "assistant", "content": "Fixed."},
    {"role": "user", "content": "Again."},
    {"role": "assistant", "content": "Again, fixed."},
]


def test_adapter_train_generate_render_masks_each_assistant_turns_tokens(tmp_path):
    """Render tokenizes the growing conversation, masking every assistant stretch's tokens."""
    template = tmp_path / "chat_template.jinja"
    template.write_text(TEMPLATE)
    result = AdapterTrainGenerate.render(RENDER_MESSAGES, template.read_text(), _ByteTokenizer())
    full = AdapterTrainGenerate.render_text(RENDER_MESSAGES, TEMPLATE, False, {})
    assert result["text"] == full
    assert result["tokens"] == len(full)
    assert len(result["input_ids"]) == len(result["assistant_mask"]) == len(full)
    mask = result["assistant_mask"]
    for message in RENDER_MESSAGES:
        if message["role"] != "assistant":
            continue
        start = full.find(message["content"])
        assert mask[start : start + len(message["content"])] == [1] * len(message["content"])
    # The leading prompt, between-turn text and the trailing turn end are the model's, not user's.
    assert mask[0] == 0
    assert any(flag == 0 for flag in mask)


def test_adapter_train_generate_render_rejects_a_template_that_is_not_a_growing_prefix():
    """A template whose prompt changes as the conversation grows cannot be a growing prefix."""
    template = "{% if add_generation_prompt %}P{% else %}F{% endif %}{% for m in messages %}{{ m.content }}{% endfor %}"
    with pytest.raises(ValueError, match="growing prefix"):
        AdapterTrainGenerate.render(
            [
                {"role": "user", "content": "A"},
                {"role": "assistant", "content": "B"},
            ],
            template,
            _ByteTokenizer(),
        )


def test_adapter_train_generate_render_rejects_a_conversation_without_an_assistant_turn():
    """Without an assistant turn there is nothing to mask."""
    with pytest.raises(ValueError, match="no assistant turn"):
        AdapterTrainGenerate.render([{"role": "user", "content": "A"}], TEMPLATE, _ByteTokenizer())
