"""The Terminal-Bench agent: runtime build, environment, command line, log counts, Harbor class.

Nothing here touches the machine: the runtime build gets a fake runner and fake packages, and the
Harbor class (tested only where `harbor` is installed) a fake environment.
"""

import asyncio
import gzip
import json
import shlex
import subprocess
from pathlib import Path

import pytest

from dreamference.terminal_bench import tb_agent_spec, tb_runtime, tb_stats


def event(kind, **fields):
    """One `ragon exec --json` line."""
    return json.dumps({"type": kind, **fields})


def command(output):
    """A finished command's event."""
    item = {"id": "i", "type": "command_execution", "command": "x", "aggregated_output": output}
    return event("item.completed", item=item)


# ---- tb_stats -----------------------------------------------------------------------------------


def test_the_counts_take_tokens_from_finished_turns_and_lengths_from_finished_commands():
    lines = [
        "Reading additional input from stdin...",
        event("item.started", item={"type": "command_execution"}),
        command("a\nb\n"),
        command("\n".join(["x"] * 61)),
        command("\n".join(["y"] * 201) + "\n"),
        event("item.completed", item={"type": "mcp_tool_call"}),
        event("turn.completed", usage={"input_tokens": 10, "cached_input_tokens": 4}),
        event("turn.completed", usage={"input_tokens": 5, "output_tokens": 7}),
    ]
    stats = tb_stats.from_lines(lines)
    assert (stats["commands"], stats["tool_calls"], stats["turns"]) == (3, 1, 2)
    assert (stats["input_tokens"], stats["cached_input_tokens"], stats["output_tokens"]) == (
        15,
        4,
        7,
    )
    assert (stats["outputs_over_60"], stats["outputs_over_200"]) == (2, 1)
    assert stats["max_lines"] == 201
    assert stats["output_lines"] == 2 + 61 + 201
    assert stats["bytes_over_200"] == len("y\n" * 201)
    assert stats["bytes_over_60"] == len("y\n" * 201) + len("\n".join(["x"] * 61))


def test_an_output_of_exactly_the_threshold_is_not_long():
    stats = tb_stats.from_lines([command("\n".join(["x"] * 60))])
    assert stats["outputs_over_60"] == 0


def test_a_missing_log_counts_nothing(tmp_path):
    assert tb_stats.from_log(tmp_path / "none.jsonl") == tb_stats.empty()


def test_the_rollouts_say_what_the_model_read_and_which_outputs_were_digests(tmp_path):
    day = tmp_path / "sessions" / "2026" / "11" / "22"
    day.mkdir(parents=True)
    digest = "Output:\n[digest] 300 lines, exit code 0. Shown: the last 20 lines\n" + "x\n" * 20
    records = [
        {"type": "response_item", "payload": {"type": "function_call_output", "output": digest}},
        {"type": "response_item", "payload": {"type": "custom_tool_call_output", "output": "ok"}},
        {"type": "response_item", "payload": {"type": "message", "content": "[digest] 9 lines"}},
    ]
    lines = [json.dumps(record) for record in records]
    (day / "rollout-1.jsonl").write_text("\n".join(["not json", *lines]) + "\n")
    stats = tb_stats.from_rollouts(tmp_path)
    assert (stats["seen_outputs"], stats["digests"]) == (2, 1)
    assert stats["seen_lines"] == 22 + 1
    assert tb_stats.from_rollouts(tmp_path / "missing") == tb_stats.seen_empty()


# ---- tb_agent_spec ------------------------------------------------------------------------------


def test_the_environment_names_the_model_server_and_keeps_it_off_any_proxy():
    env = tb_agent_spec.environment("http://gate.lan:8000", "sk-ragon-x", "corp.example,localhost")
    assert env["DREAMFERENCE_VLLM_HOST"] == "http://gate.lan:8000"
    assert env["RAGON_API_KEY"] == "sk-ragon-x"
    assert env["CODEX_HOME"].startswith(tb_agent_spec.AGENT_LOG_DIR)
    assert env["NO_PROXY"] == env["no_proxy"] == "corp.example,localhost,127.0.0.1,::1,gate.lan"
    assert env["DREAMFERENCE_DIGEST_INSIDE"] == "0"
    assert "HOME" not in env and "PATH" not in env  # the container's own


