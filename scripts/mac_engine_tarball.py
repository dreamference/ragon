#!/usr/bin/env python3
"""The Mac node's engine tarball, ragon-engine-macos-arm64-<version>.tar.gz, and its SHA256SUMS line.

install.sh --role node checks the tarball against SHA256SUMS, refuses a member that is absolute or
has a `..` component, and unpacks it as /usr/local/lib/ragon/engine-<version> (linked as `engine`);
the firewall rule and the node's start path (ragon-rs/src/mac_engine.rs) run `engine/bin/ragon-serve`.
So the archive holds, at its root:

    bin/ragon-serve          the Metal serve (the kernels are compiled into it from source at
                             start, so there is no .metallib to ship)
    lib/<name>.dylib         every library it loads that macOS does not provide (Homebrew's PCRE2
                             and ICU today), found by `otool -L`, rewritten to load from
                             @executable_path/../lib (and @loader_path from each other), re-signed
                             ad hoc because install_name_tool breaks the arm64 signature

Run on a Mac with the engine's build dependencies (cmake, nlohmann-json, pcre2, icu4c, pkg-config):

    mac_engine_tarball.py build --engine-src ENGINE_CHECKOUT --version 0.9.10 --out DIR
    mac_engine_tarball.py pack --serve BUILT_RAGON_SERVE --version 0.9.10 --out DIR
    mac_engine_tarball.py verify DIR/ragon-engine-macos-arm64-0.9.10.tar.gz

`build` configures and builds the `ragon-serve` target with RAGON_BACKEND=metal, then packs;
`pack` takes a serve already built; both write the tarball and `<tarball>.sha256` (its SHA256SUMS
line) to --out and verify the result. `verify` unpacks into a temporary directory and checks the
members, that nothing loads from outside macOS and the bundle, and every signature. The pure parts
(otool parsing, the rewrite plan, the member and sums checks) are tested on Linux in
tests/test_mac_engine_tarball.py.
"""

from __future__ import annotations

import argparse
import hashlib
import os
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
from pathlib import Path
from typing import Callable

ASSET_PREFIX = "ragon-engine-macos-arm64-"
SERVE = "bin/ragon-serve"
LIB_DIR = "lib"
# Libraries every macOS has: never bundled.
SYSTEM_PREFIXES = ("/usr/lib/", "/System/")
# What the serve links but Homebrew provides; for the hint when cmake cannot find them.
BREW_DEPS = ("cmake", "pkg-config", "nlohmann-json", "pcre2", "icu4c")


def asset_name(version: str) -> str:
    if not re.fullmatch(r"[0-9A-Za-z][0-9A-Za-z.+-]*", version):
        raise ValueError(f"not a version: {version!r}")
    return f"{ASSET_PREFIX}{version}.tar.gz"


def parse_otool_l(text: str, is_dylib: bool) -> list[str]:
    """The load commands' paths from `otool -L FILE`: the first line names FILE; a dylib's first
    entry is its own id, which is not a dependency."""
    lines = text.splitlines()[1:]
    deps = []
    for line in lines:
        m = re.match(r"\s+(\S.*?) \(compatibility version", line)
        if m:
            deps.append(m.group(1))
    return deps[1:] if is_dylib else deps


def is_system(dep: str) -> bool:
    return dep.startswith(SYSTEM_PREFIXES)


def resolve(dep: str, referrer: Path, rpaths: list[str]) -> Path | None:
    """Where a load path points, for a file at `referrer`; None for @rpath entries that no rpath
    resolves (the caller names it)."""
    if dep.startswith("@loader_path/") or dep.startswith("@executable_path/"):
        return referrer.parent / dep.split("/", 1)[1]
    if dep.startswith("@rpath/"):
        for rp in rpaths:
            base = Path(rp.replace("@loader_path", str(referrer.parent)))
            if (base / dep[len("@rpath/") :]).exists():
                return base / dep[len("@rpath/") :]
        return None
    return Path(dep)


