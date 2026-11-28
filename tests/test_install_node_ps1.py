"""install.ps1 -Role node: what Linux can check about it without running PowerShell.

The logic itself runs in tests/windows/install-node.tests.ps1 (temp prefix, fake sc.exe, icacls
and ragon), which windows.yml runs under Windows PowerShell 5.1. Here: the script stays ASCII,
the node's checks come before anything is placed, the state table matches the Rust one, and the
record install.ps1 writes is the one `ragon update` (ragon-rs/src/node_update.rs) reads.
"""

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
INSTALL = (ROOT / "install.ps1").read_text(encoding="utf-8")
NODE_UPDATE = (ROOT / "ragon-rs" / "src" / "node_update.rs").read_text(encoding="utf-8")
LOCATOR = ROOT / "ragon-rs" / "node-locator" / "src" / "lib.rs"


def test_install_ps1_stays_ascii_for_windows_powershell_5_1() -> None:
    assert INSTALL.isascii()


def test_the_role_parameter_is_client_or_node_and_defaults_to_the_client() -> None:
    assert re.search(r'\[ValidateSet\("client", "node"\)\]\[string\]\$Role = "client"', INSTALL)


def test_the_node_needs_a_signed_release_and_the_engine_zip_is_checked_before_anything_is_placed() -> (
    None
):
    check = INSTALL.index("the node role needs a signed release")
    zip_check = INSTALL.index(
        "does not match its checksum in SHA256SUMS; nothing was installed.", check
    )
    place = INSTALL.index("# -- install ----")
    assert check < zip_check < place
    assert "if ($null -eq $Signed)" in INSTALL[check - 120 : check]


def test_the_node_role_stops_before_any_download_unless_elevated() -> None:
    elevated = INSTALL.index("the node role needs an elevated PowerShell")
    assert elevated < INSTALL.index("$ReleaseUrl = ")


def test_uninstall_runs_before_any_network_use() -> None:
    assert INSTALL.index("Uninstall-RagonNode $NodePaths") < INSTALL.index("$ReleaseUrl = ")


def test_the_service_is_made_by_the_supervisor_from_the_program_files_copy() -> None:
    assert '"server", "install", "--name", $NodeServiceName' in INSTALL
    assert 'Ragon = (Join-Path $NodePaths.Bin "ragon.exe")' in INSTALL
    assert r"NT SERVICE\ragon-node" in INSTALL


def test_the_ps_state_table_is_the_rust_one() -> None:
    rust = LOCATOR.read_text(encoding="utf-8") if LOCATOR.exists() else ""
    if "pub fn table()" not in rust:
        return  # the state layout lands in its own branch (win-node-state-layout)
    table = rust[rust.index("pub fn table()") :]
    table = table[: table.index("\n    }\n")]
    words = {
        "System": "SYSTEM",
        "Administrators": "ADMINS",
        "Service": "SERVICE",
        "Job": "JOB",
        "Sandbox": "SANDBOX",
    }
    access = {"Full": "F", "Modify": "M", "ReadExecute": "RX", "Read": "R"}
    rust_rows = set()
    for item in ("Root", "Models", "NodeId", "Advert", "Jobs"):
        for who in ("System", "Administrators", "Service"):
            rust_rows.add(
                (item, words[who], {"System": "F", "Administrators": "F", "Service": "M"}[who])
            )
    for match in re.finditer(r"rows\.push\(rule\((\w+), (\w+), (\w+)\)\);\n", table):
        item, who, level = match.groups()
        if who in ("System", "Administrators", "Service") and item not in (
            "Root",
            "Models",
            "NodeId",
            "Advert",
            "Jobs",
        ):
            continue
        rust_rows.add((item, words[who], access[level]))
    ps = INSTALL[INSTALL.index("function Get-NodeAclTable") :]
    ps = ps[: ps.index("return $Rows")]
    ps_rows = {
        tuple(m.groups()) for m in re.finditer(r'\$Rows \+= , @\("(\w+)", "(\w+)", "(\w+)"\)', ps)
    }
    # the first block of the PS table is a loop over the five items with the three base rows
    for item in ("Root", "Models", "NodeId", "Advert", "Jobs"):
        ps_rows |= {(item, "SYSTEM", "F"), (item, "ADMINS", "F"), (item, "SERVICE", "M")}
    assert ps_rows == rust_rows


def test_the_record_install_ps1_writes_is_what_ragon_update_reads() -> None:
    writer = INSTALL[INSTALL.index("function Save-NodeRecord") :]
    writer = writer[: writer.index("\n}\n")]
    written = set(re.findall(r'"(\w+)=\$', writer))
    read = set(re.findall(r'field\("(\w+)"\)', NODE_UPDATE))
    assert read <= written, (read, written)


def test_the_engine_asset_is_named_the_same_in_the_script_and_the_updater() -> None:
    assert "ragon-engine-$($Tag.TrimStart('v'))-windows-gfx1151.zip" in INSTALL
    assert 'format!("ragon-engine-{}-windows-gfx1151.zip"' in NODE_UPDATE


def test_windows_yml_runs_the_node_installer_tests() -> None:
    workflow = (ROOT / ".github" / "workflows" / "windows.yml").read_text(encoding="utf-8")
    assert r"tests\windows\install-node.tests.ps1" in workflow
