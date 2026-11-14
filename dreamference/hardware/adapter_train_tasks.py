"""Gradable task candidates drawn from a repository's history, for adapter training.

`AdapterTrainTasks` walks the first-parent history of a repository (read-only: `git log` and
nothing else) and keeps the commits that make a usable task: not a merge, not by a bot, a
prose message, source code changed together with a test file, a diff within 400 lines and 15
files, and no vendored or generated path. Each candidate carries the parent sha (the state the
task starts from), the message (the task statement) and the touched test files (the grading).
The newest 10% by commit date, at least 20 and at most 60, is the hold-out; a repository with
fewer than 50 candidates is refused with the reason.
"""

import re
import subprocess
from pathlib import Path, PurePosixPath
from typing import Final

from dreamference.hardware.adapter_task_candidate import TaskCandidate

MAX_CHANGED_LINES: Final[int] = 400
MAX_FILES: Final[int] = 15
MIN_CANDIDATES: Final[int] = 50
HOLDOUT_FRACTION: Final[float] = 0.10
HOLDOUT_MIN: Final[int] = 20
HOLDOUT_MAX: Final[int] = 60
MIN_SUBJECT_WORDS: Final[int] = 3
MIN_SUBJECT_LETTERS: Final[int] = 10

CODE_SUFFIXES: Final[frozenset[str]] = frozenset(
    {
        ".py", ".rs", ".go", ".js", ".jsx", ".ts", ".tsx", ".java", ".kt", ".c", ".cc", ".cpp",
        ".h", ".hpp", ".cs", ".rb", ".php", ".swift", ".scala", ".sh",
    }
)  # fmt: skip
TEST_FILE: Final[re.Pattern[str]] = re.compile(
    r"(^|/)(tests?|__tests__|spec)/|(^|/)test_[^/]*$|_tests?\.[a-z]+$|\.(test|spec)\.[a-z]+$"
)
EXCLUDED_PATH: Final[re.Pattern[str]] = re.compile(
    r"(^|/)(vendor|vendored|node_modules|third_party|dist|build|target|__pycache__|generated|gen)/"
    r"|\.min\.(js|css)$|_pb2(_grpc)?\.py$|\.pb\.go$|\.generated\.|\.lock$"
    r"|(^|/)(package-lock\.json|yarn\.lock|poetry\.lock|Cargo\.lock|go\.sum)$"
)
BOT_IDENTITY: Final[re.Pattern[str]] = re.compile(
    r"\[bot\]|(^|[-_ ])bot($|[-_ @])|dependabot|renovate", re.I
)
NOT_PROSE: Final[re.Pattern[str]] = re.compile(r"^(merge|revert|fixup!|squash!|wip)\b", re.I)
HEADER_FIELDS: Final[int] = 6
NUMSTAT_FIELDS: Final[int] = 3
RECORD: Final[str] = "\x01"
FIELD: Final[str] = "\x00"
LOG_FORMAT: Final[str] = "%x01%H%x00%P%x00%an%x00%ae%x00%ct%x00%B%x02"


