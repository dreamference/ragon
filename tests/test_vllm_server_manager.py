"""The vLLM launch command never turns on prompt or completion logging (logging spec O10, §11.4)."""

import pytest

from dreamference.hardware.model_downloader import ModelDownloader
from dreamference.hardware.model_matrix_registry import ModelMatrixRegistry
from dreamference.vllm_server import VLLMServerManager

FORBIDDEN = ("--enable-log-requests", "--enable-log-outputs", "--max-log-len", "--log-requests")

CASES = [
    (key, engine)
    for key in sorted(ModelMatrixRegistry.MATRIX)
    for engine in (ModelMatrixRegistry.engines(key) or [None])
]


@pytest.mark.parametrize(("model", "engine"), CASES)
def test_no_registry_entry_and_engine_builds_a_request_logging_command(monkeypatch, model, engine):
    mgr = VLLMServerManager()
    monkeypatch.setattr(ModelDownloader, "is_model_tensorized", classmethod(lambda cls, key: False))
    monkeypatch.setattr(mgr, "is_docker_available", lambda: True)
    monkeypatch.setattr(mgr, "image_has_tensorizer", lambda image: False)
    monkeypatch.setattr(mgr, "detect_image_vllm_version", lambda image: (0, 12))
    monkeypatch.setattr(mgr, "_docker_run_prefix", lambda *a, **k: ["docker", "run"])

    try:
        cmd = mgr.build_launch_command(model=model, engine=engine, gate=False)
    except RuntimeError as exc:
        pytest.skip(f"no launch command for this entry in the sandbox: {exc}")

    joined = " ".join(str(part) for part in cmd)
    for token in FORBIDDEN:
        assert token not in joined
