"""The Mac node's engine tarball, `scripts/mac_engine_tarball.py`: the parts that run anywhere.

otool's output, the dependency walk, the install_name_tool plan, the packing and the member and
SHA256SUMS checks install.sh makes. The build, the rewrite and the dyld check run on a Mac only
(the script's `build`/`pack`/`verify`); nothing here calls otool, codesign or cmake.
"""

import hashlib
import importlib.util
import subprocess
import tarfile
from pathlib import Path

import pytest

REPO = Path(__file__).resolve().parent.parent
SCRIPT = REPO / "scripts" / "mac_engine_tarball.py"
INSTALL_CHECK = r"^/|(^|/)\.\.(/|$)"  # install.sh's grep -E over `tar -tzf`

spec = importlib.util.spec_from_file_location("mac_engine_tarball", SCRIPT)
met = importlib.util.module_from_spec(spec)
spec.loader.exec_module(met)

SERVE_OTOOL = """/build/ragon-serve:
\t/opt/homebrew/opt/pcre2/lib/libpcre2-8.0.dylib (compatibility version 15.0.0, current version 15.0.0)
\t/opt/homebrew/opt/icu4c@77/lib/libicuuc.77.dylib (compatibility version 77.0.0, current version 77.1.0)
\t/System/Library/Frameworks/Metal.framework/Versions/A/Metal (compatibility version 1.0.0, current version 370.0.0)
\t/usr/lib/libc++.1.dylib (compatibility version 1.0.0, current version 1900.180.0)
\t/usr/lib/libSystem.B.dylib (compatibility version 1.0.0, current version 1351.0.0)
"""
ICUUC_OTOOL = """/opt/homebrew/Cellar/icu4c@77/77.1/lib/libicuuc.77.1.dylib:
\t/opt/homebrew/opt/icu4c@77/lib/libicuuc.77.dylib (compatibility version 77.0.0, current version 77.1.0)
\t@loader_path/libicudata.77.dylib (compatibility version 77.0.0, current version 77.1.0)
\t/usr/lib/libc++.1.dylib (compatibility version 1.0.0, current version 1900.180.0)
"""


def test_otool_lists_dependencies_without_the_file_or_a_dylibs_own_id():
    assert met.parse_otool_l(SERVE_OTOOL, is_dylib=False) == [
        "/opt/homebrew/opt/pcre2/lib/libpcre2-8.0.dylib",
        "/opt/homebrew/opt/icu4c@77/lib/libicuuc.77.dylib",
        "/System/Library/Frameworks/Metal.framework/Versions/A/Metal",
        "/usr/lib/libc++.1.dylib",
        "/usr/lib/libSystem.B.dylib",
    ]
    assert met.parse_otool_l(ICUUC_OTOOL, is_dylib=True) == [
        "@loader_path/libicudata.77.dylib",
        "/usr/lib/libc++.1.dylib",
    ]


def test_the_closure_follows_libraries_through_each_other_and_skips_macos(tmp_path):
    # A fake Homebrew tree: the opt link resolves to the Cellar file, icudata sits beside icuuc.
    cellar = tmp_path / "Cellar"
    cellar.mkdir()
    for name in ("libpcre2-8.0.dylib", "libicuuc.77.1.dylib", "libicudata.77.1.dylib"):
        (cellar / name).write_bytes(b"\0")
    opt = tmp_path / "opt"
    opt.mkdir()
    (opt / "libpcre2-8.0.dylib").symlink_to(cellar / "libpcre2-8.0.dylib")
    (opt / "libicuuc.77.dylib").symlink_to(cellar / "libicuuc.77.1.dylib")
    (cellar / "libicudata.77.dylib").symlink_to(cellar / "libicudata.77.1.dylib")
    serve = tmp_path / "ragon-serve"
    deps = {
        serve: [str(opt / "libpcre2-8.0.dylib"), str(opt / "libicuuc.77.dylib"), "/usr/lib/libc++.1.dylib"],
        cellar / "libicuuc.77.1.dylib": ["@loader_path/libicudata.77.dylib", "/usr/lib/libc++.1.dylib"],
        cellar / "libpcre2-8.0.dylib": [],
        cellar / "libicudata.77.1.dylib": [],
    }
    libs = met.closure(serve, lambda p, is_dylib: deps[Path(p)])
    assert set(libs) == {cellar / "libpcre2-8.0.dylib", cellar / "libicuuc.77.1.dylib", cellar / "libicudata.77.1.dylib"}
    # Bundled under the names they are loaded by.
    assert sorted(met.bundle_names(libs).values()) == ["libicudata.77.dylib", "libicuuc.77.dylib", "libpcre2-8.0.dylib"]


def test_an_unresolved_rpath_is_named():
    with pytest.raises(SystemExit, match="cannot resolve @rpath/libx.dylib"):
        met.closure(Path("/b/ragon-serve"), lambda p, d: ["@rpath/libx.dylib"])


