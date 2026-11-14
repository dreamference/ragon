"""One gradable task candidate drawn from a repository's history."""

from dataclasses import dataclass


@dataclass(frozen=True)
class TaskCandidate:
    """One commit that makes a gradable task.

    Attributes:
        commit: The commit sha (the reference solution).
        parent: Its first parent's sha (the state the task starts from).
        message: The full commit message.
        committed_at: The commit date, seconds since the epoch.
        tests: The test files the commit touches (the grading).
        files: Every file the commit touches.
        changed_lines: Lines added plus lines deleted.
    """

    commit: str
    parent: str
    message: str
    committed_at: int
    tests: tuple[str, ...]
    files: tuple[str, ...]
    changed_lines: int
