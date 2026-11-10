"""The log level: one grammar (LOGGING §4.2) and one precedence (§4.3) in Python and in Rust."""

from pathlib import Path

import pytest

from dreamference.config.dreamference_config import DEFAULT_LOG_LEVEL, DreamferenceConfig
from dreamference.config.log_level_spec import LogLevelSpec

REPO = Path(__file__).resolve().parent.parent
VECTORS = REPO / "ragon-rs" / "vectors" / "log_level_cases.txt"
RUST_TEST = REPO / "ragon-rs" / "log" / "tests" / "vectors.rs"


def _vector_lines() -> list[list[str]]:
    rows = []
    for line in VECTORS.read_text().splitlines():
        if line.startswith("#") or not line.strip():
            continue
        rows.append([field.strip() for field in line.split("|")])
    return rows


@pytest.mark.parametrize("fields", _vector_lines(), ids=lambda fields: fields[0] or "blank")
def test_every_shared_vector_parses_to_the_expected_levels(fields):
    spec_text, want_default, want_overrides, want_warnings = fields
    spec = LogLevelSpec.parse(spec_text)
    assert spec.default == (None if want_default == "-" else want_default)
    got = [f"{component}={level}" for component, level in spec.overrides.items()]
    assert got == ([] if want_overrides == "-" else want_overrides.split(";"))
    assert len(spec.warnings) == int(want_warnings)


def test_the_rust_crate_reads_the_same_vectors():
    # The Rust test includes the same file; a rename of either side must fail here.
    assert "log_level_cases.txt" in RUST_TEST.read_text()
    assert len(_vector_lines()) >= 10


def test_the_level_resolves_through_the_tiers(tmp_path, monkeypatch):
    monkeypatch.delenv("RAGON_LOG", raising=False)
    cfg_file = tmp_path / "tiers.yaml"
    cfg_file.write_text("vllm_host: http://localhost:8000\n")
    assert DreamferenceConfig(config_file=str(cfg_file)).log_level == DEFAULT_LOG_LEVEL

    cfg_file.write_text('vllm_host: http://localhost:8000\nlog_level: "warn,engine=debug"\n')
    assert DreamferenceConfig(config_file=str(cfg_file)).log_level == "warn,engine=debug"

    monkeypatch.setenv("RAGON_LOG", "debug")
    assert DreamferenceConfig(config_file=str(cfg_file)).log_level == "debug"

    monkeypatch.setenv("RAGON_LOG", "   ")
    assert DreamferenceConfig(config_file=str(cfg_file)).log_level == "warn,engine=debug"

    assert DreamferenceConfig(config_file=str(cfg_file), log_level="trace").log_level == "trace"


def test_a_default_log_level_is_not_written_back(tmp_path, monkeypatch):
    monkeypatch.delenv("RAGON_LOG", raising=False)
    cfg_file = tmp_path / "save.yaml"
    cfg_file.write_text("vllm_host: http://localhost:8000\n")
    config = DreamferenceConfig(config_file=str(cfg_file))
    saved = config.save_config(tmp_path / "out.yaml").read_text()
    assert "log_level" not in saved
    config = DreamferenceConfig(config_file=str(cfg_file), log_level="info,engine=debug")
    saved = config.save_config(tmp_path / "out2.yaml").read_text()
    assert "info,engine=debug" in saved
