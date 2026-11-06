"""Coverage for the server manager's image build failures, log tail and loading status."""

from __future__ import annotations

import subprocess
from types import SimpleNamespace

import pytest

from dreamference.vllm_server.vllm_server_manager import DEFAULT_VLLM_IMAGE, VLLMServerManager


@pytest.mark.parametrize(
    "error, text",
    [
        (subprocess.CalledProcessError(1, "docker"), "Failed to build"),
        (OSError("gone"), "Unexpected error building"),
    ],
)
def test_a_failed_image_build_is_reported_and_the_image_is_not_ready(
    monkeypatch, capsys, error, text
):
    manager = VLLMServerManager()
    monkeypatch.setattr(manager, "is_image_present", lambda image: False)

    def build(*args, **kwargs):
        raise error

    monkeypatch.setattr(subprocess, "run", build)
    assert manager.ensure_docker_image(DEFAULT_VLLM_IMAGE) is False
    assert text in capsys.readouterr().out


def test_the_request_log_tail_follows_the_container_of_the_port(monkeypatch, capsys):
    calls = []
    monkeypatch.setattr(subprocess, "run", lambda command, **kw: calls.append(command))
    VLLMServerManager().show_request_logs(8123)
    assert calls == [["docker", "logs", "-f", "dreamference-vllm-8123"]]
    assert "dreamference-vllm-8123" in capsys.readouterr().out


@pytest.mark.parametrize(
    "log, expected",
    [
        (
            "INFO Loading safetensors checkpoint shards: 50%",
            "INFO Loading safetensors checkpoint shards: 50%",
        ),
        ("warming up", "Warming up model (CUDA graphs)..."),
        ("loading model weights", "Loading model weights into GPU memory..."),
        ("hf_hub_download x", "Downloading model components..."),
        ("nothing useful", "Initializing container..."),
    ],
)
def test_a_server_that_is_not_healthy_reports_its_loading_stage_from_the_container_log(
    monkeypatch, log, expected
):
    manager = VLLMServerManager("http://localhost:8123")
    monkeypatch.setattr(manager, "check_health", lambda: False)

    def run(command, **kwargs):
        if command[1] == "inspect":
            return SimpleNamespace(stdout="true\n", stderr="")
        return SimpleNamespace(stdout=log, stderr="")

    monkeypatch.setattr(subprocess, "run", run)
    assert manager.get_server_status()["loading_status"] == expected


def test_a_failing_docker_leaves_the_loading_status_empty(monkeypatch):
    manager = VLLMServerManager("http://localhost:8123")
    monkeypatch.setattr(manager, "check_health", lambda: False)

    def run(command, **kwargs):
        raise OSError("no docker")

    monkeypatch.setattr(subprocess, "run", run)
    assert manager.get_server_status()["loading_status"] is None