def closure(
    serve: Path,
    otool: Callable[[Path, bool], list[str]],
    rpaths: Callable[[Path], list[str]] = lambda p: [],
) -> dict[Path, list[str]]:
    """Every non-system library the serve loads, directly or through another: {real file: the load
    paths that name it}. `otool(path, is_dylib)` lists a file's dependencies."""
    found: dict[Path, list[str]] = {}
    todo: list[tuple[Path, bool]] = [(serve, False)]
    seen: set[Path] = set()
    while todo:
        path, is_dylib = todo.pop()
        if path in seen:
            continue
        seen.add(path)
        for dep in otool(path, is_dylib):
            if is_system(dep):
                continue
            target = resolve(dep, path, rpaths(path))
            if target is None:
                raise SystemExit(f"{path}: cannot resolve {dep} (no LC_RPATH entry holds it)")
            real = Path(os.path.realpath(target))
            names = found.setdefault(real, [])
            if dep not in names:
                names.append(dep)
            todo.append((real, True))
    return found


def bundle_names(libs: dict[Path, list[str]]) -> dict[Path, str]:
    """Each library's file name in lib/: the name the binaries load it by (the last component of
    its first load path, e.g. libicuuc.77.dylib, not the real file's libicuuc.77.1.dylib)."""
    names: dict[Path, str] = {}
    for real, loads in sorted(libs.items()):
        name = Path(loads[0]).name
        if name in names.values():
            raise SystemExit(f"two libraries would both be lib/{name}")
        names[real] = name
    return names


def rewrite_plan(
    serve_deps: list[str], lib_deps: dict[str, list[str]], names_by_load: dict[str, str]
) -> list[list[str]]:
    """install_name_tool calls (paths inside the stage) that make the bundle self-contained:
    bin/ragon-serve loads @executable_path/../lib/NAME, each lib loads its siblings from
    @loader_path/NAME, each lib's id is @rpath/NAME. `lib_deps` maps a lib/ name to its
    dependencies' load paths, `names_by_load` a load path to its lib/ name."""
    plan: list[list[str]] = []
    changes: list[str] = []
    for dep in serve_deps:
        if dep in names_by_load:
            changes += ["-change", dep, f"@executable_path/../{LIB_DIR}/{names_by_load[dep]}"]
    if changes:
        plan.append(["install_name_tool", *changes, SERVE])
    for name, deps in sorted(lib_deps.items()):
        args = ["-id", f"@rpath/{name}"]
        for dep in deps:
            if dep in names_by_load:
                args += ["-change", dep, f"@loader_path/{names_by_load[dep]}"]
        plan.append(["install_name_tool", *args, f"{LIB_DIR}/{name}"])
    return plan


def bad_member(name: str) -> bool:
    """What install.sh refuses: an absolute member or one with a `..` component."""
    return name.startswith("/") or ".." in name.split("/")


def pack(stage: Path, tarball: Path) -> list[str]:
    """Writes the stage's bin/ and lib/ as a gzip tarball with root-relative members (sorted, owner
    0, no extended attributes or AppleDouble files) and returns the member names."""
    members = []
    with tarfile.open(tarball, "w:gz", format=tarfile.PAX_FORMAT) as tar:
        for top in ("bin", LIB_DIR):
            root = stage / top
            if not root.is_dir():
                continue
            paths = [root, *sorted(p for p in root.rglob("*"))]
            for path in paths:
                name = path.relative_to(stage).as_posix()
                if path.name.startswith("._") or path.is_symlink():
                    raise SystemExit(f"{name}: no AppleDouble files or links in the engine tarball")
                info = tar.gettarinfo(str(path), arcname=name)
                info.uid = info.gid = 0
                info.uname = info.gname = ""
                info.pax_headers = {}
                if info.isfile():
                    with path.open("rb") as f:
                        tar.addfile(info, f)
                else:
                    tar.addfile(info)
                members.append(name)
    return members


