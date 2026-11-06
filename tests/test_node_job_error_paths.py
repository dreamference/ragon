"""NodeJob's OSError and edge paths: a log that cannot be read, a git that cannot start, a follow."""

import subprocess

import pytest

from dreamference.node import node_job
from dreamference.node.node_job import NodeJob


def test_a_command_that_cannot_start_exits_127_and_says_why(tmp_path):
    sink = (tmp_path / "out.log").open("wb")
    code = NodeJob._run(["/nonexistent/binary-for-test"], sink, tmp_path)
    sink.close()
    assert code == 127
    assert b"could not start" in (tmp_path / "out.log").read_bytes()


def test_git_with_a_git_dir_flag_and_with_a_tree_are_routed_apart(monkeypatch):
    calls: list[tuple] = []

    def fake_run(rest, *args, **kwargs):
        calls.append((rest, args, kwargs))
        return subprocess.CompletedProcess(rest, 0, "", "")

    monkeypatch.setattr(node_job.SafeGit, "run", staticmethod(fake_run))
    NodeJob._git([node_job.GIT_DIR_FLAG, "/repo", "status"])
    NodeJob._git(["-C", "/tree", "log"])
    assert calls[0] == (["status"], (), {"git_dir": "/repo"})
    assert calls[1] == (["log"], ("/tree",), {})


def test_a_worktree_without_a_readable_git_file_has_no_admin_dir(tmp_path):
    assert NodeJob._admin_dir(tmp_path, tmp_path) is None


def test_a_missing_log_does_not_show_a_missing_module(tmp_path):
    assert NodeJob.missing_module(tmp_path / "absent.log") is False


def test_the_user_bus_environment_falls_back_to_the_run_directory(monkeypatch):
    monkeypatch.delenv("XDG_RUNTIME_DIR", raising=False)
    monkeypatch.setattr(node_job.Path, "is_dir", lambda self: True)
    environment = NodeJob.user_bus_environment()
    assert environment["XDG_RUNTIME_DIR"].startswith("/run/user/")


def _commit_record() -> dict:
    return {"id": "j1", "author": {"name": "n", "email": "e@x"}, "command": ["echo", "hi"]}


@pytest.mark.parametrize("failing_step", ["add", "commit"])
def test_a_failed_commit_step_leaves_a_note_and_no_branch(monkeypatch, tmp_path, failing_step):
    def fake_git(cls, arguments):
        if "status" in arguments:
            return subprocess.CompletedProcess(arguments, 0, " M f\n", "")
        failed = failing_step in arguments
        return subprocess.CompletedProcess(
            arguments, 1 if failed else 0, "", "boom" if failed else ""
        )

    monkeypatch.setattr(NodeJob, "_git", classmethod(fake_git))
    record = _commit_record()
    assert NodeJob._commit(record, tmp_path, tmp_path / "admin") is None
    assert record["note"].startswith("git ") and "boom" in record["note"]


def test_follow_reads_the_log_and_reports_the_note_of_a_failed_job(monkeypatch, tmp_path, capsys):
    log = tmp_path / "output.log"
    log.write_bytes(b"hello\n")
    monkeypatch.setattr(NodeJob, "job_dir", classmethod(lambda cls, job_id: tmp_path))
    states = [{"status": "running"}, {"status": "failed", "note": "it broke", "exit_code": 3}]
    monkeypatch.setattr(
        NodeJob,
        "reconcile",
        classmethod(lambda cls, job_id: states.pop(0) if len(states) > 1 else states[0]),
    )
    monkeypatch.setattr(node_job.time, "sleep", lambda s: None)
    assert NodeJob.follow("j1", poll_s=0) != 0
    captured = capsys.readouterr()
    assert "it broke" in captured.err


def test_follow_survives_an_unreadable_log(monkeypatch, tmp_path):
    monkeypatch.setattr(NodeJob, "job_dir", classmethod(lambda cls, job_id: tmp_path))
    monkeypatch.setattr(NodeJob, "reconcile", classmethod(lambda cls, job_id: {"status": "done"}))
    assert NodeJob.follow("j1", poll_s=0) == 0


def test_prune_after_gives_no_time_for_a_record_without_one():
    assert NodeJob.prune_after({"status": "done"}) is None
    assert NodeJob.prune_after({"status": "done", "finished": "not a time"}) is None
    assert (
        NodeJob.prune_after({"status": "done", "finished": "2026-01-01T00:00:00+00:00"}) is not None
    )