class AdapterTrainTasks:
    """Finds task candidates in a repository's first-parent history."""

    @classmethod
    def is_test_path(cls, path: str) -> bool:
        """Say whether a path is a test file.

        Args:
            path: Repository-relative path with forward slashes.

        Returns:
            bool: True for files under a test folder or named like a test.
        """
        return TEST_FILE.search(path) is not None

    @classmethod
    def is_code_path(cls, path: str) -> bool:
        """Say whether a path is non-test source code.

        Args:
            path: Repository-relative path with forward slashes.

        Returns:
            bool: True for a source file that is not a test.
        """
        return PurePosixPath(path).suffix in CODE_SUFFIXES and not cls.is_test_path(path)

    @classmethod
    def is_prose(cls, message: str) -> bool:
        """Say whether a commit message reads as a description of a change.

        Args:
            message: The full commit message.

        Returns:
            bool: True when the subject has enough words and letters and is not a merge,
                revert, fixup or work-in-progress marker.
        """
        subject = message.strip().split("\n", 1)[0].strip()
        if NOT_PROSE.match(subject):
            return False
        letters = sum(ch.isalpha() for ch in subject)
        return len(subject.split()) >= MIN_SUBJECT_WORDS and letters >= MIN_SUBJECT_LETTERS

    @classmethod
    def is_bot(cls, name: str, email: str) -> bool:
        """Say whether an author is an automation account.

        Args:
            name: The author name.
            email: The author email.

        Returns:
            bool: True for names or addresses that look like a bot.
        """
        return BOT_IDENTITY.search(name) is not None or BOT_IDENTITY.search(email) is not None

    @classmethod
    def candidates(cls, repo: Path) -> list[TaskCandidate]:
        """Return every gradable candidate, oldest first.

        Args:
            repo: A git repository (read only; nothing is written in it).

        Returns:
            list[TaskCandidate]: Candidates in first-parent order, oldest first, ties kept in
                history order.

        Raises:
            RuntimeError: When git cannot read the repository.
        """
        out = cls._log(repo)
        found: list[TaskCandidate] = []
        for record in out.split(RECORD)[1:]:
            candidate = cls._parse(record)
            if candidate is not None:
                found.append(candidate)
        found.reverse()
        return found

    @classmethod
    def split(
        cls, candidates: list[TaskCandidate]
    ) -> tuple[tuple[TaskCandidate, ...], tuple[TaskCandidate, ...]]:
        """Hold out the newest candidates by commit date.

        Args:
            candidates: Candidates, in any order.

        Returns:
            tuple: Training tasks, then the newest 10% (20 to 60, never all) as hold-out.
        """
        ordered = sorted(candidates, key=lambda c: c.committed_at)
        size = min(HOLDOUT_MAX, max(HOLDOUT_MIN, round(len(ordered) * HOLDOUT_FRACTION)))
        size = min(size, max(0, len(ordered) - 1))
        cut = len(ordered) - size
        return tuple(ordered[:cut]), tuple(ordered[cut:])

    @classmethod
    def collect(cls, repo: Path) -> tuple[tuple[TaskCandidate, ...], tuple[TaskCandidate, ...]]:
        """Find candidates and split them, or refuse when there are too few.

        Args:
            repo: A git repository.

        Returns:
            tuple[tuple[TaskCandidate, ...], tuple[TaskCandidate, ...]]: The training tasks and the hold-out.

        Raises:
            RuntimeError: With the count and the minimum when fewer than 50
                candidates qualify.
        """
        found = cls.candidates(repo)
        if len(found) < MIN_CANDIDATES:
            raise RuntimeError(
                f"only {len(found)} gradable commits in the first-parent history "
                f"(non-merge, non-bot, prose message, code plus a test file, at most "
                f"{MAX_CHANGED_LINES} changed lines in {MAX_FILES} files, no vendored or "
                f"generated paths); at least {MIN_CANDIDATES} are needed"
            )
        return cls.split(found)

    @classmethod
    def _log(cls, repo: Path) -> str:
        """Run the one read-only git command.

        Args:
            repo: The repository.

        Returns:
            str: `git log` output with the record format and numstat.

        Raises:
            RuntimeError: When git fails.
        """
        try:
            done = subprocess.run(
                ["git", "-C", str(repo), "log", "--first-parent", "--no-merges", "--numstat",
                 "-z", "--no-renames", f"--format={LOG_FORMAT}"],
                capture_output=True, text=True, check=False, timeout=300,
            )  # fmt: skip
        except (OSError, subprocess.SubprocessError) as exc:
            raise RuntimeError(f"cannot run git in {repo}: {exc}") from exc
        if done.returncode != 0:
            raise RuntimeError(f"git log failed in {repo}: {done.stderr.strip()[:200]}")
        return done.stdout

    @classmethod
    def _parse(cls, record: str) -> TaskCandidate | None:
        """Turn one log record into a candidate, or None when it does not qualify.

        Args:
            record: The text after a record marker: the header fields, then numstat entries.

        Returns:
            TaskCandidate | None: The candidate, or None.
        """
        header, _, rest = record.partition("\x02")
        parts = header.split(FIELD)
        if len(parts) < HEADER_FIELDS:
            return None
        sha, parents, name, email, stamp, message = (
            parts[0],
            parts[1],
            parts[2],
            parts[3],
            parts[4],
            FIELD.join(parts[5:]),
        )
        parent_list = parents.split()
        if len(parent_list) != 1 or cls.is_bot(name, email) or not cls.is_prose(message):
            return None
        stats = cls._numstat(rest)
        if stats is None:
            return None
        files = tuple(path for path, _ in stats)
        changed = sum(lines for _, lines in stats)
        tests = tuple(p for p in files if cls.is_test_path(p))
        return cls._qualify(
            TaskCandidate(sha, parent_list[0], message.strip(), int(stamp), tests, files, changed)
        )

    @classmethod
    def _qualify(cls, task: TaskCandidate) -> TaskCandidate | None:
        """Apply the size, path and code-plus-test rules.

        Args:
            task: The commit as a candidate, not yet checked.

        Returns:
            TaskCandidate | None: The task when every rule holds.
        """
        files, tests, changed = task.files, task.tests, task.changed_lines
        if not files or len(files) > MAX_FILES or changed > MAX_CHANGED_LINES:
            return None
        if any(EXCLUDED_PATH.search(p) for p in files):
            return None
        if not tests or not any(cls.is_code_path(p) for p in files):
            return None
        return task

    @classmethod
    def _numstat(cls, rest: str) -> list[tuple[str, int]] | None:
        """Read the numstat entries of one record.

        Args:
            rest: The NUL-separated `added<TAB>deleted<TAB>path` entries.

        Returns:
            list[tuple[str, int]] | None: Path and changed lines per file, or None when a
                binary file is touched.
        """
        stats: list[tuple[str, int]] = []
        for entry in rest.split(FIELD):
            entry = entry.strip("\n")
            if not entry:
                continue
            fields = entry.split("\t", 2)
            if len(fields) != NUMSTAT_FIELDS:
                continue
            added, deleted, path = fields
            if not (added.isdigit() and deleted.isdigit()):
                return None
            stats.append((path, int(added) + int(deleted)))
        return stats