def check_members(names: list[str]) -> None:
    bad = [n for n in names if bad_member(n)]
    if bad:
        raise SystemExit(f"members outside the tarball's folder: {bad[:3]}")
    if SERVE not in names:
        raise SystemExit(f"the tarball has no {SERVE}")
    stray = [n for n in names if n.split("/")[0] not in ("bin", LIB_DIR)]
    if stray:
        raise SystemExit(f"members outside bin/ and lib/: {stray[:3]}")


def sums_line(tarball: Path) -> str:
    """The tarball's SHA256SUMS line, in sha256sum's format (what install.sh's awk reads)."""
    digest = hashlib.sha256(tarball.read_bytes()).hexdigest()
    return f"{digest}  {tarball.name}\n"


def check_sums(tarball: Path, sums_text: str) -> None:
    for line in sums_text.splitlines():
        parts = line.split()
        if len(parts) == 2 and parts[1].lstrip("*") == tarball.name:
            if parts[0] != hashlib.sha256(tarball.read_bytes()).hexdigest():
                raise SystemExit(f"{tarball.name} does not match its SHA256SUMS line")
            return
    raise SystemExit(f"no SHA256SUMS line for {tarball.name}")


DYLD_LOADED = re.compile(r"^dyld\[\d+\]: <[0-9A-Fa-f-]+> (/.+)$")


def dyld_loaded(stderr: str) -> list[str]:
    """The image paths in DYLD_PRINT_LIBRARIES output (`dyld[pid]: <uuid> /path`); dyld's other
    notes ("move loaded to delayed: libX.dylib", about the shared cache) name no file."""
    return [m.group(1) for m in map(DYLD_LOADED.match, stderr.splitlines()) if m]


# --- macOS only below: otool, install_name_tool, codesign, cmake ---------------------------------


def run(cmd: list[str], cwd: Path | None = None) -> str:
    print("+", " ".join(cmd), file=sys.stderr)
    return subprocess.run(cmd, cwd=cwd, check=True, text=True, capture_output=True).stdout


def otool_deps(path: Path, is_dylib: bool) -> list[str]:
    return parse_otool_l(run(["otool", "-L", str(path)]), is_dylib)


def otool_rpaths(path: Path) -> list[str]:
    text = run(["otool", "-l", str(path)])
    return re.findall(r"cmd LC_RPATH\n\s+cmdsize \d+\n\s+path (\S+)", text)


def stage_serve(serve: Path, stage: Path) -> None:
    (stage / "bin").mkdir(parents=True)
    (stage / LIB_DIR).mkdir()
    shutil.copy2(serve, stage / SERVE)
    libs = closure(serve, otool_deps, otool_rpaths)
    names = bundle_names(libs)
    names_by_load = {load: names[real] for real, loads in libs.items() for load in loads}
    lib_deps = {}
    for real, name in names.items():
        dst = stage / LIB_DIR / name
        shutil.copyfile(real, dst)
        dst.chmod(0o644)
        lib_deps[name] = otool_deps(real, True)
    for cmd in rewrite_plan(otool_deps(serve, False), lib_deps, names_by_load):
        run(cmd, cwd=stage)
    for target in [stage / SERVE, *sorted((stage / LIB_DIR).iterdir())]:
        run(["codesign", "--force", "--sign", "-", str(target)])
    (stage / SERVE).chmod(0o755)


