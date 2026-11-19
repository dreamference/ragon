"""The node's service key reaches every unattended caller as RAGON_API_KEY (spec AUTH §13)."""

from pathlib import Path

from dreamference.node.service_key import ServiceKey

KEY = "sk-ragon-" + "A" * 43


def write_key(mode: int, text: str = KEY + "\n") -> Path:
    path = ServiceKey.path()
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)
    path.chmod(mode)
    return path


def test_service_key_read_returns_the_key_of_a_private_file():
    write_key(0o600)
    assert ServiceKey.read() == KEY


def test_service_key_read_refuses_a_file_others_can_read_or_a_missing_one():
    assert ServiceKey.read() is None
    write_key(0o644)
    assert ServiceKey.read() is None
    write_key(0o600, "not-a-key")
    assert ServiceKey.read() is None


def test_service_key_apply_sets_the_variable_but_keeps_a_callers_own_key():
    write_key(0o600)
    env: dict[str, str] = {}
    assert ServiceKey.apply(env) is True
    assert env["RAGON_API_KEY"] == KEY
    own = {"RAGON_API_KEY": "sk-ragon-mine"}
    ServiceKey.apply(own)
    assert own["RAGON_API_KEY"] == "sk-ragon-mine"
    assert ServiceKey.environment() == {"RAGON_API_KEY": KEY}


def test_service_key_apply_leaves_the_environment_alone_without_a_key():
    env: dict[str, str] = {}
    assert ServiceKey.apply(env) is False
    assert env == {}
    assert ServiceKey.environment() == {}


def test_every_unattended_caller_applies_the_service_key():
    root = Path(__file__).resolve().parent.parent / "dreamference"
    for name in (
        "night_shift/night_shift_task_run.py",
        "swe_bench/swe_bench_instance_run.py",
        "runner/codex_test_runner.py",
        "runner/codex_runner.py",
        "cli/first_run.py",
        "audit/egress_audit.py",
    ):
        assert "ServiceKey." in (root / name).read_text(), name
