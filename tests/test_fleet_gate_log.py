"""The fleet log is named by node id with only program names, and the gate stamps UTC with Z."""

import datetime
import re
import subprocess
from pathlib import Path

from dreamference.node.fleet_session import FleetSession
from dreamference.vllm_server.model_gate_service import ModelGateService


def test_fleet_log_path_uses_node_id_and_keeps_output_in_an_artefact(tmp_path: Path) -> None:
    session = FleetSession(
        "secret-host", "owner", tmp_path / "run", log_path=tmp_path / "x-pending.log"
    )
    result = subprocess.CompletedProcess(
        ["/usr/bin/ssh", "secret-host", "ls"], 0, "OUTPUT-TEXT", ""
    )
    result.elapsed_s = 1.25  # type: ignore[attr-defined]
    session._log(["/usr/bin/ssh", "secret-host", "ls"], result)
    session.name_log(f"{datetime.date.today().isoformat()}-node-abc")
    log = tmp_path / f"{datetime.date.today().isoformat()}-node-abc.log"
    assert session.log_path == log
    assert "secret-host" not in log.name
    text = log.read_text()
    assert text == "ssh -> exit 0 in 1.2s\n" or text == "ssh -> exit 0 in 1.3s\n"
    assert "OUTPUT-TEXT" not in text and "secret-host" not in text
    assert "OUTPUT-TEXT" in log.with_suffix(".out").read_text()
    assert not (tmp_path / "x-pending.log").exists()


def test_gate_log_timestamp_is_utc_with_z(capsys) -> None:
    ModelGateService.log("hello")
    line = capsys.readouterr().err
    assert re.match(r"\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ model gate: hello\n", line)