def verify(tarball: Path, sums_text: str | None = None) -> None:
    """Unpacks into a temporary directory and checks it as install.sh and dyld will."""
    if sums_text is not None:
        check_sums(tarball, sums_text)
    with tarfile.open(tarball) as tar:
        names = tar.getnames()
        check_members(names)
        with tempfile.TemporaryDirectory(prefix="ragon-engine-verify-") as tmp:
            try:
                tar.extractall(tmp, filter="data")
            except TypeError:  # Python before 3.12 (and its backports): the members are checked above
                tar.extractall(tmp)
            root = Path(tmp)
            roots = (str(root), os.path.realpath(root))
            for name in names:
                path = root / name
                if not path.is_file():
                    continue
                for dep in otool_deps(path, name.startswith(LIB_DIR + "/")):
                    if is_system(dep):
                        continue
                    if not dep.startswith(("@executable_path/../lib/", "@loader_path/")):
                        raise SystemExit(f"{name} still loads {dep}")
                    target = resolve(dep, path, [])
                    if target is None or not target.exists():
                        raise SystemExit(f"{name} loads {dep}, which is not in the tarball")
                run(["codesign", "--verify", "--strict", str(path)])
            # dyld itself: the serve's --help loads every library and exits.
            env = dict(os.environ, DYLD_PRINT_LIBRARIES="1")
            out = subprocess.run([str(root / SERVE), "--help"], env=env, capture_output=True, text=True)
            loaded = dyld_loaded(out.stderr)
            if not any(l.startswith(roots) and "/lib/" in l for l in loaded):
                raise SystemExit("dyld printed no library from the bundle (was DYLD_PRINT_LIBRARIES honoured?)")
            outside = [l for l in loaded if not l.startswith((*roots, *SYSTEM_PREFIXES))]
            if outside:
                raise SystemExit(f"the serve loaded libraries from outside the bundle: {outside[:3]}")
    print(f"verified {tarball.name}: {len(names)} members, self-contained, signed")


def build(engine_src: Path, build_dir: Path, jobs: int) -> Path:
    prefixes = []
    for formula in ("icu4c", "pcre2", "nlohmann-json"):
        try:
            prefixes.append(run(["brew", "--prefix", formula]).strip())
        except (OSError, subprocess.CalledProcessError):
            raise SystemExit(f"Homebrew {formula} is missing (brew install {' '.join(BREW_DEPS)})")
    run(
        [
            "cmake", "-S", str(engine_src), "-B", str(build_dir), "-DRAGON_BACKEND=metal",
            "-DCMAKE_BUILD_TYPE=Release", f"-DCMAKE_PREFIX_PATH={';'.join(prefixes)}",
        ]
    )
    run(["cmake", "--build", str(build_dir), "--target", "ragon-serve", "-j", str(jobs)])
    serve = build_dir / "ragon-serve"
    if not serve.is_file():
        raise SystemExit(f"the build made no {serve}")
    return serve


def pack_serve(serve: Path, version: str, out: Path) -> Path:
    out.mkdir(parents=True, exist_ok=True)
    tarball = out / asset_name(version)
    with tempfile.TemporaryDirectory(prefix="ragon-engine-stage-") as tmp:
        stage = Path(tmp) / "stage"
        stage_serve(serve, stage)
        check_members(pack(stage, tarball))
    line = sums_line(tarball)
    (out / f"{tarball.name}.sha256").write_text(line)
    verify(tarball, line)
    print(line, end="")
    return tarball


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    b = sub.add_parser("build", help="build ragon-serve (Metal) and pack it")
    b.add_argument("--engine-src", type=Path, required=True)
    b.add_argument("--build-dir", type=Path)
    b.add_argument("--jobs", type=int, default=os.cpu_count() or 8)
    p = sub.add_parser("pack", help="pack a ragon-serve already built")
    p.add_argument("--serve", type=Path, required=True)
    for s in (b, p):
        s.add_argument("--version", required=True)
        s.add_argument("--out", type=Path, required=True)
    v = sub.add_parser("verify", help="check a tarball as install.sh and dyld will")
    v.add_argument("tarball", type=Path)
    v.add_argument("--sums", type=Path, help="SHA256SUMS (default: <tarball>.sha256)")
    args = ap.parse_args(argv)
    if args.cmd == "verify":
        sums = args.sums or args.tarball.with_name(args.tarball.name + ".sha256")
        verify(args.tarball, sums.read_text() if sums.exists() else None)
        return 0
    asset_name(args.version)
    if args.cmd == "build":
        build_dir = args.build_dir or args.out / "build"
        serve = build(args.engine_src, build_dir, args.jobs)
    else:
        serve = args.serve
    pack_serve(serve, args.version, args.out)
    return 0


if __name__ == "__main__":
    sys.exit(main())
