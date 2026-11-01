"""Branch tests for the docs egress audit's verdict, its step timeout and its two refusals."""

import subprocess
from unittest.mock import MagicMock

from dreamference.audit import DocsEgressAudit
from dreamference.audit import docs_egress_audit as module
from dreamference.audit.egress_trace import EgressTrace
from dreamference.audit.egress_verdict import FAIL, PASS, TRACE_FAILED


def _trace(**fields) -> EgressTrace:
    return EgressTrace(processes={"ragon-docs": 1}, **fields)


def test_a_clean_trace_of_a_run_that_did_its_work_passes():
    assert DocsEgressAudit.judge(_trace(), indexed=True, found=True).status == PASS


def test_a_trace_with_no_process_is_a_trace_failure_before_anything_else():
    verdict = DocsEgressAudit.judge(EgressTrace(), indexed=True, found=True)
    assert verdict.status == TRACE_FAILED and "no process" in verdict.problems[0]


def test_a_run_that_indexed_nothing_cannot_pass_on_an_empty_trace():
    verdict = DocsEgressAudit.judge(_trace(), indexed=False, found=True)
    assert verdict.status == TRACE_FAILED and "did not index" in verdict.problems[0]


def test_a_search_that_missed_the_answer_cannot_pass_on_an_empty_trace():
    verdict = DocsEgressAudit.judge(_trace(), indexed=True, found=False)
    assert verdict.status == TRACE_FAILED and "did not find" in verdict.problems[0]


def test_a_destination_fails_even_when_the_run_did_not_index():
    verdict = DocsEgressAudit.judge(
        _trace(destinations={"1.2.3.4:443": 2}), indexed=False, found=False
    )
    assert verdict.status == FAIL
    assert verdict.problems == ["connected or sent to 1.2.3.4:443 (2x)"]


def test_dns_names_servers_and_networked_git_each_fail_the_trace():
    verdict = DocsEgressAudit.judge(
        _trace(
            dns_servers={"9.9.9.9:53": 1},
            dns_names={"example.org": 3},
            networked_git=["git fetch origin"],
        ),
        indexed=True,
        found=True,
    )
    assert verdict.status == FAIL and len(verdict.problems) == 3


def test_a_step_that_outlives_the_timeout_has_its_whole_group_killed(monkeypatch):
    process = MagicMock(pid=4242)
    process.communicate.side_effect = [
        subprocess.TimeoutExpired("strace", 1),
        subprocess.TimeoutExpired("strace", 10),
    ]
    monkeypatch.setattr(subprocess, "Popen", MagicMock(return_value=process))
    killed = MagicMock()
    monkeypatch.setattr(module.os, "killpg", killed)
    outcome = DocsEgressAudit.step(["strace", "ragon-docs"], "/tmp", {})
    killed.assert_called_once_with(4242, module.signal.SIGKILL)
    assert outcome == f"did not finish within {module.RUN_TIMEOUT_S} s"


def test_a_step_whose_group_is_already_gone_still_reports_the_timeout(monkeypatch):
    process = MagicMock(pid=1)
    process.communicate.side_effect = [subprocess.TimeoutExpired("s", 1), ("", "")]
    monkeypatch.setattr(subprocess, "Popen", MagicMock(return_value=process))
    monkeypatch.setattr(module.os, "killpg", MagicMock(side_effect=ProcessLookupError))
    assert "did not finish" in DocsEgressAudit.step(["x"], "/tmp", {})


def test_a_finished_step_returns_both_streams(monkeypatch):
    process = MagicMock()
    process.communicate.return_value = ("out;", "err")
    monkeypatch.setattr(subprocess, "Popen", MagicMock(return_value=process))
    assert DocsEgressAudit.step(["x"], "/tmp", {}) == "out;err"


def test_without_strace_the_audit_is_a_trace_failure_and_runs_nothing(monkeypatch, capsys):
    monkeypatch.setattr(module.shutil, "which", lambda name: None)
    trace = MagicMock()
    monkeypatch.setattr(DocsEgressAudit, "trace", trace)
    assert DocsEgressAudit.run(ragon_docs="/bin/true") == 2
    assert "strace is not installed" in capsys.readouterr().out
    trace.assert_not_called()


def test_a_ragon_docs_that_is_not_executable_is_a_trace_failure(monkeypatch, tmp_path, capsys):
    monkeypatch.setattr(module.shutil, "which", lambda name: "/usr/bin/strace")
    trace = MagicMock()
    monkeypatch.setattr(DocsEgressAudit, "trace", trace)
    assert DocsEgressAudit.run(ragon_docs=str(tmp_path / "missing")) == 2
    assert "ragon-docs is not built" in capsys.readouterr().out
    trace.assert_not_called()
