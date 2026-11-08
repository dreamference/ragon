"""The node watcher's crate (specs/DREAMFERENCE_MONITORING.md §8, Phase 1).

Its behaviour is tested in Rust (`ragon-rs/node-monitor/`, `cargo test` in a copy of the folder, all
probes over a fake `Host`); nothing here touches the machine.
"""

import re
import tomllib
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
CRATE = REPO / "ragon-rs" / "node-monitor"


def test_the_crate_is_named_as_decision_o1_says():
    manifest = tomllib.loads((CRATE / "Cargo.toml").read_text())
    assert manifest["package"]["name"] == "ragon-node-monitor"
    assert manifest["bin"][0]["name"] == "ragon-node-monitor"


def test_the_crate_is_not_a_launcher_member_and_not_in_the_branded_build_list():
    # Phase 1 ships nothing: the binary joins the release installer in a later phase.
    launcher = tomllib.loads((REPO / "ragon-rs" / "Cargo.toml").read_text())
    assert "ragon-node-monitor" not in launcher["dependencies"]
    builder = (REPO / "dreamference" / "runner" / "codex_branded_builder.py").read_text()
    assert "node-monitor" not in builder


def test_the_crate_has_no_http_client_or_async_runtime():
    manifest = (CRATE / "Cargo.toml").read_text()
    for name in ("reqwest", "tokio", "hyper", "ureq"):
        assert f"\n{name} =" not in manifest, name


def test_the_binary_names_no_host_but_loopback():
    # §8.5: HTTP goes through `Host::http_get(port, ...)`, which has no way to name another host.
    source = "\n".join(p.read_text() for p in (CRATE / "src").glob("*.rs"))
    assert "Ipv4Addr::LOCALHOST" in source
    assert not re.search(r"\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}\b", source.replace("127.0.0.1", ""))
    assert "http://" not in source and "https://" not in source
    assert "UdpSocket" not in source and "TcpListener" not in source


def test_the_crates_versions_are_the_codex_workspaces():
    workspace = REPO / "codex" / "codex-rs" / "Cargo.toml"
    if not workspace.exists():  # the submodule is not checked out
        return
    deps = tomllib.loads((CRATE / "Cargo.toml").read_text())["dependencies"]
    pinned = workspace.read_text()
    for name, spec in deps.items():
        version = spec if isinstance(spec, str) else spec["version"]
        found = re.search(rf'^{name} = (?:"([^"]+)"|\{{ version = "([^"]+)")', pinned, re.M)
        assert found, name
        assert (found.group(1) or found.group(2)).lstrip("=") == version.lstrip("="), name


def test_every_unit_the_monitor_watches_is_a_unit_the_product_writes():
    # §3.8 asks for the reverse test too (every product unit is listed); that one waits for the
    # unit table to be complete (Phase 2 adds the system units and ragon-signal@).
    watched = re.findall(
        r'"(ragon-[\w@-]+\.(?:service|timer))"', (CRATE / "src" / "units.rs").read_text()
    )
    assert watched
    written = "\n".join(
        p.read_text(errors="replace")
        for root in ("dreamference", "ragon-rs", "ragon-web-rs", "scripts")
        for p in (REPO / root).rglob("*")
        if p.suffix in (".py", ".rs", ".sh") and "node-monitor" not in p.parts
    )
    for unit in watched:
        assert unit.rsplit(".", 1)[0] in written, unit
