"""The Mac node's acceptance gate scripts, `scripts/mac_node_gate.sh` and its client twin.

Dry runs against a stand-in engine on Linux (specs/DREAMFERENCE_MAC_NODE.md §8, §9): the stand-in
answers `/v1/models` on an ephemeral loopback port, every other command the scripts name is
listed, not run, and evidence files stand in for the engine's gate runs.
"""

import http.server
import os
import re
import subprocess
import threading
from collections.abc import Iterator
from pathlib import Path

import pytest

REPO = Path(__file__).resolve().parent.parent
GATE = REPO / "scripts" / "mac_node_gate.sh"
TWIN = REPO / "scripts" / "mac_node_gate_client.sh"
ENGINE_CHECKS = {"G4", "G5", "G6", "G7", "G8", "G15", "G19", "G20"}
ALL_CHECKS = [f"G{n}" for n in range(1, 21)]
ROW = re.compile(r"^\| (G\d+) \| (\w+) \| (.*) \|$", re.MULTILINE)

# The spec's thresholds; a change here is a visible loosening or tightening (spec §8).
THRESHOLDS = {
    "GATE_MEMORY_GB": "128",
    "M3_HOST_GAP_MS_MAX": "1",
    "M3_DECODE_BYTE_MODEL_FRACTION_MIN": "0.90",
    "M4_PREFILL_PROMPT_TOKENS_MIN": "35000",
    "L3_TOP1_FLIPS_MAX": "85",
    "L3_FLIP_GAP_NATS_MAX": "1.75",
    "L3_TOP5_AGREEMENT_MIN": "1.0",
    "L3_MEAN_KL_NATS_MAX": "0.0008",
    "L3_PPL_RATIO_MIN": "0.999",
    "L3_PPL_RATIO_MAX": "1.001",
    "M5_MEMORY_GROWTH_GB_MAX": "1",
    "M7_SOAK_HOURS": "8",
    "M7_THERMAL_MINUTES": "60",
    "G20_MCNEMAR_ALPHA": "0.05",
    "G1_HEALTH_LIMIT_S": '""',
    "G7_TURN_LIMIT_S": '""',
    "G16_RESTART_LIMIT_S": '""',
}

GOOD_G6 = (
    "candidate=abc123\ntop1_flips=60\nmax_flip_gap_nats=1.2\ntop5_agreement=1.0\n"
    "mean_kl_nats=0.0005\nkl_grows_with_position=0\nppl_ratio=1.0004\nacceptance_within_spread=1\n"
)


class _StandIn(http.server.BaseHTTPRequestHandler):
    def do_GET(self) -> None:
        body = b'{"object":"list","data":[{"id":"stand-in","object":"model"}]}'
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args: object) -> None:
        return


@pytest.fixture
def stand_in() -> Iterator[str]:
    """A stand-in engine answering `/v1/models` on an ephemeral loopback port."""
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), _StandIn)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    yield f"http://127.0.0.1:{server.server_address[1]}"
    server.shutdown()
    server.server_close()


def _env(tmp_path: Path) -> dict[str, str]:
    env = dict(os.environ, PATH="/usr/bin:/bin", TMPDIR=str(tmp_path))
    for name in ("RAGON", "RAGON_ADMIN", "RAGON_SEARCH", "SSH", "SSH_KEYGEN"):
        env[name] = "/bin/false"
    return env


def _gate(tmp_path: Path, *args: str) -> tuple[int, dict[str, tuple[str, str]], str]:
    report = tmp_path / "report.md"
    done = subprocess.run(
        ["bash", str(GATE), "--report", str(report), *args],
        cwd=tmp_path,
        env=_env(tmp_path),
        capture_output=True,
        text=True,
        timeout=60,
    )
    text = report.read_text()
    rows = {g: (status, detail) for g, status, detail in ROW.findall(text)}
    return done.returncode, rows, text


def _twin(tmp_path: Path, *args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["bash", str(TWIN), "--node", "gate-box", *args],
        cwd=tmp_path,
        env=_env(tmp_path),
        capture_output=True,
        text=True,
        timeout=60,
    )


def test_scripts_parse_and_avoid_bash4() -> None:
    """Both scripts parse and keep to macOS's /bin/bash 3.2."""
    for script in (GATE, TWIN):
        subprocess.run(["bash", "-n", str(script)], check=True)
        text = "\n".join(
            row for row in script.read_text().splitlines() if not row.lstrip().startswith("#")
        )
        for construct in (
            "declare -A",
            "mapfile",
            "readarray",
            ",,}",
            "^^}",
            "date -d",
            "python",
            "jq ",
        ):
            assert construct not in text, (script.name, construct)
        assert os.access(script, os.X_OK), script.name


def test_thresholds_are_the_specs() -> None:
    """Each threshold constant equals the spec's value, so a loosening shows in this file."""
    text = GATE.read_text()
    found = dict(re.findall(r"^([A-Z][A-Z0-9_]+)=(\S+)", text, re.MULTILINE))
    for name, value in THRESHOLDS.items():
        assert found.get(name) == value, name


def test_preview_dry_run_skips_the_engine_checks_with_a_reason(
    tmp_path: Path, stand_in: str
) -> None:
    """The acceptance dry run: 20 rows, the eight engine checks skipped with a reason, exit 0."""
    code, rows, text = _gate(tmp_path, "--dry-run", "--engine-url", stand_in)
    assert code == 0, text
    assert list(rows) == ALL_CHECKS
    for check, (status, detail) in rows.items():
        if check in ENGINE_CHECKS:
            assert status == "skip"
            assert "S3 preview staging" in detail
        else:
            assert status == "dry", (check, detail)
    assert "DRY RUN" in text
    assert "answers /v1/models" in rows["G1"][1]


