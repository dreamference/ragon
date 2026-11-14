"""AdapterTrainTasks on a fixture git repository built in the test."""

import os
import subprocess
from pathlib import Path

import pytest

from dreamference.hardware.adapter_train_tasks import AdapterTrainTasks

BASE_TIME = 1_700_000_000


def git(
    repo: Path, *args: str, when: int = BASE_TIME, author: str = "Dev One <dev@example.org>"
) -> None:
    name, _, mail = author.partition(" <")
    env = {
        **os.environ,
        "GIT_AUTHOR_NAME": name, "GIT_AUTHOR_EMAIL": mail.rstrip(">"),
        "GIT_COMMITTER_NAME": name, "GIT_COMMITTER_EMAIL": mail.rstrip(">"),
        "GIT_AUTHOR_DATE": f"{when} +0000", "GIT_COMMITTER_DATE": f"{when} +0000",
    }  # fmt: skip
    subprocess.run(["git", "-C", str(repo), *args], check=True, capture_output=True, env=env)


def commit(
    repo: Path,
    files: dict[str, str],
    message: str,
    when: int,
    author: str = "Dev One <dev@example.org>",
) -> None:
    for rel, text in files.items():
        path = repo / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
    git(repo, "add", "-A", when=when, author=author)
    git(repo, "commit", "-q", "-m", message, when=when, author=author)


@pytest.fixture
def repo(tmp_path: Path) -> Path:
    root = tmp_path / "repo"
    root.mkdir()
    git(root, "init", "-q", "-b", "main")
    commit(root, {"README.md": "x\n"}, "Start the project history", BASE_TIME)
    return root


def good(repo: Path, i: int, when: int | None = None) -> None:
    commit(
        repo,
        {
            f"src/mod{i}.py": f"V = {i}\n",
            f"tests/test_mod{i}.py": f"def test_{i}():\n    assert {i}\n",
        },
        f"Add module {i} with its test",
        when if when is not None else BASE_TIME + 100 + i,
    )


def test_a_good_commit_is_a_candidate_with_parent_message_and_tests(repo: Path) -> None:
    good(repo, 1)
    found = AdapterTrainTasks.candidates(repo)
    assert len(found) == 1
    c = found[0]
    parent = subprocess.run(
        ["git", "-C", str(repo), "rev-parse", "HEAD^"], capture_output=True, text=True, check=True
    )
    assert c.parent == parent.stdout.strip()
    assert c.message == "Add module 1 with its test"
    assert c.tests == ("tests/test_mod1.py",)
    assert set(c.files) == {"src/mod1.py", "tests/test_mod1.py"}
    assert c.changed_lines == 3


def test_commits_that_miss_a_rule_are_not_candidates(repo: Path) -> None:
    t = BASE_TIME + 10
    commit(repo, {"src/a.py": "A = 1\n"}, "Change code without any test", t)
    commit(repo, {"tests/test_b.py": "x = 1\n"}, "Only touch a test file here", t + 1)
    commit(repo, {"src/c.py": "C = 1\n", "tests/test_c.py": "x\n"}, "fix", t + 2)
    commit(
        repo, {"src/d.py": "D = 1\n", "tests/test_d.py": "x\n"}, "Revert the module d change", t + 3
    )
    commit(
        repo,
        {"src/e.py": "E = 1\n", "tests/test_e.py": "x\n"},
        "Bump the e dependency",
        t + 4,
        author="dependabot[bot] <bot@example.org>",
    )
    commit(
        repo,
        {"vendor/f.py": "F = 1\n", "src/f.py": "F\n", "tests/test_f.py": "x\n"},
        "Update vendored f with test",
        t + 5,
    )
    commit(
        repo,
        {"src/big.py": "x\n" * 401, "tests/test_big.py": "x\n"},
        "Add the big module and test",
        t + 6,
    )
    many = {f"src/m{i}.py": "x\n" for i in range(15)} | {"tests/test_many.py": "x\n"}
    commit(repo, many, "Add sixteen files at once", t + 7)
    assert AdapterTrainTasks.candidates(repo) == []


def test_limits_are_inclusive(repo: Path) -> None:
    files = {f"src/m{i}.py": "x\n" for i in range(14)} | {"tests/test_edge.py": "x\n" * 386}
    commit(repo, files, "Add fifteen files and four hundred lines", BASE_TIME + 5)
    found = AdapterTrainTasks.candidates(repo)
    assert len(found) == 1 and found[0].changed_lines == 400 and len(found[0].files) == 15


def test_merges_are_skipped_and_only_the_first_parent_is_walked(repo: Path) -> None:
    git(repo, "checkout", "-q", "-b", "side")
    good(repo, 7)
    git(repo, "checkout", "-q", "main")
    good(repo, 8)
    git(
        repo,
        "merge",
        "--no-ff",
        "-q",
        "-m",
        "Merge the side branch with tests",
        "side",
        when=BASE_TIME + 500,
    )
    found = AdapterTrainTasks.candidates(repo)
    assert [c.message for c in found] == ["Add module 8 with its test"]


def test_refuses_below_fifty_with_the_reason(repo: Path) -> None:
    for i in range(49):
        good(repo, i)
    with pytest.raises(RuntimeError, match=r"only 49 gradable .*at least 50"):
        AdapterTrainTasks.collect(repo)


def test_holdout_is_the_newest_by_date_with_a_floor_of_twenty(repo: Path) -> None:
    # History order is the reverse of date order for the first ten commits.
    for i in range(50):
        good(repo, i, when=BASE_TIME + 1000 - i if i < 10 else BASE_TIME + 2000 + i)
    train, holdout = AdapterTrainTasks.collect(repo)
    assert len(holdout) == 20 and len(train) == 30
    assert min(c.committed_at for c in holdout) >= max(c.committed_at for c in train)
    assert holdout[-1].message == "Add module 49 with its test"


def test_holdout_is_ten_percent_between_the_bounds_and_capped_at_sixty(repo: Path) -> None:
    for i in range(300):
        good(repo, i)
    found = AdapterTrainTasks.candidates(repo)
    assert len(AdapterTrainTasks.split(found)[1]) == 30
    assert len(AdapterTrainTasks.split(found * 3)[1]) == 60


def test_a_non_repository_is_an_error(tmp_path: Path) -> None:
    with pytest.raises(RuntimeError, match="git log failed"):
        AdapterTrainTasks.candidates(tmp_path)


def test_nothing_is_written_in_the_repository(repo: Path) -> None:
    good(repo, 1)
    before = sorted(
        p.relative_to(repo).as_posix() for p in repo.rglob("*") if ".git/" not in p.as_posix() + "/"
    )
    index = (repo / ".git" / "index").read_bytes()
    AdapterTrainTasks.candidates(repo)
    after = sorted(
        p.relative_to(repo).as_posix() for p in repo.rglob("*") if ".git/" not in p.as_posix() + "/"
    )
    assert before == after and (repo / ".git" / "index").read_bytes() == index
