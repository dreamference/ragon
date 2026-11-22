"""An x86-64 `ragon` that starts inside every Terminal-Bench task image.

The task images are amd64 and run glibc 2.31 to 2.39 (Debian bullseye to Ubuntu 24.04); the
release's `ragon-x86_64-unknown-linux-gnu` needs glibc 2.39. As for SWE-bench
(`SweBenchRuntime`), the runtime is a directory placed at `/opt/ragon` in the container:

```text
bin/ragon, bin/codex-code-mode-host   the release binaries, ELF interpreter and rpath -> lib/
lib/                                    the loader, libc, libm and libgcc_s of Ubuntu 24.04 amd64
```

The libraries come from the Ubuntu archive's `libc6` and `libgcc-s1` packages (amd64), so the
runtime can be built on any machine, an arm64 GB10 included. Only the two binaries use them; the
commands the agent runs use the image's own libraries.
"""

import gzip
import hashlib
import shutil
import subprocess
import tempfile
from collections.abc import Callable, Iterable
from pathlib import Path
from typing import Final

# Where the runtime sits in every task container.
CONTAINER_MOUNT: Final[str] = "/opt/ragon"

# The release assets, by the name they get under bin/.
RELEASE_ASSETS: Final[dict[str, str]] = {
    "ragon": "ragon-x86_64-unknown-linux-gnu.gz",
    "codex-code-mode-host": "codex-code-mode-host-x86_64-unknown-linux-gnu.gz",
}

# The loader the binaries are pointed at, and the libraries copied beside it.
LOADER: Final[str] = "ld-linux-x86-64.so.2"
LIBRARIES: Final[tuple[str, ...]] = ("libc.so.6", "libm.so.6", "libgcc_s.so.1")

# Where the amd64 packages put their libraries.
DEB_LIBRARY_DIRS: Final[tuple[str, ...]] = ("usr/lib/x86_64-linux-gnu", "lib/x86_64-linux-gnu")

# A `sha256sum` line: the hash and the name.
SUMS_FIELDS: Final[int] = 2

Runner = Callable[[list[str]], subprocess.CompletedProcess]


def _run(argv: list[str]) -> subprocess.CompletedProcess:
    """Runs one build step, raising on failure."""
    return subprocess.run(argv, check=True, capture_output=True, text=True, timeout=300)


def sha256(path: Path) -> str:
    """The SHA-256 of a file, hex."""
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def check_sums(sums_file: Path, files: Iterable[Path]) -> None:
    """Checks files against a `sha256sum`-style list (`<hex>  <name>` per line).

    Args:
        sums_file: The release's sums file.
        files: The files to check; each must be listed by its base name.

    Raises:
        ValueError: A file is not listed or its hash differs.
    """
    listed: dict[str, str] = {}
    for line in sums_file.read_text().splitlines():
        parts = line.split()
        if len(parts) == SUMS_FIELDS:
            listed[parts[1].lstrip("*").rsplit("/", 1)[-1]] = parts[0].lower()
    for path in files:
        expected = listed.get(path.name)
        if expected is None:
            raise ValueError(f"{path.name} is not in {sums_file.name}")
        if sha256(path) != expected:
            raise ValueError(f"{path.name}: SHA-256 differs from {sums_file.name}")


def _library_dir(root: Path) -> Path:
    """The directory of an extracted package tree that holds the amd64 libraries."""
    for relative in DEB_LIBRARY_DIRS:
        if (root / relative / "libc.so.6").exists():
            return root / relative
    raise FileNotFoundError(f"no x86_64 libc.so.6 under {root}")


def build(
    target: Path,
    release_dir: Path,
    debs: Iterable[Path],
    patchelf: str = "patchelf",
    run: Runner = _run,
) -> Path:
    """Builds the runtime directory.

    Args:
        target: The directory to (re)create.
        release_dir: Holds the two `.gz` release assets (checked against a sums file beforehand
            with `check_sums`).
        debs: The amd64 `libc6` and `libgcc-s1` packages.
        patchelf: The `patchelf` binary.
        run: Runs a command (tests replace it).

    Returns:
        Path: `target`.

    Raises:
        FileNotFoundError: An asset or a library is missing.
        ValueError: A binary needs a library the runtime does not carry.
    """
    if target.exists():
        shutil.rmtree(target)
    (target / "bin").mkdir(parents=True)
    (target / "lib").mkdir()
    with tempfile.TemporaryDirectory() as scratch:
        tree = Path(scratch)
        for deb in debs:
            run(["dpkg-deb", "-x", str(deb), str(tree)])
        libraries = _library_dir(tree)
        for name in (LOADER, *LIBRARIES):
            shutil.copy2(libraries / name, target / "lib" / name, follow_symlinks=True)
    for name, asset in RELEASE_ASSETS.items():
        binary = target / "bin" / name
        with gzip.open(release_dir / asset, "rb") as source, binary.open("wb") as sink:
            shutil.copyfileobj(source, sink)
        binary.chmod(0o755)
        needed = run([patchelf, "--print-needed", str(binary)]).stdout.split()
        missing = sorted(set(needed) - set(LIBRARIES) - {LOADER})
        if missing:
            raise ValueError(f"{name} needs {', '.join(missing)}, which the runtime lacks")
        run(
            [
                patchelf,
                "--set-interpreter",
                f"{CONTAINER_MOUNT}/lib/{LOADER}",
                "--set-rpath",
                f"{CONTAINER_MOUNT}/lib",
                str(binary),
            ]
        )
    return target
