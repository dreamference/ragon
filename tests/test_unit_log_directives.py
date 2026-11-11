"""Every service unit Ragon writes carries the logging directives of LOGGING §3.3."""

import re
from pathlib import Path

import pytest

from dreamference.chat.matrix_homeserver import MatrixHomeserver
from dreamference.config.unit_log_directives import UnitLogDirectives
from dreamference.forge.forge_service import ForgeService
from dreamference.night_shift.night_shift_scheduler import NightShiftScheduler
from dreamference.node.node_monitor_service import NodeMonitorService
from dreamference.remote.remote_box import RemoteBox
from dreamference.remote.remote_control_plane import RemoteControlPlane
from dreamference.remote.remote_public_certificate import RemotePublicCertificate
from dreamference.remote.remote_tunnel import RemoteTunnel

ROOT = Path(__file__).resolve().parents[1]
COMPONENTS = [
    line.strip()
    for line in (ROOT / "ragon-rs/vectors/logging/components.txt").read_text().splitlines()
    if line.strip() and not line.startswith("#")
]
RUST_UNIT_FILES = {
    "ragon-rs/monitor/src/units.rs": "monitor",
    "ragon-rs/signal/src/unit.rs": "signal",
    "ragon-rs/sync/src/cli.rs": "sync",
    "ragon-rs/web/src/cli.rs": "web",
    "ragon-rs/chat/src/cli.rs": "chat",
}


def _python_units() -> dict[str, str]:
    units: dict[str, str] = {}
    units["night"] = NightShiftScheduler.render_units("01:00-07:00", "/usr/bin/ragon-admin")[1]
    units["monitor"] = NodeMonitorService.unit_text()
    units["tunnel"] = RemoteTunnel.render_unit("remote.example.org")
    units["relay"] = RemoteBox.relay_unit("remote.example.org")
    units["matrix-tls"] = MatrixHomeserver.tls_unit("100.64.0.1", "/c.pem", "/k.pem")
    for name, text in (
        list(ForgeService.units().items())
        + list(RemoteControlPlane.proxy_units().items())
        + list(RemotePublicCertificate.timer_units().items())
        + list(MatrixHomeserver.proxy_units("127.0.0.1:1").items())
    ):
        units[name] = text
    return units


def _check(name: str, text: str) -> None:
    sections = re.split(r"^\[Service\]\n", text, flags=re.MULTILINE)
    if len(sections) == 1:
        return  # a socket, timer or path unit has no process to log
    for body in sections[1:]:
        body = body.split("\n[", 1)[0]
        ident = re.search(r"^SyslogIdentifier=ragon-([a-z][a-z0-9-]*)$", body, re.MULTILINE)
        assert ident, f"{name}: no SyslogIdentifier"
        component = ident.group(1)
        assert component in COMPONENTS, f"{name}: {component} is not in components.txt"
        for line in (
            r"EnvironmentFile=-(%h/\.config|/etc)/dreamference/log\.env",
            f"LogExtraFields=RAGON_COMPONENT={component}",
            "LogRateLimitIntervalSec=30s",
            "LogRateLimitBurst=20000",
        ):
            assert re.search(f"^{line}$", body, re.MULTILINE), f"{name}: missing {line}"


def test_every_python_unit_text_carries_the_log_directives():
    units = _python_units()
    assert len(units) >= 10
    serviced = 0
    for name, text in units.items():
        _check(name, text)
        serviced += "[Service]" in text
    assert serviced >= 8


def test_inject_replaces_an_old_identifier_and_leaves_other_units_alone():
    text = "[Service]\nSyslogIdentifier=old\nExecStart=/x\n"
    out = UnitLogDirectives.inject(text, "forge")
    assert "old" not in out and out.count("SyslogIdentifier=") == 1
    assert (
        UnitLogDirectives.inject("[Timer]\nOnCalendar=daily\n", "forge")
        == "[Timer]\nOnCalendar=daily\n"
    )


@pytest.mark.parametrize("path,component", sorted(RUST_UNIT_FILES.items()))
def test_every_rust_unit_template_carries_the_log_directives(path, component):
    source = (ROOT / path).read_text()
    assert component in COMPONENTS
    for needle in (
        f"SyslogIdentifier=ragon-{component}",
        "EnvironmentFile=-",
        f"LogExtraFields=RAGON_COMPONENT={component}",
        "LogRateLimitIntervalSec=30s",
        "LogRateLimitBurst=20000",
    ):
        assert needle in source, f"{path}: {needle}"


def test_no_other_rust_or_python_file_writes_a_service_unit():
    known = {Path(p) for p in RUST_UNIT_FILES}
    found = set()
    for base in ("ragon-rs", "dreamference"):
        for f in (ROOT / base).rglob("*"):
            if f.suffix not in (".rs", ".py") or "target" in f.parts or "tests" in f.parts:
                continue
            if "ExecStart=" in f.read_text(errors="ignore"):
                found.add(f.relative_to(ROOT))
    python_known = {
        Path(f"dreamference/{p}.py")
        for p in (
            "remote/remote_tunnel",
            "remote/remote_box",
            "remote/remote_control_plane",
            "remote/remote_public_certificate",
            "night_shift/night_shift_scheduler",
            "node/node_monitor_service",
            "forge/forge_service",
            "chat/matrix_homeserver",
        )
    }
    assert found <= known | python_known, found - known - python_known
