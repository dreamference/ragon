"""The engine choice, fall-back and canary helpers of the CLI controller."""

import subprocess
from unittest.mock import MagicMock

import pytest

from dreamference.cli import dreamference_cli_controller as ctl
from dreamference.cli.dreamference_cli_controller import DreamferenceCLIController as C
from dreamference.hardware import ModelMatrixRegistry


def _engines(monkeypatch, engines):
    monkeypatch.setattr(ModelMatrixRegistry, "engines", classmethod(lambda cls, m: list(engines)))


def _reply(content=None, status=200, tool_calls=None):
    r = MagicMock(status_code=status)
    r.json.return_value = {"choices": [{"message": {"content": content, "tool_calls": tool_calls}}]}
    return r


def _canary(replies):
    session = MagicMock()
    session.post.side_effect = replies
    return C._run_correctness_canary(session, "http://h/v1", "m", {})


def test_canary_all_pass():
    call = [{"function": {"name": "get_weather", "arguments": '{"city":"Paris"}'}}]
    rows = _canary(
        [
            _reply("391"),
            _reply("```python\ndef add(a, b):\n    return a + b\n```"),
            _reply(None, tool_calls=call),
        ]
    )
    assert all("pass" in v for v in rows.values())


def test_canary_failures():
    rows = _canary([_reply("1"), _reply("def (:"), _reply("prose")])
    assert all("FAIL" in v for v in rows.values())


def test_canary_unreachable():
    rows = _canary([_reply(status=500), ConnectionError("x"), _reply(status=500)])
    assert all("unreachable" in v for v in rows.values())


def test_server_engine_messages(monkeypatch, capsys):
    _engines(monkeypatch, ["ragon", "sglang"])
    monkeypatch.setattr(
        ModelMatrixRegistry,
        "get_launch_overrides",
        classmethod(lambda cls, m, e: {"vision": False}),
    )
    assert C._server_engine("m", None, False) is None
    out = capsys.readouterr().out
    assert "also: --engine sglang" in out and "reads no images" in out
    assert C._server_engine("m", "sglang", True) == "sglang"
    with pytest.raises(SystemExit) as info:
        C._server_engine("m", "vllm", True)
    assert info.value.code == 2
    with pytest.raises(SystemExit):
        C._server_engine("m", "vllm", False)
    assert "server_engine in the config" in capsys.readouterr().out


def _adapters(monkeypatch, adapters, has_feature):
    from dreamference.vllm_server.adapter_serving import AdapterServing

    monkeypatch.setattr(AdapterServing, "launch_adapters", classmethod(lambda cls, m: adapters))
    monkeypatch.setattr(
        ModelMatrixRegistry, "engine_has_feature", classmethod(lambda cls, *a: has_feature)
    )


def test_engine_for_adapters(monkeypatch, capsys):
    _engines(monkeypatch, ["ragon", "sglang"])
    assert C._engine_for_adapters("m", "sglang", False) == "sglang"
    _adapters(monkeypatch, [], False)
    assert C._engine_for_adapters("m", None, False) is None
    _adapters(monkeypatch, [{"name": "a1"}], True)
    assert C._engine_for_adapters("m", None, False) is None
    _adapters(monkeypatch, [{"name": "a1"}], False)
    assert C._engine_for_adapters("m", None, False) == "sglang"
    assert "a1" in capsys.readouterr().out
    with pytest.raises(SystemExit) as info:
        C._engine_for_adapters("m", "ragon", True)
    assert info.value.code == 2
    _engines(monkeypatch, ["ragon"])
    assert C._engine_for_adapters("m", None, False) is None


