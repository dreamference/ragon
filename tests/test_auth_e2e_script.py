"""The accounts end-to-end gate script (specs/DREAMFERENCE_AUTH.md §17, tail row 6).

Dry runs, plus real runs against a fake `ssh` on PATH that records every call (no machine is
touched), and the node helper's pure parts.
"""

import importlib.util
import os
import py_compile
import subprocess
import sys
from pathlib import Path
from types import ModuleType
from typing import ClassVar

import pytest

pytestmark = pytest.mark.skipif(sys.platform == "win32", reason="POSIX shell script")
SCRIPT = Path(__file__).resolve().parent.parent / "scripts" / "auth_e2e.sh"
HELPER = SCRIPT.with_name("auth_e2e_node.py")
STEPS = [
    "E0a",
    "E0b",
    "E0c",
    "E0d",
    "E1",
    "E2",
    "E3",
    "E4",
    "E5",
    "E6",
    "E7",
    "E8",
    "E9",
    "E10",
    "E11",
    "E12",
    "E13",
    "E14",
]


# A fake ssh: records the host and the command, swallows stdin (the staged files), and answers
# like a node would, with credentials in its output so the masking can be checked.
FAKE_SSH = """#!/usr/bin/env bash
host=""; tty=""
for a in "$@"; do
  case "$a" in
    -tt) tty=" [tty]" ;;
    -o|BatchMode=*|ConnectTimeout=*) ;;
    *) [ -z "$host" ] && host="$a" || cmd="$a" ;;
  esac
done
printf '%s\\t%s%s\\n' "$host" "$cmd" "$tty" >> "$FAKE_SSH_LOG"
cat > /dev/null
case "$cmd" in
  *"--version"*) echo "ragon-admin 0.9.9-test" ;;
  *" unauth "*) echo "ok   a bad credential gets 401"; echo "refusals_rise=1" ;;
  *" sdk "*) echo "Authorization: Bearer abc.def-ghi"; echo "key sk-ragon-AbC123_x"; echo "id eyJhbGc.eyJzdWIi.c2ln" ;;
esac
case "$cmd" in *"$FAKE_SSH_FAIL"*) [ -n "$FAKE_SSH_FAIL" ] && exit 7 ;; esac
exit 0
"""


def _run(*args: str, env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["bash", str(SCRIPT), *args],
        capture_output=True,
        text=True,
        timeout=60,
        check=False,
        env=env,
    )


def _fake(tmp_path: Path, fail: str = "") -> tuple[dict[str, str], Path, Path]:
    """A PATH with the fake ssh first, its call log, and an evidence folder with the person steps."""
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    (bin_dir / "ssh").write_text(FAKE_SSH)
    (bin_dir / "ssh").chmod(0o755)
    calls = tmp_path / "ssh-calls.log"
    evidence = tmp_path / "evidence"
    evidence.mkdir()
    for person in ("E7", "E11"):
        (evidence / f"{person}.env").write_text("RESULT=pass\n")
    env = {
        **os.environ,
        "PATH": f"{bin_dir}{os.pathsep}{os.environ['PATH']}",
        "FAKE_SSH_LOG": str(calls),
        "FAKE_SSH_FAIL": fail,
    }
    return env, calls, evidence


def _real(tmp_path: Path, fail: str = "") -> tuple[subprocess.CompletedProcess[str], str, Path]:
    env, calls, evidence = _fake(tmp_path, fail)
    done = _run(
        "--primary",
        "prim",
        "--secondary",
        "sec",
        "--evidence",
        str(evidence),
        "--any-time",
        env=env,
    )
    return done, calls.read_text(), evidence


def _helper() -> ModuleType:
    spec = importlib.util.spec_from_file_location("auth_e2e_node", HELPER)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_a_dry_run_lists_every_step_and_is_not_a_pass() -> None:
    done = _run("--primary", "a", "--secondary", "b", "--dry-run")
    assert done.returncode == 0
    for step in STEPS:
        assert f"[plan] {step}:" in done.stdout
    assert "not a gate pass" in done.stdout


def test_usage_errors_exit_two() -> None:
    assert _run("--primary", "a").returncode == 2
    assert _run("--primary", "a", "--secondary", "a").returncode == 2


