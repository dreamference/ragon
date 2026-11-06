"""The bundle a provisioning run installs from (specs/DREAMFERENCE_RAGON_FLEET.md §7.2).

`install.sh --from <dir>` takes a folder holding the same asset names and the same
`ragon-<target>.sha256sums` as a release, so a new node installs what this machine runs with no
GitHub token. Two sources:

- `this` (the default): the binaries installed here, gzipped under the release names, and a wheel
  of this checkout's `dreamference` package. A new node then runs this machine's own build, even
  one never released. On a release install there is no wheel to copy (pip keeps none), so `this`
  there means the installed version's release.
- `release[=X.Y.Z]`: a release's own assets, downloaded once here with this machine's `gh` login.
"""

import gzip
import hashlib
import os
import platform
import re
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Final

from dreamference.runner.safe_git import SafeGit

BUNDLE_ROOT: Final[str] = str(Path("~/.cache/dreamference/fleet").expanduser())

# The binaries a node needs, as the release names them; the first two are required. The optional
# ones are `install.sh`'s list on Linux: the local file index and the Signal bridge's
# daemon; a bundle without them would leave a node with neither.
REQUIRED_BINARIES: Final[tuple[str, ...]] = ("ragon", "codex-code-mode-host")
OPTIONAL_BINARIES: Final[tuple[str, ...]] = (
    "ragon-search",
    "ragon-fetch",
    "ragon-code",
    "ragon-docs",
    "ragon-signal",
    "ragon-delegate",
)

# The default `--from`: this machine's own build (or, on a release install, its release).
SOURCE_THIS: Final[str] = "this"
RELEASE_PREFIX: Final[str] = "release="

RELEASE_REPO: Final[str] = os.environ.get("RAGON_RELEASE_REPO", "dreamference/ragon")