def test_may_fall_back_and_exit_or_raise(monkeypatch, capsys):
    _engines(monkeypatch, ["ragon", "sglang"])
    assert C._may_fall_back("m", None, False) is True
    assert C._may_fall_back("m", None, True) is False
    assert C._may_fall_back("m", "sglang", False) is False
    assert C._exit_unless_fall_back(True, "why") == "why"
    with pytest.raises(SystemExit) as info:
        C._exit_unless_fall_back(False, "why", "m", None)
    assert info.value.code == 1
    assert "SGLang serves the same model" in capsys.readouterr().out
    with pytest.raises(SystemExit):
        C._exit_unless_fall_back(False, "why")
    assert C._raise_unless_fall_back(True, RuntimeError("boom"), "m", None) == "boom"
    with pytest.raises(RuntimeError):
        C._raise_unless_fall_back(False, RuntimeError("boom"), "m", None)
    capsys.readouterr()
    _engines(monkeypatch, ["vllm"])
    C._suggest_engine_fallback("m", None, "x")
    assert capsys.readouterr().out == ""


def test_fall_back_to_sglang_reruns_start(monkeypatch, capsys):
    rerun = MagicMock()
    monkeypatch.setattr(C, "run_cli", rerun)
    vm = MagicMock()
    C._fall_back_to_sglang(vm, MagicMock(port=8000), "line one\nline two", ["server", "start"])
    vm.stop_server.assert_called_once_with(port=8000)
    vm.remove_server.assert_called_once_with(port=8000)
    rerun.assert_called_once_with(["server", "start", "--engine", "sglang"])
    assert "line one" in capsys.readouterr().out
    C._fall_back_to_sglang(vm, MagicMock(port=1), "", [])
    assert "no reason given" in capsys.readouterr().out


def test_tool_call_canary(monkeypatch, capsys):
    from dreamference.vllm_server.tool_call_canary import ToolCallCanary

    monkeypatch.setattr(ctl, "model_key_for_served_id", lambda *a: "k", raising=False)
    import dreamference.hardware as hw

    monkeypatch.setattr(hw, "model_key_for_served_id", lambda *a: "k")
    vm = MagicMock(host="http://h")
    vm.get_models.return_value = []
    assert C._run_tool_call_canary(vm, "m") is None
    vm.get_models.return_value = ["served"]
    monkeypatch.setattr(ToolCallCanary, "run", classmethod(lambda cls, *a, **k: (True, "ok")))
    assert C._run_tool_call_canary(vm, "m") is True
    monkeypatch.setattr(ToolCallCanary, "run", classmethod(lambda cls, *a, **k: (False, "bad")))
    assert C._run_tool_call_canary(vm, "m", "sglang") is False
    out = capsys.readouterr().out
    assert "Tool-call check skipped" in out and "well formed" in out and "bad" in out


def test_read_kv_pool_facts(monkeypatch):
    log = "GPU KV cache size: 1,000 tokens\nMaximum concurrency for 500 tokens per request: 2.00x"
    monkeypatch.setattr(subprocess, "run", lambda *a, **k: MagicMock(stdout=log, stderr=""))
    rows = C._read_kv_pool_facts("c", "500")
    assert "2.00 x max-model-len" in rows["KV Pool"] and "2.00x at 500" in rows["Max Concurrency"]
    assert "x max-model-len" not in C._read_kv_pool_facts("c", None)["KV Pool"]
    monkeypatch.setattr(subprocess, "run", lambda *a, **k: MagicMock(stdout="", stderr=""))
    assert C._read_kv_pool_facts("c", None) == {}

    def boom(*a, **k):
        raise OSError

    monkeypatch.setattr(subprocess, "run", boom)
    assert C._read_kv_pool_facts("c", None) == {}


