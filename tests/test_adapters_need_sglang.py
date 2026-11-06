"""Installed LoRA adapters start SGLang on a node whose ragon-serve build cannot serve them."""

import pytest

from dreamference.cli import adapter_command
from dreamference.cli.adapter_command import AdapterCommand
from dreamference.cli.dreamference_cli_controller import DreamferenceCLIController as Cli
from dreamference.config import DreamferenceConfig
from dreamference.hardware import ModelMatrixRegistry
from dreamference.hardware.model_matrix_registry import ENGINE_RAGON, ENGINE_SGLANG
from dreamference.vllm_server.adapter_serving import AdapterServing

MODEL = "qwen3.8-27b-nvfp4-dflash2"


@pytest.fixture
def ragon_default():
    """The registry as shipped: ragon-serve is the model's default engine since the 0.9.0 switch."""
    assert ModelMatrixRegistry.engines(MODEL) == [ENGINE_RAGON, ENGINE_SGLANG]


@pytest.fixture
def older_image(monkeypatch):
    """A ragon-serve build from before the adapter feature."""
    monkeypatch.setattr(
        ModelMatrixRegistry,
        "engine_has_feature",
        classmethod(lambda cls, key, engine, feature: False),
    )


@pytest.fixture
def sglang_default(monkeypatch):
    """The rollback position (QWEN38_DEFAULT_ENGINE = ENGINE_SGLANG): SGLang first."""
    monkeypatch.setattr(
        ModelMatrixRegistry, "engines", classmethod(lambda cls, key: [ENGINE_SGLANG, ENGINE_RAGON])
    )


@pytest.fixture
def adapters(monkeypatch):
    monkeypatch.setattr(
        AdapterServing, "launch_adapters", classmethod(lambda cls, key: [{"name": "mine"}])
    )


@pytest.fixture
def no_adapters(monkeypatch):
    monkeypatch.setattr(AdapterServing, "launch_adapters", classmethod(lambda cls, key: []))


def test_ragon_default_with_an_adapter_falls_back_to_sglang_and_says_so(
    ragon_default, older_image, adapters, capsys
):
    assert Cli._engine_for_adapters(MODEL, None, from_flag=False) == ENGINE_SGLANG
    out = capsys.readouterr().out
    assert "Adapters need SGLang until ragon-serve serves them" in out
    assert "mine" in out


def test_ragon_from_the_config_with_an_adapter_also_falls_back(
    ragon_default, older_image, adapters
):
    assert Cli._engine_for_adapters(MODEL, ENGINE_RAGON, from_flag=False) == ENGINE_SGLANG


def test_explicit_engine_ragon_with_an_adapter_is_refused(
    ragon_default, older_image, adapters, capsys
):
    with pytest.raises(SystemExit) as stop:
        Cli._engine_for_adapters(MODEL, ENGINE_RAGON, from_flag=True)
    assert stop.value.code == 2
    assert "--engine ragon cannot serve the installed adapters" in capsys.readouterr().out


def test_ragon_with_no_sglang_entry_stays_on_ragon(adapters, monkeypatch, capsys):
    monkeypatch.setattr(
        ModelMatrixRegistry, "engines", classmethod(lambda cls, key: [ENGINE_RAGON])
    )
    assert Cli._engine_for_adapters(MODEL, None, from_flag=False) is None
    assert capsys.readouterr().out == ""


def test_ragon_without_adapters_is_untouched(ragon_default, no_adapters, capsys):
    assert Cli._engine_for_adapters(MODEL, None, from_flag=False) is None
    assert Cli._engine_for_adapters(MODEL, ENGINE_RAGON, from_flag=True) == ENGINE_RAGON
    assert capsys.readouterr().out == ""


def test_the_shipped_default_with_an_adapter_stays_on_ragon_serve(adapters, capsys):
    # No monkeypatched registry: a plain `server start` with the pinned image, adapter installed.
    assert Cli._engine_for_adapters(MODEL, None, from_flag=False) is None
    assert capsys.readouterr().out == ""


def test_explicit_sglang_with_an_adapter_is_untouched(adapters, capsys):
    assert Cli._engine_for_adapters(MODEL, ENGINE_SGLANG, from_flag=True) == ENGINE_SGLANG
    assert Cli._engine_for_adapters(MODEL, ENGINE_SGLANG, from_flag=False) == ENGINE_SGLANG
    assert capsys.readouterr().out == ""