class FleetBundle:
    """Builds the folder `install.sh --from` installs."""

    @classmethod
    def target(cls) -> str:
        """Name this machine's release target.

        Returns:
        str: This machine's release target, `aarch64-unknown-linux-gnu` on a GB10.
        """
        machine = {"arm64": "aarch64", "amd64": "x86_64"}.get(
            platform.machine(), platform.machine()
        )
        return f"{machine}-unknown-linux-gnu"

    @classmethod
    def build(cls, source: str = "this") -> Path | None:
        """Builds (or reuses) the bundle for one run.

        Args:
            source: `this`, `release` or `release=X.Y.Z`.

        Returns:
            Optional[Path]: The bundle folder, or None with the reason printed.
        """
        from dreamference.runner.codex_branded_builder import CodexBrandedBuilder

        if source == SOURCE_THIS and CodexBrandedBuilder.has_source():
            return cls.from_this()
        version = source.partition("=")[2] if source.startswith("release=") else None
        if source == SOURCE_THIS:
            from dreamference import __version__

            version = __version__
        empty = version is not None and not version.strip().lstrip("v")
        if empty or (source not in ("this", "release") and not source.startswith("release=")):
            print(
                f"❌ --from is `this` or `release[=X.Y.Z]` (a release needs a version after `=`), not `{source}`."
            )
            return None
        return cls.from_release(version)

    @classmethod
    def from_this(cls) -> Path | None:
        """Build a bundle of the installed binaries and a wheel of this checkout.

        Returns:
        Optional[Path]: A bundle of the binaries installed here and a wheel of this checkout,
        or None if a required binary is missing or the wheel could not be built.
        """
        from dreamference.runner.codex_branded_builder import INSTALL_DIR, REPO_ROOT

        target = cls.target()
        binaries = Path(INSTALL_DIR) / "bin"
        missing = [name for name in REQUIRED_BINARIES if not (binaries / name).is_file()]
        if missing:
            print(
                f"❌ {', '.join(missing)} not installed here; run `ragon-admin codex build` first."
            )
            return None
        digest = cls._digest(
            [
                binaries / name
                for name in (*REQUIRED_BINARIES, *OPTIONAL_BINARIES)
                if (binaries / name).is_file()
            ]
        )
        bundle = Path(BUNDLE_ROOT) / f"bundle-this-{digest[:12]}"
        if (
            (bundle / f"ragon-{target}.sha256sums").is_file()
            and any(bundle.glob("dreamference-*.whl"))
            and (bundle / "install.sh").is_file()
        ):
            return bundle
        staging = bundle.with_name(bundle.name + ".partial")
        shutil.rmtree(staging, ignore_errors=True)
        staging.mkdir(parents=True)
        sums = []
        for name in (*REQUIRED_BINARIES, *OPTIONAL_BINARIES):
            if not (binaries / name).is_file():
                continue
            asset = staging / f"{name}-{target}.gz"
            with (
                Path(binaries / name).open("rb") as raw,
                gzip.open(asset, "wb", compresslevel=6) as packed,
            ):
                shutil.copyfileobj(raw, packed)
            sums.append(f"{cls._sha256(asset)}  {asset.name}")
        (staging / f"ragon-{target}.sha256sums").write_text("\n".join(sums) + "\n")
        shutil.copy2(Path(REPO_ROOT) / "install.sh", staging / "install.sh")
        wheel = subprocess.run(
            [
                sys.executable,
                "-m",
                "pip",
                "wheel",
                "--no-deps",
                "--quiet",
                "-w",
                str(staging),
                REPO_ROOT,
            ],
            capture_output=True,
            text=True,
            check=False,
        )
        if wheel.returncode != 0 or not any(staging.glob("dreamference-*.whl")):
            print(f"❌ Could not build a wheel of this checkout: {wheel.stderr.strip()[-300:]}")
            return None
        (staging / "VERSION").write_text(f"this-{digest[:12]}\n")
        shutil.rmtree(bundle, ignore_errors=True)
        staging.rename(bundle)
        return bundle

    @classmethod
    def from_release(cls, version: str | None) -> Path | None:
        """Args: version: `X.Y.Z`, or None for the latest release.

        Returns:
            Optional[Path]: A bundle of the release's assets for this target, downloaded with
            this machine's `gh` login, or None.
        """
        if shutil.which("gh") is None:
            print("❌ --from release downloads with `gh`, which is not installed here.")
            return None
        try:
            tag = cls._download(version)
        except OSError as error:
            print(f"❌ {error}")
            return None
        return Path(BUNDLE_ROOT) / f"bundle-{tag}"

    @classmethod
    def _download(cls, version: str | None) -> str:
        """Downloads a release's assets for this target into its bundle folder, once.

        Args:
            version: `X.Y.Z`, or None for the latest release.

        Returns:
            str: The release's tag, which names the folder.

        Raises:
            OSError: The release was not found or not downloaded.
        """
        tag = f"v{version.lstrip('v')}" if version else cls.latest_tag()
        bundle = Path(BUNDLE_ROOT) / f"bundle-{tag}"
        target = cls.target()
        if (bundle / f"ragon-{target}.sha256sums").is_file():
            return tag
        bundle.mkdir(parents=True, exist_ok=True)
        command = [
            "gh",
            "release",
            "download",
            tag,
            "-R",
            RELEASE_REPO,
            "-D",
            str(bundle),
            "--clobber",
            "-p",
            f"*-{target}.gz",
            "-p",
            f"ragon-{target}.sha256sums",
            "-p",
            "SHA256SUMS*",
            "-p",
            "release-key-transition.pub*",
            "-p",
            "dreamference-*.whl",
            "-p",
            "install.sh",
        ]
        result = subprocess.run(command, capture_output=True, text=True, check=False)
        if result.returncode != 0:
            raise OSError(f"Could not download the release: {result.stderr.strip()[-300:]}")
        (bundle / "VERSION").write_text(f"{tag}\n")
        return tag

    @classmethod
    def latest_tag(cls) -> str:
        """The latest release's tag.

        A bundle records the tag, never "latest": a node installed from one release must not
        look current against the next.

        Returns:
            str: The tag, `vX.Y.Z`.

        Raises:
            OSError: `gh` did not name one.
        """
        result = subprocess.run(
            ["gh", "release", "view", "-R", RELEASE_REPO, "--json", "tagName", "-q", ".tagName"],
            capture_output=True,
            text=True,
            check=False,
        )
        tag = result.stdout.strip()
        if result.returncode != 0 or not re.fullmatch(r"v[0-9][0-9A-Za-z.+-]*", tag):
            raise OSError(
                f"Could not find the latest release: {result.stderr.strip()[-300:] or tag}"
            )
        return tag

    @classmethod
    def expected_version(cls, source: str = SOURCE_THIS) -> str | None:
        """The version a bundle of `source` would record, without building or downloading it.

        A dry run compares it with what a node has.

        Args:
            source: `this`, `release` or `release=X.Y.Z`.

        Returns:
            Optional[str]: `this-<digest>` for this checkout's build, `vX.Y.Z` for a named
            release, or None when it is only known after a download (the latest release).
        """
        from dreamference.runner.codex_branded_builder import INSTALL_DIR, CodexBrandedBuilder

        if source == SOURCE_THIS and CodexBrandedBuilder.has_source():
            binaries = Path(INSTALL_DIR) / "bin"
            present = [
                binaries / name
                for name in (*REQUIRED_BINARIES, *OPTIONAL_BINARIES)
                if (binaries / name).is_file()
            ]
            return f"this-{cls._digest(present)[:12]}" if present else None
        if source == SOURCE_THIS:
            from dreamference import __version__

            return f"v{__version__.lstrip('v')}"
        if source.startswith(RELEASE_PREFIX):
            if not source.partition("=")[2].strip().lstrip("v"):
                raise ValueError(
                    "--from release= needs a version: `release=X.Y.Z`, or `release` for the latest."
                )
            return f"v{source.partition('=')[2].lstrip('v')}"
        return None

    @classmethod
    def version(cls, bundle: Path) -> str:
        """Args: bundle: A bundle folder.

        Returns:
            str: What it records as its version (`this-<digest>` or a release tag).
        """
        try:
            return (bundle / "VERSION").read_text().strip()
        except OSError:
            return "bundle"

    @classmethod
    def files(cls, bundle: Path) -> list[Path]:
        """Args: bundle: A bundle folder.

        Returns:
            List[Path]: Every file in it, which is what is copied to a node.
        """
        return sorted(path for path in bundle.iterdir() if path.is_file())

    @classmethod
    def _sha256(cls, path: Path) -> str:
        digest = hashlib.sha256()
        with Path(path).open("rb") as handle:
            for chunk in iter(lambda: handle.read(1 << 20), b""):
                digest.update(chunk)
        return digest.hexdigest()

    @classmethod
    def _digest(cls, paths: list[Path]) -> str:
        """The bundle's name: a digest of the binaries and of the checkout's changes, cheap to recompute."""
        digest = hashlib.sha256()
        for path in paths:
            stat = path.stat()
            digest.update(f"{path.name}:{stat.st_size}:{int(stat.st_mtime)}".encode())
        try:
            from dreamference.runner.codex_branded_builder import REPO_ROOT

            head = SafeGit.run(["rev-parse", "HEAD"], REPO_ROOT).stdout.strip()
            dirty = SafeGit.run(["status", "--porcelain", "dreamference"], REPO_ROOT).stdout
            # The status names a changed file the same way after every further edit: its diff
            # is what changes. install.sh goes into the bundle too.
            dirty += SafeGit.run(
                ["diff", "HEAD", "--binary", "--", "dreamference", "install.sh"], REPO_ROOT
            ).stdout
            digest.update(f"{head}:{hashlib.sha256(dirty.encode()).hexdigest()}".encode())
        except OSError:
            pass
        return digest.hexdigest()
