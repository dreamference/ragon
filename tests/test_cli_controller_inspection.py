"""Tests for the CLI controller's server-inspection helpers (deep panel, canary, engine facts)."""

import json
import subprocess
from types import SimpleNamespace
from unittest.mock import MagicMock, patch

from rich.console import Console

from dreamference.cli import dreamference_cli_controller as mod
from dreamference.cli.dreamference_cli_controller import DreamferenceCLIController as Ctl

INSPECT_ARGS = "dreamference.cli.model_deep_inspector.ModelDeepInspector"


def _inspect(cmd, image="img:1", labels=None, rc=0):
    """A fake `docker inspect` result."""
    payload = [{"Config": {"Cmd": cmd, "Image": image, "Labels": labels}}]
    return SimpleNamespace(returncode=rc, stdout=json.dumps(payload), stderr="")


def _render(profile, run):
    out = Console(record=True, width=200)
    with (
        patch("subprocess.run", run),
        patch(f"{INSPECT_ARGS}.quantization_map", return_value={"a": "b"}),
        patch(f"{INSPECT_ARGS}.kv_geometry", return_value={}),
        patch(f"{INSPECT_ARGS}.sampling_provenance", return_value={"long": "x"}),
        patch(f"{INSPECT_ARGS}.graph_coverage", return_value={"g": "y"}),
        patch(f"{INSPECT_ARGS}.profile_acceptance_by_workload", return_value=profile),
    ):
        Ctl._render_deep_inspection(
            out, SimpleNamespace(model="m"), "http://h", "http://h:8123/v1", "m"
        )
    return out.export_text()


def test_deep_inspection_renders_table_and_suggestion():
    spec = json.dumps({"num_speculative_tokens": 4})
    cmd = ["--kv-cache-dtype", "fp8", "--speculative-config", spec, "--max-num-seqs"]
    profile = [
        {"category": "code", "acceptance": 80.0, "tau": 3.2, "slots": [0.9, 0.1], "suggested_n": 5},
        {"category": "prose", "acceptance": 30.0, "tau": 1.5, "slots": [], "suggested_n": 2},
    ]
    text = _render(profile, MagicMock(return_value=_inspect(cmd)))
    assert "Quantization Map" in text and "code" in text
    assert "Running n=4" in text and "2-5" in text


def test_deep_inspection_without_counters_or_docker():
    text = _render([], MagicMock(side_effect=OSError("no docker")))
    assert "No speculative counters" in text


def test_deep_inspection_bad_spec_json_and_inspect_failure():
    cmd = ["--speculative-config", "{bad"]
    profile = [{"category": "c", "acceptance": 1.0, "tau": 1.0, "slots": [], "suggested_n": 1}]
    text = _render(profile, MagicMock(return_value=_inspect(cmd)))
    assert "Running n=" not in text
    text = _render(profile, MagicMock(return_value=_inspect(cmd, rc=1)))
    assert "Acceptance" in text


def test_tool_call_canary_paths():
    mgr = MagicMock(host="http://h")
    mgr.get_models.return_value = []
    assert Ctl._run_tool_call_canary(mgr, None) is None
    mgr.get_models.return_value = ["m"]
    target = "dreamference.vllm_server.tool_call_canary.ToolCallCanary.run"
    with patch(target, return_value=(True, "ok")):
        assert Ctl._run_tool_call_canary(mgr, None) is True
    with (
        patch(target, return_value=(False, "bad")),
        patch.object(Ctl, "_suggest_engine_fallback") as fallback,
    ):
        assert Ctl._run_tool_call_canary(mgr, "req") is False
    fallback.assert_called_once()


def test_read_kv_pool_facts():
    log = SimpleNamespace(
        stdout="GPU KV cache size: 1,000 tokens\n",
        stderr="Maximum concurrency for 500 tokens per request: 2.00x\n",
    )
    with patch("subprocess.run", return_value=log):
        rows = Ctl._read_kv_pool_facts("c", "500")
        assert rows["KV Pool"].startswith("1,000 tokens") and "2.00 x" in rows["KV Pool"]
        assert rows["Max Concurrency"] == "2.00x at 500 tokens/request"
        assert Ctl._read_kv_pool_facts("c", None)["KV Pool"] == "1,000 tokens"
    with patch("subprocess.run", side_effect=subprocess.SubprocessError()):
        assert Ctl._read_kv_pool_facts("c", None) == {}


def _drift(running, expected):
    mgr = MagicMock()
    mgr.build_launch_command.return_value = expected
    with patch.object(mod, "VLLMServerManager", return_value=mgr):
        return Ctl._detect_recipe_drift("qwen3.8-27b-nvfp4-dflash2", 8000, running)


def test_recipe_drift_match_and_difference():
    expected = ["docker", "run", "img", "serve", "m", "--a=1", "--b", "2", "--flag", "--c", "3"]
    same = ["serve", "m", "--a", "1", "--b", "2", "--flag", "--c", "3"]
    assert "Recipe Match" in _drift(same, expected)
    rows = _drift(["serve", "m", "--a", "9", "--extra"], expected)
    assert "differ" in rows["Recipe Match"]
    assert rows["  drift --a"] == "running=9  recipe=1"
    assert rows["  drift --extra"].startswith("running=(set)")


