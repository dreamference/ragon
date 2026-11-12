"""The Docker network Dreamference's own sidecars are created on.

It also holds the check for a container that was not created on one.

A container's DNS setup is decided by the network it is *created* on, and attaching it to another
network later does not change it. On Docker's default bridge a container gets a copy of the host's
upstream DNS servers (`/run/systemd/resolve/resolv.conf`) taken when it starts. On a user-defined
network its lookups go through Docker's resolver to the host's own stub resolver
(`127.0.0.53`), at lookup time.

The copy is what failed: after a reboot Docker restarted `dreamference-searxng` and
`dreamference-stt` five seconds before the Wi-Fi had a DNS server, both copied an empty list
("NO EXTERNAL NAMESERVERS DEFINED" in their `/etc/resolv.conf`), and every search engine answered
"HTTP connection error" until the containers were restarted by hand. The Gmail sidecar, on a
user-defined network, came through the same boot unharmed.
"""

import subprocess
from typing import Final

# User-defined, so its containers resolve names through the host's resolver as it is at lookup
# time. A network of its own, because SearXNG serves `ragon-search` and the MCP server on
# machines where the web UI is not installed.
SIDECAR_NETWORK: Final[str] = "dreamference-sidecars"

# Docker's error text when the network exists.
ALREADY_EXISTS: Final[str] = "already exists"

# `HostConfig.NetworkMode` of a container created without `--network`: `bridge` from the CLI,
# `default` from some API clients.
DEFAULT_BRIDGE_MODES: Final[frozenset] = frozenset({"bridge", "default"})


class SidecarNetwork:
    """Creates the sidecar network and tells which containers still sit on the default bridge."""

    @classmethod
    def ensure(cls) -> bool:
        """Creates the sidecar network unless it exists.

        Returns:
            bool: True if the network exists once this returns.
        """
        exists = subprocess.run(
            ["docker", "network", "inspect", SIDECAR_NETWORK, "--format", "{{.Name}}"],
            capture_output=True,
            text=True,
            timeout=30,
            check=False,
        )
        if exists.returncode == 0:
            return True
        created = subprocess.run(
            ["docker", "network", "create", SIDECAR_NETWORK],
            capture_output=True,
            text=True,
            timeout=30,
            check=False,
        )
        # Two commands racing to create it: the loser's "already exists" is success.
        return created.returncode == 0 or ALREADY_EXISTS in created.stderr

    @classmethod
    def network_mode(cls, container: str) -> str:
        """Reports the network a container was created on.

        Args:
            container: The container's name.

        Returns:
            str: Its `HostConfig.NetworkMode`, or an empty string if there is no such container.
        """
        result = subprocess.run(
            ["docker", "inspect", container, "--format", "{{.HostConfig.NetworkMode}}"],
            capture_output=True,
            text=True,
            timeout=30,
            check=False,
        )
        return result.stdout.strip() if result.returncode == 0 else ""

    @classmethod
    def created_on_default_bridge(cls, container: str) -> bool:
        """Tells whether a container exists and was created on Docker's default bridge.

        There its DNS servers are a copy taken at start.

        Args:
            container: The container's name.

        Returns:
            bool: True if it must be recreated to resolve names reliably after a reboot.
        """
        return cls.network_mode(container) in DEFAULT_BRIDGE_MODES
