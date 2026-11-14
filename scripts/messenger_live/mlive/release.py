"""The build the live test judges: the latest published release, installed into a prefix of its own.

The suite used to test whatever `~/.local/bin/ragon` happened to be, which could be a stale
install. `install_latest()` runs the repository's `install.sh` (the client role, the same checks as
a user's install) with a home of its own under the suite's cache, so the machine's own install is
never touched, and the run tests and reports that release.
"""

from __future__ import annotations

import os
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Final

INSTALL_TIMEOUT: Final = 1800
# The flag the after-merge check leaves for the next run: test the installed build, which it has
# just matched to the merge, instead of the (older) latest release.
USE_INSTALLED: Final = "use-installed"


@dataclass
class Release:
    """What `install_latest()` made."""

    ragon: Path
    tag: str


def prefix_home(cache: Path) -> Path:
    """The prefix's own home folder."""
    return cache / "release-home"


def install_latest(cache: Path, installer: Path) -> Release:
    """Installs the latest published release's client into the suite's own prefix.

    Args:
        cache: The suite's cache folder.
        installer: The repository's `install.sh`.

    Returns:
        Release: The prefix's `ragon` and the release tag.

    Raises:
        RuntimeError: When the installer failed or left no `ragon`.
    """
    home = prefix_home(cache)
    home.mkdir(parents=True, exist_ok=True)
    env = {k: v for k, v in os.environ.items() if not k.startswith("RAGON_")}
    env["HOME"] = str(home)
    env.pop("XDG_STATE_HOME", None)
    if not env.get("GH_TOKEN") and not env.get("GITHUB_TOKEN"):
        # The prefix's home has no `gh` login: pass the token on, which only raises the rate limit.
        done = subprocess.run(
            ["gh", "auth", "token"], capture_output=True, text=True, check=False, timeout=60
        )
        if done.returncode == 0 and done.stdout.strip():
            env["GH_TOKEN"] = done.stdout.strip()
    done = subprocess.run(
        ["bash", str(installer), "--role", "client", "--no-advertise"],
        capture_output=True,
        text=True,
        check=False,
        timeout=INSTALL_TIMEOUT,
        env=env,
        stdin=subprocess.DEVNULL,
    )
    if done.returncode != 0:
        tail = " | ".join((done.stdout + done.stderr).strip().splitlines()[-3:])
        raise RuntimeError(f"install.sh exited {done.returncode}: {tail}"[:400])
    ragon = home / ".local" / "bin" / "ragon"
    version_file = home / ".local" / "share" / "dreamference" / "ragon" / "VERSION"
    if not ragon.exists() or not version_file.is_file():
        raise RuntimeError("install.sh left no ragon in the prefix")
    return Release(ragon, version_file.read_text().splitlines()[0].strip())