def test_the_rewrite_makes_the_serve_load_from_its_own_lib_folder():
    uc = "/opt/homebrew/opt/icu4c@77/lib/libicuuc.77.dylib"
    plan = met.rewrite_plan(
        serve_deps=[uc, "/usr/lib/libc++.1.dylib"],
        lib_deps={"libicuuc.77.dylib": ["@loader_path/libicudata.77.dylib"], "libicudata.77.dylib": []},
        names_by_load={uc: "libicuuc.77.dylib", "@loader_path/libicudata.77.dylib": "libicudata.77.dylib"},
    )
    assert plan == [
        ["install_name_tool", "-change", uc, "@executable_path/../lib/libicuuc.77.dylib", "bin/ragon-serve"],
        ["install_name_tool", "-id", "@rpath/libicudata.77.dylib", "lib/libicudata.77.dylib"],
        [
            "install_name_tool", "-id", "@rpath/libicuuc.77.dylib",
            "-change", "@loader_path/libicudata.77.dylib", "@loader_path/libicudata.77.dylib",
            "lib/libicuuc.77.dylib",
        ],
    ]


def _stage(tmp_path):
    stage = tmp_path / "stage"
    (stage / "bin").mkdir(parents=True)
    (stage / "lib").mkdir()
    (stage / "bin" / "ragon-serve").write_bytes(b"serve")
    (stage / "bin" / "ragon-serve").chmod(0o755)
    (stage / "lib" / "libpcre2-8.0.dylib").write_bytes(b"lib")
    return stage


def test_the_tarball_passes_install_shs_member_check(tmp_path):
    tarball = tmp_path / met.asset_name("0.9.10")
    assert tarball.name == "ragon-engine-macos-arm64-0.9.10.tar.gz"
    members = met.pack(_stage(tmp_path), tarball)
    met.check_members(members)
    listing = subprocess.run(["tar", "-tzf", str(tarball)], capture_output=True, text=True, check=True).stdout
    names = listing.split()
    assert "bin/ragon-serve" in names and "lib/libpcre2-8.0.dylib" in names
    assert subprocess.run(["grep", "-Eq", INSTALL_CHECK], input=listing, text=True).returncode == 1
    with tarfile.open(tarball) as tar:
        serve = tar.getmember("bin/ragon-serve")
        assert serve.mode & 0o111 and serve.uid == 0 and serve.uname == ""


@pytest.mark.parametrize("name", ["/bin/ragon-serve", "../x", "bin/../../x", "bin/.."])
def test_members_outside_the_folder_are_refused(name):
    assert met.bad_member(name)
    with pytest.raises(SystemExit, match="outside"):
        met.check_members(["bin/ragon-serve", name])


def test_the_serve_must_be_there_and_nothing_beside_bin_and_lib():
    with pytest.raises(SystemExit, match="no bin/ragon-serve"):
        met.check_members(["lib/libx.dylib"])
    with pytest.raises(SystemExit, match="outside bin/ and lib/"):
        met.check_members(["bin/ragon-serve", "share/x"])
    assert not met.bad_member("lib/lib..x.dylib")


def test_appledouble_files_are_refused(tmp_path):
    stage = _stage(tmp_path)
    (stage / "lib" / "._libpcre2-8.0.dylib").write_bytes(b"x")
    with pytest.raises(SystemExit, match="AppleDouble"):
        met.pack(stage, tmp_path / "e.tar.gz")


def test_the_sums_line_is_what_install_shs_awk_reads(tmp_path):
    tarball = tmp_path / met.asset_name("0.9.10")
    met.pack(_stage(tmp_path), tarball)
    line = met.sums_line(tarball)
    assert line == f"{hashlib.sha256(tarball.read_bytes()).hexdigest()}  {tarball.name}\n"
    awk = 'awk -v file="$F" \'{ f = $2; sub(/^\\*/, "", f); if (f == file) { print $1; exit } }\''
    got = subprocess.run(
        ["sh", "-c", awk], input="0" * 64 + "  other.tar.gz\n" + line, capture_output=True, text=True,
        env={"F": tarball.name, "PATH": "/usr/bin:/bin"},
    ).stdout.strip()
    assert got == line.split()[0]
    met.check_sums(tarball, line)
    with pytest.raises(SystemExit, match="does not match"):
        met.check_sums(tarball, "0" * 64 + f"  {tarball.name}\n")
    with pytest.raises(SystemExit, match="no SHA256SUMS line"):
        met.check_sums(tarball, "")


@pytest.mark.parametrize("bad", ["", "../1", "0.9 10", "/x"])
def test_versions_are_plain(bad):
    with pytest.raises(ValueError):
        met.asset_name(bad)