def test_recipe_drift_engines_and_failures():
    expected = ["ragon-serve", "--a", "1"]
    assert "Recipe Match" in _drift(["ragon-serve", "--a", "1"], expected)
    assert "Recipe Match" in _drift(["python3", "-m", "sglang.launch_server", "--a", "1"], expected)
    assert _drift(["serve", "m"], ["docker", "run", "img"]) == {}
    mgr = MagicMock()
    mgr.build_launch_command.side_effect = ValueError("x")
    with patch.object(mod, "VLLMServerManager", return_value=mgr):
        assert Ctl._detect_recipe_drift("k", 1, []) == {}


def test_served_key():
    resp = MagicMock()
    resp.json.return_value = {"data": [{"id": "served"}]}
    with (
        patch("requests.get", return_value=resp),
        patch.object(mod, "model_key_for_served_id", return_value="key"),
    ):
        assert Ctl._served_key("http://h", "alias") == "key"
    with patch("requests.get", side_effect=OSError()):
        assert Ctl._served_key("http://h", "alias") == "alias"


def test_engine_facts_vllm_container(tmp_path):
    spec = json.dumps({"method": "dflash", "num_speculative_tokens": 3, "model": "d"})
    cmd = [
        "serve", "--attention-backend", "fa", "--load-format", "tensorizer",
        "--enable-prefix-caching", "--speculative-config", spec, "--max-model-len", "100",
    ]  # fmt: skip
    cache = tmp_path / "torch_compile_cache"
    cache.mkdir()
    (cache / "g1").mkdir()
    mgr = MagicMock()
    mgr.detect_image_vllm_version.return_value = (0, 11)
    with (
        patch("subprocess.run", return_value=_inspect(cmd)),
        patch.object(mod, "VLLMServerManager", return_value=mgr),
        patch("dreamference.vllm_server.vllm_server_manager.VLLM_CACHE_HOME", tmp_path),
        patch.object(Ctl, "_read_kv_pool_facts", return_value={"KV Pool": "p"}),
        patch.object(Ctl, "_served_key", return_value="k"),
        patch.object(Ctl, "_detect_recipe_drift", return_value={"Recipe Match": "ok"}),
    ):
        facts = Ctl._collect_engine_facts(8000, "http://h", "alias")
        assert facts["Image"] == "img:1 (vLLM 0.11)"
        assert facts["Attention"] == "fa" and facts["Tensorizer"] == "Active"
        assert facts["Prefix Caching"] == "On"
        assert facts["Speculative"] == "dflash n=3 via d"
        assert facts["Compile Cache"] == "Warm (1 graph(s))"
        assert facts["Recipe Match"] == "ok" and facts["KV Pool"] == "p"


def test_engine_facts_defaults_and_cold_cache(tmp_path):
    cmd = ["serve", "--no-enable-prefix-caching", "--speculative-config", "weird"]
    with (
        patch("subprocess.run", return_value=_inspect(cmd, image="")),
        patch("dreamference.vllm_server.vllm_server_manager.VLLM_CACHE_HOME", tmp_path),
        patch.object(Ctl, "_read_kv_pool_facts", return_value={}),
        patch.object(Ctl, "_served_key", return_value="k"),
        patch.object(Ctl, "_detect_recipe_drift", return_value={}),
    ):
        facts = Ctl._collect_engine_facts(8000, "http://h", "alias")
    assert facts["Image"].startswith("Unknown")
    assert facts["Prefix Caching"] == "Off (recipe)" and facts["Speculative"] == "weird"
    assert facts["Compile Cache"].startswith("Cold")
    with (
        patch("subprocess.run", return_value=_inspect(["serve"])),
        patch("dreamference.vllm_server.vllm_server_manager.VLLM_CACHE_HOME", tmp_path),
        patch.object(Ctl, "_read_kv_pool_facts", return_value={}),
        patch.object(Ctl, "_served_key", return_value="k"),
        patch.object(Ctl, "_detect_recipe_drift", return_value={}),
    ):
        facts = Ctl._collect_engine_facts(8000, "http://h", "alias")
    assert facts["Prefix Caching"] == "vLLM default" and facts["Speculative"] == "Disabled"


def test_engine_facts_ragon_serve_and_docker_failure():
    with (
        patch("subprocess.run", return_value=_inspect(["ragon-serve", "--a", "1"])),
        patch.object(Ctl, "_served_key", return_value="k"),
        patch.object(Ctl, "_detect_recipe_drift", return_value={"Recipe Match": "ok"}),
    ):
        facts = Ctl._collect_engine_facts(8000, "http://h", "alias")
    assert facts["Recipe Match"] == "ok"
    with (
        patch("subprocess.run", side_effect=OSError()),
        patch.object(Ctl, "_read_kv_pool_facts", return_value={}),
    ):
        assert Ctl._collect_engine_facts(8000, "http://h", "a")["Image"].startswith("Unknown")