def _facts(monkeypatch, cmd, image="img", ragon_args=None, cache=None):
    import json

    from dreamference.vllm_server import vllm_server_manager as vsm
    from dreamference.vllm_server.ragon_serve_launch_builder import RagonServeLaunchBuilder

    data = json.dumps([{"Config": {"Cmd": cmd, "Image": image, "Labels": {"a": "b"}}}])
    monkeypatch.setattr(
        subprocess, "run", lambda *a, **k: MagicMock(returncode=0, stdout=data, stderr="")
    )
    monkeypatch.setattr(
        RagonServeLaunchBuilder, "server_args_of", classmethod(lambda cls, c: ragon_args)
    )
    monkeypatch.setattr(
        RagonServeLaunchBuilder, "facts", classmethod(lambda cls, *a: {"Engine": "ragon"})
    )
    monkeypatch.setattr(C, "_served_key", classmethod(lambda cls, h, a: "key"))
    monkeypatch.setattr(
        C, "_detect_recipe_drift", classmethod(lambda cls, *a: {"Recipe Match": "same"})
    )
    monkeypatch.setattr(C, "_read_kv_pool_facts", classmethod(lambda cls, *a: {"KV Pool": "1"}))
    monkeypatch.setattr(
        ctl.VLLMServerManager, "detect_image_vllm_version", lambda self, i: (0, 11), raising=False
    )
    if cache is not None:
        monkeypatch.setattr(vsm, "VLLM_CACHE_HOME", cache)
    return C._collect_engine_facts(8000, "http://h", "alias")


def test_collect_engine_facts_for_ragon_serve(monkeypatch):
    facts = _facts(monkeypatch, ["ragon-serve"], ragon_args=["--x"])
    assert facts == {"Engine": "ragon", "Recipe Match": "same"}


def test_collect_engine_facts_for_vllm(monkeypatch, tmp_path):
    (tmp_path / "torch_compile_cache").mkdir()
    (tmp_path / "torch_compile_cache" / "g").mkdir()
    cmd = [
        "serve",
        "--attention-backend",
        "fa",
        "--kv-cache-dtype",
        "fp8",
        "--enable-prefix-caching",
        "--speculative-config",
        '{"method": "dflash", "num_speculative_tokens": 4}',
        "--load-format",
        "tensorizer",
    ]
    facts = _facts(monkeypatch, cmd, cache=tmp_path)
    assert facts["Image"] == "img (vLLM 0.11)"
    assert facts["Attention"] == "fa" and facts["Prefix Caching"] == "On"
    assert facts["Speculative"] == "dflash n=4 via self (in-checkpoint head)"
    assert facts["Tensorizer"] == "Active" and facts["Compile Cache"].startswith("Warm")
    assert facts["Recipe Match"] == "same" and facts["KV Pool"] == "1"


def test_collect_engine_facts_defaults_and_bad_speculative_json(monkeypatch, tmp_path):
    (tmp_path / "torch_compile_cache").mkdir()
    facts = _facts(
        monkeypatch,
        ["serve", "--no-enable-prefix-caching", "--speculative-config", "{bad"],
        image="",
        cache=tmp_path,
    )
    assert facts["Image"].startswith("Unknown")
    assert facts["Prefix Caching"] == "Off (recipe)" and facts["Speculative"] == "{bad"
    assert facts["Compile Cache"].startswith("Cold")
    facts = _facts(monkeypatch, ["serve"], cache=tmp_path)
    assert facts["Prefix Caching"] == "vLLM default" and facts["Speculative"] == "Disabled"


def test_collect_engine_facts_unknown_cache_and_inspect_failure(monkeypatch, tmp_path):
    from dreamference.vllm_server import vllm_server_manager as vsm

    class _Boom:
        def __truediv__(self, other):
            raise OSError

    _facts(monkeypatch, ["serve"], cache=_Boom())
    monkeypatch.setattr(vsm, "VLLM_CACHE_HOME", _Boom())
    assert C._collect_engine_facts(8000, "http://h", "a")["Compile Cache"] == "Unknown"

    def boom(*a, **k):
        raise OSError

    monkeypatch.setattr(subprocess, "run", boom)
    facts = C._collect_engine_facts(8000, "http://h", "a")
    assert facts["Image"].startswith("Unknown") and "Recipe Match" not in facts