def test_a_real_run_outside_the_gpu_window_is_blocked() -> None:
    done = _run("--primary", "a", "--secondary", "b", "--now", "2026-11-20 21:00")
    assert done.returncode == 3
    assert "GPU window" in done.stderr


def test_any_time_skips_the_window_check_and_says_so() -> None:
    done = _run(
        "--primary",
        "no-such-host.invalid",
        "--secondary",
        "b",
        "--now",
        "2026-11-20 21:00",
        "--any-time",
    )
    assert done.returncode == 3
    assert "GPU window" not in done.stderr
    assert "window check skipped (--any-time)" in done.stderr
    assert "unreachable" in done.stderr


def test_a_real_run_with_an_unreachable_node_is_blocked() -> None:
    done = _run(
        "--primary", "no-such-host.invalid", "--secondary", "b", "--now", "2026-11-20 12:00"
    )
    assert done.returncode == 3
    assert "unreachable" in done.stderr


def test_the_dry_run_plan_names_the_new_checks() -> None:
    plan = _run("--primary", "a", "--secondary", "b", "--dry-run").stdout
    for needle in (
        "node user add e2e-",
        " ca ",
        " signin ",
        " pty ",
        " device ",
        " key ",
        " sdk ",
        " revoke-key ",
        " web ",
        " forgejo ",
        " night ",
        " unauth ",
        " logout ",
        "E1.env",
        "E8.env",
        "WEAK",
    ):
        assert needle in plan, needle
    # E4's logout --all runs after every other primary step.
    lines = [line for line in plan.splitlines() if line.startswith("[plan] ")]
    assert lines[-1].startswith("[plan] E4:")


def test_a_dry_run_touches_no_machine(tmp_path: Path) -> None:
    env, calls, _ = _fake(tmp_path)
    done = _run("--primary", "a", "--secondary", "b", "--dry-run", env=env)
    assert done.returncode == 0
    assert not calls.exists()


def test_the_script_has_a_cleanup_trap_for_the_test_user() -> None:
    text = SCRIPT.read_text()
    assert "trap 'rc=$?; cleanup; exit $rc' EXIT" in text
    assert "ragon-admin node user remove $TEST_USER" in text


def test_a_green_fake_run_writes_masked_evidence_and_removes_the_user(tmp_path: Path) -> None:
    done, calls, evidence = _real(tmp_path)
    assert done.returncode == 0, done.stdout + done.stderr
    assert "GREEN" in done.stdout
    for step in [s for s in STEPS if s not in ("E7", "E11", "E14")]:
        assert (evidence / f"{step}.log").is_file(), step
    for node in ("prim", "sec"):
        assert (evidence / f"E14-{node}.log").is_file()
    assert "--- exit: 0" in (evidence / "E5.log").read_text()
    for driven in ("E1", "E8"):
        assert "RESULT=pass" in (evidence / f"{driven}.env").read_text()
    sdk = (evidence / "E6.log").read_text()
    assert "Bearer ***" in sdk and "sk-ragon-***" in sdk and "eyJ***" in sdk
    for secret in ("abc.def-ghi", "AbC123_x", "eyJhbGc.eyJzdWIi"):
        assert secret not in sdk
    summary = (evidence / "summary.txt").read_text()
    assert "ragon-admin 0.9.9-test" in summary and "passed" in summary and "at: 20" in summary
    assert "window: any time (--any-time" in summary
    # The user is created on the primary, removed on exit, and the run folders go on both nodes.
    assert "ragon-admin node user add e2e-" in calls
    last = calls.splitlines()
    assert any(line.startswith("prim\t") and "node user remove e2e-" in line for line in last)
    assert sum("rm -rf /tmp/ragon-auth-e2e." in line for line in last) == 2
    # The password never appears in argv: only file reads of the run folder.
    assert "--password " not in calls


def test_the_whoami_steps_run_on_a_pty(tmp_path: Path) -> None:
    # `ragon whoami` refuses to run without a terminal; the helper's signin runs it too.
    _, calls, _ = _real(tmp_path)
    lines = calls.splitlines()
    whoami = [line for line in lines if "ragon whoami" in line]
    signin = [line for line in lines if " signin " in line]
    assert len(whoami) == 1 and len(signin) >= 3
    assert all(line.endswith("[tty]") for line in whoami + signin)
    # Steps that do not need a terminal keep plain ssh.
    assert not any(line.endswith("[tty]") for line in lines if "ragon-admin node user list" in line)


