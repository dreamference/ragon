"""Every `docker run` argv the package builds carries the bounded log options (LOGGING §6.3)."""

from __future__ import annotations

import ast
from pathlib import Path

from dreamference.config.container_log_options import ContainerLogOptions

PACKAGE = Path(__file__).resolve().parents[1] / "dreamference"
# Lists that start with "run" but are not a docker command (lego, npm).
NOT_DOCKER = {"remote_public_certificate.py", "desktop_runner.py"}


def _is_run(node: ast.expr) -> bool:
    return isinstance(node, ast.Constant) and node.value == "run"


def _is_helper(node: ast.expr) -> bool:
    return (
        isinstance(node, ast.Starred)
        and isinstance(node.value, ast.Call)
        and isinstance(node.value.func, ast.Attribute)
        and node.value.func.attr == "args"
        and isinstance(node.value.func.value, ast.Name)
        and node.value.func.value.id == "ContainerLogOptions"
    )


def _docker_run_lists() -> list[tuple[str, int, ast.List]]:
    found = []
    for path in sorted(PACKAGE.rglob("*.py")):
        if path.name in NOT_DOCKER:
            continue
        for node in ast.walk(ast.parse(path.read_text())):
            if not isinstance(node, ast.List) or len(node.elts) < 3:
                continue
            first = node.elts[0]
            docker_run = (
                isinstance(first, ast.Constant)
                and first.value == "docker"
                and _is_run(node.elts[1])
            )
            if docker_run or _is_run(first):
                found.append((path.name, node.lineno, node))
    return found


def test_the_helper_bounds_the_log_and_labels_the_component() -> None:
    args = ContainerLogOptions.args("x", max_size="20m", max_file=5)
    assert args[2:4] == ["--log-opt", "max-size=20m"] and "max-file=5" in args
    assert "ragon.component=x" in args and "core=0" in args


def test_every_docker_run_argv_in_the_package_passes_the_helper() -> None:
    lists = _docker_run_lists()
    assert len(lists) >= 15
    missing = []
    for name, line, node in lists:
        start = 2 if _is_run(node.elts[1]) and not _is_run(node.elts[0]) else 1
        # The helper sits right after `run`, or after a leading `-d` / `--rm`, so callers and
        # tests that match the start of the argv (`docker run -d`) keep working.
        window = node.elts[start : start + 2]
        if not any(_is_helper(item) for item in window):
            missing.append(f"{name}:{line}")
    assert not missing, f"docker run without ContainerLogOptions.args: {missing}"