def test_one_report_per_run(tmp_path: Path, stand_in: str) -> None:
    """Without --report the run writes exactly one report in the working folder and cleans up."""
    subprocess.run(
        ["bash", str(GATE), "--dry-run", "--engine-url", stand_in],
        cwd=tmp_path,
        env=_env(tmp_path),
        capture_output=True,
        check=True,
        timeout=60,
    )
    assert [p.name for p in tmp_path.iterdir() if p.name.startswith("mac-node-gate")] == [
        next(p.name for p in tmp_path.glob("mac-node-gate-*.md"))
    ]


def test_an_engine_that_does_not_answer_fails_the_run(tmp_path: Path) -> None:
    """A failure exits non-zero and the report still has all twenty rows."""
    code, rows, text = _gate(tmp_path, "--dry-run", "--engine-url", "http://127.0.0.1:9")
    assert code == 1
    assert list(rows) == ALL_CHECKS
    assert rows["G1"][0] == "fail"
    assert "FAIL" in text


def test_a_failing_client_fails_the_run(tmp_path: Path, stand_in: str) -> None:
    """A client twin's failed line fails its check; a missing client row fails too."""
    results = tmp_path / "client.tsv"
    results.write_text("G3\tfail\ta wrong key was accepted\tlinux\nG9\tpass\tok\tlinux\n")
    code, rows, _ = _gate(
        tmp_path, "--dry-run", "--engine-url", stand_in, "--client-results", str(results)
    )
    assert code == 1
    assert rows["G3"][0] == "fail"
    assert rows["G9"][0] == "pass"
    assert rows["G10"] == ("fail", "no result from the client twin")


def test_full_stage_runs_the_engine_checks_against_their_evidence(
    tmp_path: Path, stand_in: str
) -> None:
    """At --stage full nothing is skipped; evidence inside the bounds passes, outside fails."""
    evidence = tmp_path / "evidence"
    evidence.mkdir()
    (evidence / "G6.env").write_text(GOOD_G6)
    (evidence / "G4.env").write_text(
        "host_gap_ms=1.4\ndecode_byte_model_fraction=0.93\nspec_equals_plain=1\nrepeat_identical=1\n"
    )
    (evidence / "G20.env").write_text("mcnemar_p=0.01\nmac_resolved=10\ngb10_resolved=20\n")
    code, rows, _ = _gate(
        tmp_path,
        "--dry-run",
        "--stage",
        "full",
        "--engine-url",
        stand_in,
        "--evidence",
        str(evidence),
        "--candidate",
        "abc123",
    )
    assert code == 1
    assert not any(status == "skip" for status, _ in rows.values())
    assert rows["G6"][0] == "pass", rows["G6"]
    assert rows["G4"][0] == "fail"
    assert "host_gap_ms=1.4>1" in rows["G4"][1]
    assert rows["G20"][0] == "fail"
    assert rows["G5"][0] == "dry"


def test_l3_evidence_for_another_candidate_fails(tmp_path: Path, stand_in: str) -> None:
    """G6 checks the L3 gate ran on this release candidate, not just that one passed."""
    evidence = tmp_path / "evidence"
    evidence.mkdir()
    (evidence / "G6.env").write_text(GOOD_G6)
    _, rows, _ = _gate(
        tmp_path,
        "--dry-run",
        "--stage",
        "full",
        "--engine-url",
        stand_in,
        "--evidence",
        str(evidence),
    )
    assert rows["G6"][0] == "fail"
    assert "candidate=abc123" in rows["G6"][1]


def test_a_real_run_off_macos_is_refused(tmp_path: Path) -> None:
    """Without --dry-run the gate runs only on macOS."""
    done = subprocess.run(
        ["bash", str(GATE)], cwd=tmp_path, env=_env(tmp_path), capture_output=True, text=True
    )
    assert done.returncode == 2
    assert "--dry-run" in done.stderr
    assert not list(tmp_path.glob("mac-node-gate-*.md"))


def test_twin_dry_run_lists_every_client_check(tmp_path: Path) -> None:
    """The twin's dry run prints one tab-separated line per client check and runs nothing."""
    done = _twin(tmp_path, "--dry-run")
    assert done.returncode == 0
    lines = [line.split("\t") for line in done.stdout.splitlines()]
    assert [fields[0] for fields in lines] == [
        "G2",
        "G3",
        "G7",
        "G9",
        "G10",
        "G11",
        "G12",
        "G13",
        "G14",
    ]
    assert {fields[1] for fields in lines} == {"dry"}
    assert {fields[3] for fields in lines} == {"linux"}


def test_twin_fails_on_evidence_out_of_line(tmp_path: Path) -> None:
    """A Night Shift run that pushed fails G11; a missing evidence file fails G13."""
    evidence = tmp_path / "evidence"
    evidence.mkdir()
    (evidence / "G11.env").write_text(
        "commit_in_worktree=1\nattempts=3\nreport=1\nmerged=0\npushed=1\n"
    )
    done = _twin(tmp_path, "--evidence", str(evidence), "--checks", "G11 G13")
    assert done.returncode == 1
    g11, g13 = (line.split("\t") for line in done.stdout.splitlines())
    assert g11[1] == "fail" and "pushed=1!=0" in g11[2]
    assert g13[1] == "fail" and "no evidence file" in g13[2]