def test_e13_revokes_a_key_of_the_test_user(tmp_path: Path) -> None:
    _, calls, _ = _real(tmp_path)
    lines = calls.splitlines()
    revoke = [line for line in lines if "--revoke e2e-sdk" in line and "sleep 60" in line]
    assert len(revoke) == 1
    assert revoke[0].startswith("sec\t") and "node user key e2e-" in revoke[0]
    assert "user key e2e " not in calls


def test_a_red_fake_run_still_removes_the_test_user(tmp_path: Path) -> None:
    done, calls, evidence = _real(tmp_path, fail=" device ")
    assert done.returncode == 1
    assert "[FAIL] E5:" in done.stdout and "RED" in done.stdout
    assert "--- exit: 7" in (evidence / "E5.log").read_text()
    assert "node user remove e2e-" in calls


def test_a_stale_driven_pass_does_not_carry_over(tmp_path: Path) -> None:
    env, _, evidence = _fake(tmp_path, fail=" pty ")
    (evidence / "E1.env").write_text("RESULT=pass\n")
    done = _run(
        "--primary",
        "prim",
        "--secondary",
        "sec",
        "--evidence",
        str(evidence),
        "--any-time",
        env=env,
    )
    assert "[FAIL] E1:" in done.stdout


def test_e14_needs_the_refusal_count_to_rise_on_both_nodes(tmp_path: Path) -> None:
    done, _, _ = _real(tmp_path, fail=" unauth ")
    assert "[FAIL] E14:" in done.stdout


def test_the_node_helper_compiles_and_its_checks_hold(tmp_path: Path) -> None:
    py_compile.compile(str(HELPER), cfile=str(tmp_path / "helper.pyc"), doraise=True)
    helper = _helper()
    masked = helper.mask(
        'Bearer abc.def {"access_token": "xyz", "password": "pw"} sk-ragon-K1 eyJa.b.c'
    )
    for secret in ("abc.def", "xyz", '"pw"', "K1", "eyJa.b"):
        assert secret not in masked, secret
    assert helper.error_shape(
        {"error": {"message": "m", "type": "invalid_request_error", "param": None, "code": "x"}}
    )
    assert not helper.error_shape({"detail": "no"})
    rows = (
        "2026-11-24\tnode1\tcodex\tsrc-a\t3 requests\n2026-11-24\tnode2\tragon\tsrc-b\t4 requests\n"
    )
    assert helper.unauth_rows(rows) == 7
    assert helper.tampered("abc") != "abc"
    assert helper.main(["auth_e2e_node.py", "nope", str(tmp_path)]) == 2
    assert helper.main(["auth_e2e_node.py", "refusals", str(tmp_path / "missing")]) == 1


def test_changes_to_ragon_web_carry_its_origin_and_reads_do_not(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    helper = _helper()
    sent = []

    class Answer:
        status = 200
        headers: ClassVar[dict[str, str]] = {}

        def read(self) -> bytes:
            return b"{}"

    def urlopen(request, timeout=None, context=None):
        sent.append((request.get_method(), request.full_url, request.get_header("Origin")))
        return Answer()

    monkeypatch.setattr(helper.urllib.request, "urlopen", urlopen)
    run = helper.Run(str(tmp_path))
    run.http("POST", f"{helper.WEB}/api/auth/device", {"user_code": "x"})
    run.http("GET", f"{helper.WEB}/signin")
    run.http("POST", f"{helper.ACCOUNTS}/oauth/token", {"a": "b"}, form=True)
    assert sent == [
        ("POST", f"{helper.WEB}/api/auth/device", helper.WEB),
        ("GET", f"{helper.WEB}/signin", None),
        ("POST", f"{helper.ACCOUNTS}/oauth/token", None),
    ]


def test_the_first_run_screen_answers_the_line_prompts_and_the_sdk_python_is_the_operators() -> None:
    source = HELPER.read_text()
    for prompt in ("Choose 1-4: ", "Username: ", "Confirm password: ", "[y/N] "):
        assert prompt in source, prompt
    script = SCRIPT.read_text()
    assert 'SDK="${AUTH_E2E_SDK_PYTHON:-python3} -I $RUN/auth_e2e_node.py sdk $RUN"' in script