def test_no_key_means_no_key_variable_and_digest_on_is_passed():
    env = tb_agent_spec.environment("http://gate.lan:8000", None, digest=True)
    assert "RAGON_API_KEY" not in env
    assert env["DREAMFERENCE_DIGEST_INSIDE"] == "1"


def test_the_key_comes_from_the_environment_first_then_the_file(tmp_path, monkeypatch):
    key_file = tmp_path / "service-key"
    key_file.write_text("sk-ragon-file\n")
    monkeypatch.delenv("RAGON_API_KEY", raising=False)
    assert tb_agent_spec.service_key(key_file) == "sk-ragon-file"
    monkeypatch.setenv("RAGON_API_KEY", "sk-ragon-env")
    assert tb_agent_spec.service_key(key_file) == "sk-ragon-env"
    monkeypatch.delenv("RAGON_API_KEY")
    assert tb_agent_spec.service_key(tmp_path / "missing") is None


def test_the_command_runs_unsandboxed_in_the_working_directory_and_logs_events():
    line = tb_agent_spec.exec_command("Fix it; don't `rm` $HOME", task_context=1000)
    words = shlex.split(line)
    assert words[words.index("&&") + 1] == "/opt/ragon/bin/ragon"
    assert "--dangerously-bypass-approvals-and-sandbox" in words
    assert "--skip-git-repo-check" in words
    assert "model_auto_compact_token_limit=1000" in words
    assert words[words.index("-C") + 1] == "$PWD"  # expanded by the shell, not quoted away
    assert "Fix it; don't `rm` $HOME" in words  # one argument, nothing expanded
    assert line.endswith("< /dev/null > /logs/agent/ragon.jsonl 2> /logs/agent/ragon.stderr")


def test_the_digest_threshold_goes_into_the_config_file_only_when_set():
    assert tb_agent_spec.config_toml(None) == ""
    assert tb_agent_spec.config_toml(60) == "[digest]\nmin_lines = 60\n"


# ---- tb_runtime ---------------------------------------------------------------------------------


@pytest.fixture
def release(tmp_path):
    """Release assets and a fake package tree with the amd64 libraries."""
    release_dir = tmp_path / "release"
    release_dir.mkdir()
    for name, asset in tb_runtime.RELEASE_ASSETS.items():
        (release_dir / asset).write_bytes(gzip.compress(f"ELF {name}".encode()))
    return release_dir


def fake_runner(needed="libc.so.6 libm.so.6 libgcc_s.so.1 ld-linux-x86-64.so.2"):
    """Fakes `dpkg-deb -x` (writes the libraries) and `patchelf`; records every call."""
    calls = []

    def run(argv):
        calls.append(argv)
        if argv[0] == "dpkg-deb":
            libs = Path(argv[3]) / "usr/lib/x86_64-linux-gnu"
            libs.mkdir(parents=True, exist_ok=True)
            for name in (tb_runtime.LOADER, *tb_runtime.LIBRARIES):
                (libs / name).write_text(name)
        stdout = needed if "--print-needed" in argv else ""
        return subprocess.CompletedProcess(argv, 0, stdout, "")

    return run, calls


def test_a_build_unpacks_the_binaries_and_points_them_at_the_mounted_libraries(tmp_path, release):
    run, calls = fake_runner()
    target = tb_runtime.build(tmp_path / "rt", release, [tmp_path / "libc6.deb"], "pe", run)
    assert (target / "bin" / "ragon").read_bytes() == b"ELF ragon"
    assert sorted(p.name for p in (target / "lib").iterdir()) == sorted(
        [tb_runtime.LOADER, *tb_runtime.LIBRARIES]
    )
    relinks = [argv for argv in calls if "--set-interpreter" in argv]
    assert len(relinks) == 2
    assert relinks[0][1:5] == [
        "--set-interpreter",
        "/opt/ragon/lib/ld-linux-x86-64.so.2",
        "--set-rpath",
        "/opt/ragon/lib",
    ]