def test_served_key(monkeypatch):
    import requests

    monkeypatch.undo()
    monkeypatch.setattr(
        requests,
        "get",
        lambda *a, **k: MagicMock(json=lambda: {"data": [{"id": "served"}]}),
    )
    monkeypatch.setattr(ctl, "model_key_for_served_id", lambda s, preferred=None: "key")
    assert C._served_key("http://h", "alias") == "key"
    monkeypatch.setattr(ctl, "model_key_for_served_id", lambda s, preferred=None: None)
    assert C._served_key("http://h", "alias") == "alias"

    def boom(*a, **k):
        raise requests.ConnectionError

    monkeypatch.setattr(requests, "get", boom)
    assert C._served_key("http://h", "alias") == "alias"


def _session(content=None, status=200, raises=False, **msg):
    session = MagicMock()
    if raises:
        session.post.side_effect = ConnectionError
        return session
    message = {"content": content, **msg}
    session.post.return_value = MagicMock(
        status_code=status, json=lambda: {"choices": [{"message": message}]}
    )
    return session


def test_text_probes_hit_and_miss():
    args = ("http://h", "m", {})
    rich = "<think> <|im_start|> <tool_call>, <answer> " + ctl._ARG_ANTARTIFACT
    assert C._inspect_probe_reasoning(_session(rich), *args) is True
    assert C._inspect_probe_reasoning(_session("", reasoning="x"), *args) is True
    assert C._inspect_probe_chatml(_session(rich), *args) is True
    assert C._inspect_probe_artifact(_session(rich), *args) is True
    assert "<answer>" in C._inspect_probe_tags(_session(rich), *args)
    for probe in (
        C._inspect_probe_reasoning,
        C._inspect_probe_chatml,
        C._inspect_probe_artifact,
    ):
        assert probe(_session("nothing"), *args) is False
        assert probe(_session(rich, status=500), *args) is False
        assert probe(_session(raises=True), *args) is False
    assert C._inspect_probe_tags(_session("none"), *args) == "None detected"
    assert C._inspect_probe_tags(_session(raises=True), *args) == "None detected"


def test_tools_probe():
    args = ("http://h", "m", {})
    assert C._inspect_probe_tools(_session("", tool_calls=[{}]), *args) is True
    assert C._inspect_probe_tools(_session(""), *args) is False
    assert C._inspect_probe_tools(_session("", status=500), *args) is False
    assert C._inspect_probe_tools(_session(raises=True), *args) is False


def test_moe_probe(monkeypatch):
    import requests

    def get(cfg, status=200):
        return lambda *a, **k: MagicMock(status_code=status, json=lambda: cfg)

    monkeypatch.setattr(requests, "get", get({"num_experts": 8}))
    assert C._inspect_probe_moe("a/b") == "Yes"
    monkeypatch.setattr(requests, "get", get({"model_type": "dense"}))
    assert C._inspect_probe_moe("a/b") == "No"
    monkeypatch.setattr(requests, "get", get({}, 404))
    assert C._inspect_probe_moe("org/Mixtral-8x7B") == "Yes (heuristic)"
    assert C._inspect_probe_moe("org/plain") == "Unknown"

    def boom(*a, **k):
        raise requests.ConnectionError

    monkeypatch.setattr(requests, "get", boom)
    assert C._inspect_probe_moe("org/plain") == "Unknown"


def test_kv_cache_probe():
    text = 'vllm:kv_cache_usage_perc{engine="0"} 0.25\n'
    s = MagicMock()
    s.get.return_value = MagicMock(status_code=200, text=text)
    assert C._inspect_probe_kv_cache(s, "http://h") == "25.0%"
    s.get.return_value = MagicMock(status_code=200, text="")
    assert C._inspect_probe_kv_cache(s, "http://h") == "Unknown"
    s.get.side_effect = ConnectionError
    assert C._inspect_probe_kv_cache(s, "http://h") == "Unknown"
