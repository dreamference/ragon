"""Provisioning's image copy and pull, model assignment, resolver check and server wait.

Every ssh command, resolver lookup and sleep is a stand-in.
"""

import subprocess
from types import SimpleNamespace
from typing import Any

import pytest

from dreamference.node import node_provisioner
from dreamference.node.node_provisioner import NodeProvisioner


@pytest.fixture
def provisioner(tmp_path, monkeypatch):
    monkeypatch.setenv("XDG_RUNTIME_DIR", str(tmp_path))
    monkeypatch.setattr(node_provisioner.time, "sleep", lambda seconds: None)
    made = NodeProvisioner(["node-b"], {"user": "owner", "model": "org/model"})
    made.results["node-b"] = {"host": "node-b", "done": [], "probe": {}}
    made.plan = {"copy_images": ["img:a"], "pull_images": ["img:b"]}
    return made


def session(pipe: int = 0, pull: int = 0, admin: int = 0) -> Any:
    return SimpleNamespace(
        pipe_into=lambda stages, command: pipe,
        run=lambda command, timeout=None: subprocess.CompletedProcess([], pull, "", "pull error"),
        run_admin=lambda arguments: subprocess.CompletedProcess([], admin, "set error", ""),
    )


def test_images_the_node_has_are_not_copied_or_pulled(provisioner):
    provisioner.results["node-b"]["probe"] = {"image:img:a": "1", "image:img:b": "1"}
    assert provisioner._images("node-b", session(pipe=1, pull=1)) is True


def test_images_missing_are_copied_then_pulled(provisioner):
    assert provisioner._images("node-b", session()) is True


def test_a_failed_image_copy_names_the_image_and_the_command_to_rerun(provisioner):
    assert provisioner._images("node-b", session(pipe=1)) is False
    result = provisioner.results["node-b"]
    assert result["failed"] == "copy an image" and "img:a" in result["reason"]
    assert result["left"] == "ragon-admin node provision node-b"


def test_a_failed_image_pull_keeps_the_tail_of_the_error(provisioner):
    assert provisioner._images("node-b", session(pull=1)) is False
    result = provisioner.results["node-b"]
    assert result["failed"] == "pull an image" and result["reason"] == "pull error"
    assert result["left"].endswith("docker pull img:b")


def test_a_node_already_on_the_model_needs_no_assignment(provisioner):
    provisioner.results["node-b"]["probe"] = {"model": "org/model"}
    assert provisioner._step_assign("node-b", session(admin=1)) is True
    assert provisioner.results["node-b"]["model"] == "org/model"


def test_a_failed_assignment_says_how_to_set_the_model_by_hand(provisioner):
    assert provisioner._step_assign("node-b", session(admin=1)) is False
    result = provisioner.results["node-b"]
    assert result["failed"] == "assign the model" and result["reason"] == "set error"
    assert result["left"] == "ragon-admin node set node-b --model org/model"


def test_a_name_the_resolver_cannot_find_is_not_resolved(monkeypatch):
    def refuse(*args, **kwargs):
        raise OSError("no such host")

    monkeypatch.setattr(node_provisioner.socket, "getaddrinfo", refuse)
    assert NodeProvisioner._resolves("node-b.local") is False
    monkeypatch.setattr(node_provisioner.socket, "getaddrinfo", lambda *a, **k: [("addr",)])
    assert NodeProvisioner._resolves("node-b.local") is True


def test_waiting_for_the_server_returns_the_model_once_it_answers(provisioner, monkeypatch):
    answers = iter([None, "org/model"])
    monkeypatch.setattr(provisioner, "_served_model", lambda host, port: next(answers))
    assert provisioner._wait_for_model("node-b", 8000) == "org/model"


def test_waiting_for_the_server_gives_up_at_the_deadline(provisioner, monkeypatch):
    provisioner.options["start_timeout"] = 1
    clock = iter([0.0, 0.5, 5.0])
    monkeypatch.setattr(node_provisioner.time, "monotonic", lambda: next(clock))
    monkeypatch.setattr(provisioner, "_served_model", lambda host, port: None)
    assert provisioner._wait_for_model("node-b", 8000) is None