def test_sglang_default_with_an_adapter_is_untouched(sglang_default, adapters, capsys):
    assert Cli._engine_for_adapters(MODEL, None, from_flag=False) is None
    assert Cli._engine_for_adapters(MODEL, ENGINE_SGLANG, from_flag=True) == ENGINE_SGLANG
    assert capsys.readouterr().out == ""


def test_adapter_install_warns_when_the_engine_is_ragon(ragon_default, older_image, capsys):
    AdapterCommand._warn_if_ragon({"key": MODEL}, DreamferenceConfig())
    assert "serves no adapters yet" in capsys.readouterr().out


def test_adapter_install_does_not_warn_on_the_shipped_default(capsys):
    AdapterCommand._warn_if_ragon({"key": MODEL}, DreamferenceConfig())
    assert capsys.readouterr().out == ""


def test_adapter_install_does_not_warn_when_the_config_names_sglang(monkeypatch, capsys):
    monkeypatch.setenv("DREAMFERENCE_SERVER_ENGINE", ENGINE_SGLANG)
    AdapterCommand._warn_if_ragon({"key": MODEL}, DreamferenceConfig())
    assert capsys.readouterr().out == ""


def test_adapter_install_does_not_warn_on_sglang(sglang_default, capsys):
    AdapterCommand._warn_if_ragon({"key": MODEL}, DreamferenceConfig())
    assert capsys.readouterr().out == ""
    assert adapter_command  # module imported


def test_adapter_install_reads_the_engine_from_the_config_it_was_given(
    older_image, monkeypatch, capsys
):
    # The warning built a fresh default configuration, so a `--config` naming ragon-serve was not read.
    from dreamference.node.node_identity import NodeIdentity

    monkeypatch.setattr(NodeIdentity, "read", classmethod(lambda cls: object()))
    monkeypatch.setattr(
        adapter_command.AdapterVerifier,
        "served_base",
        classmethod(lambda cls, host, model: {"key": MODEL}),
    )
    entry = {"display_name": "Tuned", "version": "1", "name": "tuned", "trust": "dreamference"}
    monkeypatch.setattr(
        adapter_command.AdapterInstaller, "install", classmethod(lambda cls, *a, **k: entry)
    )

    class Config:
        vllm_host = "http://127.0.0.1:8000"
        model = MODEL
        server_engine = ENGINE_RAGON
        hf_token = None

    args = Cli.build_parser().parse_args(["adapter", "install", "tuned"])
    assert AdapterCommand.run(args, Config()) == 0
    assert "serves no adapters yet" in capsys.readouterr().out


@pytest.fixture
def engine_serves_adapters(monkeypatch):
    """An engine build whose recipe lists the adapter feature."""
    monkeypatch.setattr(
        ModelMatrixRegistry,
        "engine_has_feature",
        classmethod(
            lambda cls, key, engine, feature: engine == ENGINE_RAGON and feature == "lora-adapters"
        ),
    )


def test_the_shipped_recipe_lists_the_adapter_feature():
    assert ModelMatrixRegistry.engine_has_feature(MODEL, ENGINE_RAGON, "lora-adapters")


def test_an_engine_with_the_adapter_feature_keeps_ragon_serve(
    ragon_default, adapters, engine_serves_adapters, capsys
):
    assert Cli._engine_for_adapters(MODEL, None, from_flag=False) is None
    assert Cli._engine_for_adapters(MODEL, ENGINE_RAGON, from_flag=False) == ENGINE_RAGON
    assert Cli._engine_for_adapters(MODEL, ENGINE_RAGON, from_flag=True) == ENGINE_RAGON
    assert capsys.readouterr().out == ""


def test_sglang_flag_is_honoured_when_the_engine_serves_adapters(
    adapters, engine_serves_adapters, capsys
):
    assert Cli._engine_for_adapters(MODEL, ENGINE_SGLANG, from_flag=True) == ENGINE_SGLANG
    assert capsys.readouterr().out == ""


def test_an_engine_without_the_feature_says_why_it_starts_sglang(
    ragon_default, older_image, adapters, capsys
):
    assert Cli._engine_for_adapters(MODEL, None, from_flag=False) == ENGINE_SGLANG
    assert "no adapter feature" in capsys.readouterr().out


def test_install_does_not_warn_when_the_engine_serves_adapters(
    ragon_default, engine_serves_adapters, capsys
):
    AdapterCommand._warn_if_ragon({"key": MODEL}, DreamferenceConfig())
    assert capsys.readouterr().out == ""