def test_a_binary_that_needs_another_library_stops_the_build(tmp_path, release):
    run, _ = fake_runner(needed="libc.so.6 liblzma.so.5")
    with pytest.raises(ValueError, match="liblzma"):
        tb_runtime.build(tmp_path / "rt", release, [tmp_path / "libc6.deb"], "pe", run)


def test_the_sums_check_names_a_file_that_differs(tmp_path):
    good, bad = tmp_path / "a.gz", tmp_path / "b.gz"
    good.write_bytes(b"a")
    bad.write_bytes(b"b")
    sums = tmp_path / "SHA256SUMS"
    sums.write_text(f"{tb_runtime.sha256(good)}  a.gz\n{tb_runtime.sha256(good)} *b.gz\n")
    tb_runtime.check_sums(sums, [good])
    with pytest.raises(ValueError, match=r"b\.gz"):
        tb_runtime.check_sums(sums, [bad])
    with pytest.raises(ValueError, match="not in"):
        tb_runtime.check_sums(sums, [tmp_path / "c.gz"])


# ---- harbor_agent (only where Harbor is installed) ----------------------------------------------


class FakeResult:
    def __init__(self, return_code):
        self.return_code = return_code
        self.stdout = ""
        self.stderr = ""


class FakeEnvironment:
    """Records what the agent asks of a task container."""

    def __init__(self, mounted):
        self.mounted = mounted
        self.commands = []
        self.uploads = []

    async def exec(self, command, cwd=None, env=None, timeout_sec=None, user=None):
        self.commands.append((command, env, user))
        if command == tb_agent_spec.installed_check():
            return FakeResult(0 if self.mounted else 1)
        return FakeResult(0)

    async def upload_dir(self, source_dir, target_dir):
        self.uploads.append((Path(source_dir), target_dir))


@pytest.fixture
def harbor_agent():
    pytest.importorskip("harbor")
    from dreamference.terminal_bench import harbor_agent

    return harbor_agent


def test_the_agent_needs_a_model_url(harbor_agent, tmp_path):
    with pytest.raises(ValueError, match="model_url"):
        harbor_agent.RagonAgent(tmp_path)


def test_a_mounted_runtime_is_not_uploaded_and_a_missing_one_is(harbor_agent, tmp_path):
    runtime = tmp_path / "rt"
    (runtime / "bin").mkdir(parents=True)
    (runtime / "bin" / "ragon").write_text("")
    agent = harbor_agent.RagonAgent(tmp_path, model_url="http://g:8000", runtime_dir=str(runtime))
    mounted = FakeEnvironment(mounted=True)
    asyncio.run(agent.install(mounted))
    assert mounted.uploads == []
    bare = FakeEnvironment(mounted=False)
    asyncio.run(agent.install(bare))
    assert bare.uploads == [(runtime, "/opt/ragon")]


def test_the_run_passes_the_key_in_the_environment_and_counts_the_log(
    harbor_agent, tmp_path, monkeypatch
):
    monkeypatch.setenv("RAGON_API_KEY", "sk-ragon-test")
    agent = harbor_agent.RagonAgent(tmp_path, model_url="http://g:8000", digest_min_lines="60")
    environment = FakeEnvironment(mounted=True)
    from harbor.models.agent.context import AgentContext

    context = AgentContext()
    asyncio.run(agent.run("do the task", environment, context))
    line, env, _ = environment.commands[-1]
    assert env["RAGON_API_KEY"] == "sk-ragon-test"
    assert "sk-ragon-test" not in line
    assert "min_lines = 60" in line and "do the task" in line
    (tmp_path / tb_agent_spec.EVENT_LOG_NAME).write_text(
        command("1\n2\n") + "\n" + event("turn.completed", usage={"input_tokens": 9}) + "\n"
    )
    agent.populate_context_post_run(context)
    assert context.n_input_tokens == 9
    assert context.metadata["ragon"]["commands"] == 1
    assert context.metadata["ragon"]["seen_outputs"] == 0