def test_a_sandbox_probe_that_cannot_run_names_the_reason(monkeypatch):
    def refuse(*args, **kwargs):
        raise OSError("no bwrap")

    monkeypatch.setattr(node_job.subprocess, "run", refuse)
    monkeypatch.setattr(NodeJob, "user_bus_environment", classmethod(lambda cls: {}))
    monkeypatch.setattr(NodeJob, "USE_UNIT", True)
    assert "cannot sandbox a job: no bwrap" in NodeJob.sandbox_blocker()


def test_a_sandbox_probe_that_succeeds_has_no_blocker(monkeypatch):
    seen: list[list[str]] = []

    def fine(command, **kwargs):
        seen.append(command)
        return subprocess.CompletedProcess(command, 0, "", "")

    monkeypatch.setattr(node_job.subprocess, "run", fine)
    monkeypatch.setattr(NodeJob, "user_bus_environment", classmethod(lambda cls: {}))
    monkeypatch.setattr(NodeJob, "USE_UNIT", True)
    assert NodeJob.sandbox_blocker() is None
    assert seen[0][0] == "systemd-run"


def _stub_execute(monkeypatch, tmp_path, record, added, admin="unset"):
    records = {"j1": record}
    monkeypatch.setattr(NodeJob, "read", classmethod(lambda cls, job_id: records.get(job_id)))
    monkeypatch.setattr(NodeJob, "write", classmethod(lambda cls, rec: None))
    monkeypatch.setattr(NodeJob, "job_dir", classmethod(lambda cls, job_id: tmp_path))
    monkeypatch.setattr(NodeJob, "repo_path", classmethod(lambda cls, name: tmp_path / "repo"))
    monkeypatch.setattr(NodeJob, "_git", classmethod(lambda cls, arguments: added))
    monkeypatch.setattr(NodeJob, "_remove_tree", classmethod(lambda cls, repo, tree: None))
    if admin != "unset":
        monkeypatch.setattr(NodeJob, "_admin_dir", classmethod(lambda cls, tree, repo: admin))


def _queued() -> dict:
    return {
        "id": "j1",
        "status": node_job.STATUS_QUEUED,
        "repo": "r",
        "commit": "abc",
        "airgapped": "on",
    }


def test_a_worktree_that_cannot_be_added_fails_the_job_with_git_s_words(monkeypatch, tmp_path):
    record = _queued()
    _stub_execute(
        monkeypatch, tmp_path, record, subprocess.CompletedProcess([], 128, "", "fatal: bad")
    )
    assert NodeJob.execute("j1") == 1
    assert "git worktree add failed: fatal: bad" in record["note"]
    assert record["status"] == "failed"


def test_a_worktree_with_no_admin_folder_fails_the_job(monkeypatch, tmp_path):
    record = _queued()
    _stub_execute(
        monkeypatch, tmp_path, record, subprocess.CompletedProcess([], 0, "", ""), admin=None
    )
    assert NodeJob.execute("j1") == 1
    assert "no administrative folder" in record["note"]


def test_a_runner_bug_is_recorded_not_left_running(monkeypatch, tmp_path):
    record = _queued()
    _stub_execute(
        monkeypatch, tmp_path, record, subprocess.CompletedProcess([], 0, "", ""), admin=tmp_path
    )

    def broken(cls, *args, **kwargs):
        raise RuntimeError("kaboom")

    monkeypatch.setattr(NodeJob, "_environment", classmethod(broken))
    assert NodeJob.execute("j1") == 1
    assert record["note"] == "job runner error: kaboom"
    assert record["status"] == "failed"


def test_a_job_cancelled_while_it_ran_stays_cancelled(monkeypatch, tmp_path):
    record = _queued()
    _stub_execute(monkeypatch, tmp_path, record, subprocess.CompletedProcess([], 128, "", "x"))
    monkeypatch.setattr(
        NodeJob,
        "read",
        classmethod(
            lambda cls, job_id: (
                {**record, "status": node_job.STATUS_CANCELLED}
                if record["status"] != "queued"
                else record
            )
        ),
    )
    NodeJob.execute("j1")
    assert record["status"] == "cancelled"
