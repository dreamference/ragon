"""The private Matrix homeserver behind `ragon chat`'s Matrix adapter.

`ragon-admin matrix …` (specs/DREAMFERENCE_RAGON_CHAT.md §5).

tuwunel runs as the container `dreamference-matrix` on an **internal** Docker network, so it has no
route to anything but this host: no federation, no URL previews, no push gateway, because nothing
can be reached, not because a setting says so. The host reaches it through a loopback proxy (two
user units running systemd's own `systemd-socket-proxyd`), and the phone, on a node with remote
access, through a TLS unit on the overlay address (a certificate for the node's overlay name from
its own CA, `serve_on_overlay`), otherwise through `tailscale serve` in front of that proxy, under
the node's tailnet name. That name is the Matrix server name, which is
permanent (every account's id ends in it), so it is chosen once, kept in `matrix-admin.json`, and a
later `start` refuses when Tailscale reports another one.

The bridge reads `matrix.json` (the homeserver URL, the bot's token, the allow-list); this module
writes it. Everything that touches Docker, systemd, Tailscale or HTTP goes through `_run` and
`_http`, so the tests replace those two and nothing else.
"""

import contextlib
import json
import logging
import os
import pathlib
import re
import secrets
import string
import subprocess
import sys
import time
import urllib.error
import urllib.request
from typing import Any, Final

LOG: Final[logging.Logger] = logging.getLogger(__name__)

MATRIX_CONTAINER: Final[str] = "dreamference-matrix"
# tuwunel 1.9.3, the multi-architecture index, pinned by digest.
MATRIX_IMAGE: Final[str] = (
    "ghcr.io/matrix-construct/tuwunel:v1.9.3"
    "@sha256:678b7f5350e06a41614444497c587da9dddf66767e4068a27480402f3c1367d0"
)
# The variable the container reads its registration token from; `docker run` takes the value from
# its own environment, never its argv (`run_command`).
TOKEN_ENV: Final[str] = "TUWUNEL_REGISTRATION_TOKEN"
MATRIX_VOLUME: Final[str] = "dreamference-matrix-data"
# The internal network: no route out. A fixed subnet so the container's address never moves.
MATRIX_NETWORK: Final[str] = "dreamference-matrix"
MATRIX_SUBNET: Final[str] = "172.31.231.0/24"
MATRIX_ADDRESS: Final[str] = "172.31.231.10"
MATRIX_PORT: Final[int] = 6167
# Where the host (the bridge, `tailscale serve`) reaches it: the loopback proxy.
PROXY_URL: Final[str] = f"http://127.0.0.1:{MATRIX_PORT}"
PROXY_SOCKET_UNIT: Final[str] = "ragon-matrix-proxy.socket"
PROXY_SERVICE_UNIT: Final[str] = "ragon-matrix-proxy.service"
# On a node with remote access the phone reaches the homeserver by the node's overlay name, over TLS
# from the node's own CA: a user unit running `socat`, bound to the overlay address only, in front of
# the loopback proxy. 8448 is Matrix's alternate HTTPS port; it needs no privilege to bind.
TLS_SERVICE_UNIT: Final[str] = "ragon-matrix-tls.service"
TLS_PORT: Final[int] = 8448
TLS_CERTIFICATE_LABEL: Final[str] = "matrix"
OVERLAY_INTERFACE: Final[str] = "wt0"
SOCKET_PROXYD: Final[str] = "/usr/lib/systemd/systemd-socket-proxyd"
BOT_LOCALPART: Final[str] = "ragon"
MEMORY_LIMIT: Final[str] = "1g"
TAILSCALE_INSTALL: Final[str] = (
    "Matrix reaches your phone through Tailscale, which is not running here. Install it and sign in:\n"
    "    curl -fsSL https://tailscale.com/install.sh | sh\n"
    "    sudo tailscale up\n"
    "then install Tailscale on your phone, signed in to the same tailnet, and run this again.\n"
    "(Headscale, the self-hosted coordination server, works the same way.)"
)
HTTP_OK: Final[int] = 200
HTTP_UNAUTHORIZED: Final[int] = 401
BACKEND_RUNNING: Final[str] = "Running"
UNIT_ACTIVE: Final[str] = "active"
DOCKER_TRUE: Final[str] = "true"
AUTH_SESSION_KEY: Final[str] = "session"
STAGE_TOKEN: Final[str] = "m.login.registration_token"
STAGE_DUMMY: Final[str] = "m.login.dummy"
LOCALPART: Final = re.compile(r"^[a-z0-9._=-]{1,64}$")


NETWORK_OVERLAY: Final[str] = "overlay"
NETWORK_TAILSCALE: Final[str] = "tailscale"


class MatrixHomeserver:
    """`ragon-admin matrix start|stop|status|add-user|push|remove`."""

    # -- seams the tests replace --------------------------------------------------------------

    @classmethod
    def _run(
        cls, argv: list[str], timeout: int = 120, env: dict[str, str] | None = None
    ) -> subprocess.CompletedProcess:
        """Runs a command, capturing its output.

        A missing program (Tailscale removed since `start`, say) is a failed step, not an
        exception, so `stop` and `remove` still do the rest. `env` adds variables to the
        command's environment (a secret `docker run -e NAME` passes on).
        """
        try:
            return subprocess.run(
                argv,
                capture_output=True,
                text=True,
                timeout=timeout,
                check=False,
                env={**os.environ, **env} if env else None,
            )
        except FileNotFoundError as error:
            return subprocess.CompletedProcess(argv, 127, "", str(error))

    @classmethod
    def _http(
        cls,
        method: str,
        url: str,
        body: dict[str, Any] | None = None,
        token: str | None = None,
    ) -> tuple[int, dict[str, Any]]:
        """One JSON request to the homeserver through the loopback proxy."""
        data = json.dumps(body).encode() if body is not None else None
        request = urllib.request.Request(url, data=data, method=method)
        request.add_header("Content-Type", "application/json")
        if token:
            request.add_header("Authorization", f"Bearer {token}")
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                return response.status, json.loads(response.read() or b"{}")
        except urllib.error.HTTPError as err:
            try:
                return err.code, json.loads(err.read() or b"{}")
            except ValueError:
                return err.code, {}
        except (urllib.error.URLError, OSError, ValueError):
            return 0, {}

    # -- files ----------------------------------------------------------------------------------

    @classmethod
    def chat_dir(cls) -> str:
        """`~/.ragon/chat` (or `$CODEX_HOME/chat`): where the bridge keeps its files."""
        from dreamference.runner.codex_installer import CodexInstaller

        return str(pathlib.Path(CodexInstaller.home_dir()) / "chat")

    @classmethod
    def _path(cls, name: str) -> str:
        """The path of one of the bridge's files in `chat_dir()`."""
        return str(pathlib.Path(cls.chat_dir()) / name)

    @classmethod
    def _load(cls, name: str) -> dict[str, Any]:
        try:
            with pathlib.Path(cls._path(name)).open(encoding="utf-8") as handle:
                value = json.load(handle)
            return value if isinstance(value, dict) else {}
        except (OSError, ValueError):
            return {}

    @classmethod
    def _save(cls, name: str, value: dict[str, Any]) -> None:
        """Writes a file readable only by the user, replacing it whole."""
        directory = cls.chat_dir()
        pathlib.Path(directory).mkdir(exist_ok=True, parents=True)
        pathlib.Path(directory).chmod(0o700)
        staging = cls._path(f"{name}.tmp-{os.getpid()}")
        fd = os.open(staging, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
        with os.fdopen(fd, "w", encoding="utf-8") as handle:
            json.dump(value, handle, indent=2)
        pathlib.Path(staging).replace(cls._path(name))

    # -- Tailscale ------------------------------------------------------------------------------

    @classmethod
    def tailscale_name(cls) -> str | None:
        """Returns this machine's tailnet name.

        Returns:
            Optional[str]: The name (`node.tail1234.ts.net`) when Tailscale is running and
            signed in, else None.
        """
        try:
            result = cls._run(["tailscale", "status", "--json"], timeout=30)
        except (OSError, subprocess.SubprocessError):
            return None
        if result.returncode != 0:
            return None
        try:
            status = json.loads(result.stdout)
        except ValueError:
            return None
        if status.get("BackendState") != BACKEND_RUNNING:
            return None
        name = (status.get("Self") or {}).get("DNSName") or ""
        return name.rstrip(".") or None

    # -- the overlay -------------------------------------------------------------------------

    @classmethod
    def overlay_name(cls) -> str | None:
        """The node's overlay name, when remote access is set up on this machine.

        Returns:
            Optional[str]: `ragon-<id>.netbird.selfhosted`, or None without remote access.
        """
        from dreamference.node.node_identity import NodeIdentity
        from dreamference.remote.remote_settings import RemoteSettings

        node_id = NodeIdentity.read()
        if not node_id or not RemoteSettings.read():
            return None
        return RemoteSettings.overlay_name(node_id)

    @classmethod
    def overlay_address(cls) -> str | None:
        """The address the overlay interface holds here (the only one the TLS unit binds).

        Returns:
            Optional[str]: The IPv4 address, or None when the overlay is not up.
        """
        result = cls._run(["ip", "-j", "-4", "addr", "show", "dev", OVERLAY_INTERFACE], timeout=30)
        if result.returncode != 0:
            return None
        try:
            interfaces = json.loads(result.stdout)
            return str(interfaces[0]["addr_info"][0]["local"])
        except (ValueError, LookupError, TypeError):
            return None

    @classmethod
    def tls_unit(cls, address: str, certificate: str, key: str) -> str:
        """The TLS unit's text.

        Args:
            address: The overlay address to bind.
            certificate: The certificate file.
            key: Its key file.

        Returns:
            str: The unit file.
        """
        listen = (
            f"OPENSSL-LISTEN:{TLS_PORT},bind={address},reuseaddr,fork,verify=0,"
            f"cert={certificate},key={key}"
        )
        return (
            "# Written by `ragon-admin matrix start`: the homeserver on the overlay, over TLS from the\n"
            "# node's CA (specs/DREAMFERENCE_RAGON_REMOTE_ACCESS.md §8).\n"
            "[Unit]\nDescription=Ragon Matrix homeserver, on the overlay\n"
            f"After={PROXY_SOCKET_UNIT}\n\n"
            f"[Service]\nExecStart=socat {listen} TCP:127.0.0.1:{MATRIX_PORT}\n"
            "Restart=always\nRestartSec=5\n\n[Install]\nWantedBy=default.target\n"
        )

    @classmethod
    def serve_on_overlay(cls, name: str) -> bool:
        """Issues the certificate for `name` and (re)starts the TLS unit. Logs why on failure.

        Args:
            name: The node's overlay name.

        Returns:
            bool: True when the homeserver is offered on the overlay.
        """
        from dreamference.remote.remote_certificate_authority import RemoteCertificateAuthority

        if cls._run(["socat", "-V"], timeout=30).returncode != 0:
            LOG.error(
                "❌ The overlay path needs `socat` (sudo apt install socat); then run this again."
            )
            return False
        address = cls.overlay_address()
        if not address:
            LOG.error(
                f"❌ The overlay interface {OVERLAY_INTERFACE} has no address: is this node enrolled "
                "(`ragon-admin remote status`)?"
            )
            return False
        issued = RemoteCertificateAuthority.ensure_certificate(TLS_CERTIFICATE_LABEL, [name])
        if issued is None:
            LOG.error("❌ The node's CA could not issue the homeserver's certificate.")
            return False
        directory = pathlib.Path(cls.unit_dir())
        directory.mkdir(exist_ok=True, parents=True)
        (directory / TLS_SERVICE_UNIT).write_text(
            cls.tls_unit(address, str(issued[0]), str(issued[1])), encoding="utf-8"
        )
        steps = [
            ["systemctl", "--user", "daemon-reload"],
            ["systemctl", "--user", "enable", TLS_SERVICE_UNIT],
            ["systemctl", "--user", "restart", TLS_SERVICE_UNIT],
        ]
        if not all(cls._run(step, timeout=60).returncode == 0 for step in steps):
            LOG.error("❌ Could not start the TLS unit (systemd user units).")
            return False
        return True

    @classmethod
    def stop_overlay(cls) -> None:
        """Takes the homeserver off the overlay (a no-op when it was never on it)."""
        cls._run(["systemctl", "--user", "disable", "--now", TLS_SERVICE_UNIT], timeout=60)

    # -- Docker ---------------------------------------------------------------------------------

    @classmethod
    def run_command(cls, server_name: str, push: bool) -> list[str]:
        """Builds the `docker run` command.

        The registration token (`TOKEN_ENV`) is named, not given: `docker run` reads it from its own
        environment (`start`), so it never appears in an argv other local users can read in `/proc`.

        Args:
            server_name: The permanent server name.
            push: True puts the container on the sidecar network (push notifications can leave,
                with event ids only); False, the default, on the internal network.

        Returns:
            List[str]: The argv.
        """
        from dreamference.chat.sidecar_network import SIDECAR_NETWORK

        network = (
            ["--network", SIDECAR_NETWORK]
            if push
            else ["--network", MATRIX_NETWORK, "--ip", MATRIX_ADDRESS]
        )
        env = {
            "TUWUNEL_SERVER_NAME": server_name,
            "TUWUNEL_DATABASE_PATH": "/var/lib/tuwunel",
            "TUWUNEL_ADDRESS": "0.0.0.0",
            "TUWUNEL_PORT": str(MATRIX_PORT),
            "TUWUNEL_ALLOW_FEDERATION": "false",
            "TUWUNEL_ALLOW_REGISTRATION": "true",
            "TUWUNEL_TRUSTED_SERVERS": "[]",
            "TUWUNEL_URL_PREVIEW_DOMAIN_EXPLICIT_ALLOWLIST": "[]",
        }
        argv = [
            "docker",
            "run",
            "-d",
            "--name",
            MATRIX_CONTAINER,
            "--restart",
            "unless-stopped",
            *network,
            f"--memory={MEMORY_LIMIT}",
            f"--memory-swap={MEMORY_LIMIT}",
            "--stop-timeout",
            "600",
            "-v",
            f"{MATRIX_VOLUME}:/var/lib/tuwunel",
        ]
        for key, value in env.items():
            argv += ["-e", f"{key}={value}"]
        return [*argv, "-e", TOKEN_ENV, MATRIX_IMAGE]

    @classmethod
    def ensure_network(cls) -> bool:
        """Creates the internal network once. Returns True if it exists afterwards."""
        if cls._run(["docker", "network", "inspect", MATRIX_NETWORK], timeout=30).returncode == 0:
            return True
        created = cls._run(
            [
                "docker",
                "network",
                "create",
                "--internal",
                "--subnet",
                MATRIX_SUBNET,
                MATRIX_NETWORK,
            ],
            timeout=60,
        )
        return created.returncode == 0

    @classmethod
    def container_exists(cls) -> bool:
        """True if the homeserver's container exists, running or not."""
        return cls._run(["docker", "inspect", MATRIX_CONTAINER], timeout=30).returncode == 0

    @classmethod
    def container_running(cls) -> bool:
        """True if the homeserver's container is running."""
        result = cls._run(
            ["docker", "inspect", "-f", "{{.State.Running}}", MATRIX_CONTAINER], timeout=30
        )
        return result.returncode == 0 and result.stdout.strip() == DOCKER_TRUE

    @classmethod
    def container_address(cls) -> str | None:
        """The container's address on whichever network it is on."""
        result = cls._run(
            [
                "docker",
                "inspect",
                "-f",
                "{{range .NetworkSettings.Networks}}{{.IPAddress}} {{end}}",
                MATRIX_CONTAINER,
            ],
            timeout=30,
        )
        if result.returncode != 0:
            return None
        addresses = result.stdout.split()
        return addresses[0] if addresses else None

    # -- the loopback proxy -----------------------------------------------------------------------

    @classmethod
    def unit_dir(cls) -> str:
        """The systemd user unit directory."""
        return str(pathlib.Path("~/.config/systemd/user").expanduser())

    @classmethod
    def proxy_units(cls, target: str) -> dict[str, str]:
        """Args: target: `address:port` of the container.

        Returns:
            Dict[str, str]: Unit file name → text.
        """
        socket = (
            "# Written by `ragon-admin matrix start`: the homeserver on loopback (RAGON_CHAT §5.2).\n"
            "[Unit]\nDescription=Ragon Matrix homeserver, on loopback\n\n"
            f"[Socket]\nListenStream=127.0.0.1:{MATRIX_PORT}\n\n"
            "[Install]\nWantedBy=sockets.target\n"
        )
        service = (
            "# Written by `ragon-admin matrix start`.\n"
            "[Unit]\nDescription=Ragon Matrix homeserver proxy\n"
            f"Requires={PROXY_SOCKET_UNIT}\nAfter={PROXY_SOCKET_UNIT}\n\n"
            f"[Service]\nExecStart={SOCKET_PROXYD} {target}\n"
        )
        return {PROXY_SOCKET_UNIT: socket, PROXY_SERVICE_UNIT: service}

    @classmethod
    def install_proxy(cls, target: str) -> bool:
        """Writes the proxy units, then (re)starts them. Returns True on success."""
        directory = cls.unit_dir()
        pathlib.Path(directory).mkdir(exist_ok=True, parents=True)
        for name, text in cls.proxy_units(target).items():
            with (pathlib.Path(directory) / name).open("w", encoding="utf-8") as handle:
                handle.write(text)
        steps = [
            ["systemctl", "--user", "daemon-reload"],
            ["systemctl", "--user", "stop", PROXY_SERVICE_UNIT],
            ["systemctl", "--user", "enable", "--now", PROXY_SOCKET_UNIT],
        ]
        return all(cls._run(step, timeout=60).returncode == 0 for step in steps)

    # -- accounts -------------------------------------------------------------------------------

    @classmethod
    def wait_until_answering(cls, seconds: int = 60) -> bool:
        """Waits up to `seconds` for the homeserver to answer; True if it did."""
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            status, _ = cls._http("GET", f"{PROXY_URL}/_matrix/client/versions")
            if status == HTTP_OK:
                return True
            time.sleep(1)
        return False

    @classmethod
    def register(cls, localpart: str, password: str, token: str) -> dict[str, Any] | None:
        """Creates an account through the registration-token flow.

        Returns:
            Optional[Dict[str, Any]]: The server's answer (`user_id`, `access_token`), or None.
        """
        url = f"{PROXY_URL}/_matrix/client/v3/register"
        body: dict[str, Any] = {
            "username": localpart,
            "password": password,
            "inhibit_login": False,
            "initial_device_display_name": "ragon-admin",
        }
        status, answer = cls._http("POST", url, body)
        for _ in range(3):
            if status == HTTP_OK:
                return answer
            if status != HTTP_UNAUTHORIZED or AUTH_SESSION_KEY not in answer:
                return None
            completed = answer.get("completed") or []
            stage = STAGE_TOKEN if STAGE_TOKEN not in completed else STAGE_DUMMY
            auth: dict[str, Any] = {"type": stage, "session": answer["session"]}
            if stage == STAGE_TOKEN:
                auth["token"] = token
            status, answer = cls._http("POST", url, {**body, "auth": auth})
        return answer if status == HTTP_OK else None

    @classmethod
    def password(cls) -> str:
        """A password to type once on a phone: 20 letters and digits in groups of five."""
        alphabet = string.ascii_letters + string.digits
        raw = "".join(secrets.choice(alphabet) for _ in range(20))
        return "-".join(raw[i : i + 5] for i in range(0, 20, 5))

    @classmethod
    def _ensure_container(cls, name: str, admin: dict[str, Any], push: bool) -> bool:
        """Starts the container, creating it first when it does not exist yet.

        Returns:
            bool: False (after printing why) if it could not be created.
        """
        if cls.container_exists():
            cls._run(["docker", "start", MATRIX_CONTAINER], timeout=60)
            return True
        if push:
            from dreamference.chat.sidecar_network import SidecarNetwork

            SidecarNetwork.ensure()
        created = cls._run(
            cls.run_command(name, push),
            timeout=600,
            env={TOKEN_ENV: admin["registration_token"]},
        )
        if created.returncode != 0:
            print(f"❌ The homeserver did not start: {created.stderr.strip()[:500]}")
            return False
        return True

    @classmethod
    def _ensure_bot(cls, config: dict[str, Any], admin: dict[str, Any]) -> bool:
        """Keeps the bot's token if the server still accepts it, else registers the bot anew.

        Returns:
            bool: False (after printing why) if the bot account could not be created.
        """
        token = config.get("access_token")
        if token:
            status, _ = cls._http(
                "GET", f"{PROXY_URL}/_matrix/client/v3/account/whoami", token=token
            )
            if (
                status == HTTP_UNAUTHORIZED
            ):  # only this says the token is gone; the name stays taken
                token = None
        if not token:
            answer = cls.register(
                BOT_LOCALPART, cls.password() + cls.password(), admin["registration_token"]
            )
            if not answer or not answer.get("access_token"):
                print("❌ Could not create the bot account @ragon.")
                return False
            config.update({"user_id": answer["user_id"], "access_token": answer["access_token"]})
        return True

    # -- commands -------------------------------------------------------------------------------

    @classmethod
    def _say(cls, line: str) -> None:
        """Prints one line of what the command is doing or why it stopped."""
        sys.stdout.write(line + "\n")

    @classmethod
    def _existing_name(cls, admin: dict[str, Any], network: str | None) -> str | None:
        """The name an existing homeserver keeps, or None (after saying so) if `network` contradicts it."""
        overlay = cls.overlay_name()
        kept = admin.get("server_name")
        if network == NETWORK_OVERLAY and kept != overlay:
            cls._say(
                f"❌ This homeserver is {kept}: a Matrix server name cannot change, so it stays on its "
                "network until `ragon-admin matrix remove --yes`; then start again with --network overlay."
            )
            return None
        if network == NETWORK_TAILSCALE and kept == overlay:
            cls._say(
                f"❌ This homeserver is {kept}, on the remote-access overlay, and cannot move: run "
                "`ragon-admin matrix remove --yes` (it deletes every account) and start again with "
                "--network tailscale."
            )
            return None
        return overlay if overlay and kept == overlay else cls.tailscale_name()

    @classmethod
    def _chosen_name(cls, admin: dict[str, Any], network: str | None = None) -> str | None:
        """The server name this start uses.

        A new homeserver on a node with remote access is named for the overlay unless Tailscale
        is asked for; an existing one keeps its name until `matrix remove` (REMOTE_ACCESS §14.4).

        Args:
            admin: The admin file (empty for a new homeserver).
            network: `overlay`, `tailscale` or None for the default.

        Returns:
            Optional[str]: The overlay name or the tailnet name; None (after printing why) when
            the one asked for is not available.
        """
        if admin:
            return cls._existing_name(admin, network)
        overlay = cls.overlay_name()
        if network == NETWORK_OVERLAY and not overlay:
            cls._say(
                "❌ This machine has no remote access yet, so there is no overlay name: set it up with "
                "`ragon-admin remote`, or use --network tailscale."
            )
            return None
        if overlay and network != NETWORK_TAILSCALE:
            cls._say(
                f"💡 New homeserver on the remote-access overlay, as {overlay} (--network tailscale for Tailscale)."
            )
            return overlay
        name = cls.tailscale_name()
        if name:
            cls._say(f"💡 New homeserver on Tailscale, as {name}.")
        return name

    @classmethod
    def _offer(cls, name: str) -> str | None:
        """Offers the homeserver outside this machine: on the overlay, else through Tailscale.

        Args:
            name: The server name.

        Returns:
            Optional[str]: The URL a phone signs in to; None (after printing why) on the overlay
            path when it could not be set up.
        """
        if name == cls.overlay_name():
            return f"https://{name}:{TLS_PORT}" if cls.serve_on_overlay(name) else None
        served = cls._run(["tailscale", "serve", "--bg", "--https=443", PROXY_URL], timeout=60)
        if served.returncode != 0:
            print(
                "⚠️  `tailscale serve` refused; if HTTPS is off for your tailnet, turn it on once in the Tailscale "
                f"admin console (DNS → HTTPS Certificates), then run this again. ({served.stderr.strip()[:300]})"
            )
        return f"https://{name}"

    @classmethod
    def _prepare(cls, network: str | None = None) -> tuple[str, dict[str, Any], bool] | None:
        """Checks the machine and the server name, and creates the admin file and network.

        Returns:
            tuple[str, dict[str, Any], bool] | None: The tailnet name, the admin file and whether
            push is on; None (after printing why) when `start` cannot go on.
        """
        from dreamference.node.node_identity import NodeIdentity

        if not NodeIdentity.read():
            print(
                "❌ The homeserver belongs on a node, which is always on: run this on the machine that serves "
                "the model (`ragon-admin node enable`)."
            )
            return None
        admin = cls._load("matrix-admin.json")
        name = cls._chosen_name(admin, network)
        if not name:
            if not (network == NETWORK_OVERLAY or (admin and network)):
                print(f"❌ {TAILSCALE_INSTALL}")
            return None
        if admin.get("server_name") and admin["server_name"] != name:
            print(
                f"❌ This homeserver is {admin['server_name']}, but Tailscale now names this machine {name}.\n"
                "   A Matrix server name cannot change: every account's id ends in it. Rename the machine back in "
                "Tailscale, or delete the homeserver and its accounts with `ragon-admin matrix remove --yes` and "
                "start again."
            )
            return None
        if not admin:
            admin = {
                "server_name": name,
                "registration_token": secrets.token_hex(32),
                "push": False,
            }
            cls._save("matrix-admin.json", admin)
        push = bool(admin.get("push"))
        if not push and not cls.ensure_network():
            print(f"❌ Could not create the internal Docker network {MATRIX_NETWORK}.")
            return None
        return name, admin, push

    @classmethod
    def _bring_up(cls, name: str, admin: dict[str, Any], push: bool) -> bool:
        """Starts the container and the loopback proxy and waits for the homeserver to answer.

        Returns:
            bool: False (after printing why) when it did not come up.
        """
        if not cls._ensure_container(name, admin, push):
            return False
        address = cls.container_address() or MATRIX_ADDRESS
        if not cls.install_proxy(f"{address}:{MATRIX_PORT}"):
            print("❌ Could not start the loopback proxy (systemd user units).")
            return False
        if not cls.wait_until_answering():
            print(
                f"❌ The homeserver is not answering on {PROXY_URL}; `docker logs {MATRIX_CONTAINER}` says why."
            )
            return False
        return True

    @classmethod
    def start(cls, network: str | None = None) -> int:
        """Starts (or creates) the homeserver, the proxy, the bot account and `tailscale serve`.

        Args:
            network: `overlay`, `tailscale` or None (the overlay when remote access is set up).

        Returns:
            int: The exit code.
        """
        prepared = cls._prepare(network)
        if prepared is None:
            return 1
        name, admin, push = prepared
        if not cls._bring_up(name, admin, push):
            return 1
        config = cls._load("matrix.json")
        if not cls._ensure_bot(config, admin):
            return 1
        config.update({"homeserver": PROXY_URL, "server_name": name})
        config.setdefault("allowed", [])
        cls._save("matrix.json", config)
        url = cls._offer(name)
        if url is None:
            return 1
        print(f"✅ The Matrix homeserver is running as {url} (no route out of this machine).")
        if not config["allowed"]:
            print(
                "   Next: `ragon-admin matrix add-user <name>` for your account, then `ragon chat start`."
            )
        return 0

    @classmethod
    def add_user(cls, localpart: str) -> int:
        """Creates a user account, prints its password once, and allows it to talk to the bot."""
        if not LOCALPART.match(localpart):
            print("❌ A user name is lowercase letters, digits and . _ = - only.")
            return 2
        admin = cls._load("matrix-admin.json")
        if not admin:
            print("❌ No homeserver yet: run `ragon-admin matrix start` first.")
            return 1
        password = cls.password()
        answer = cls.register(localpart, password, admin["registration_token"])
        if not answer or not answer.get("user_id"):
            print(
                f"❌ Could not create {localpart} (is the name taken, or the homeserver stopped?)."
            )
            return 1
        config = cls._load("matrix.json")
        allowed = config.setdefault("allowed", [])
        if answer["user_id"] not in allowed:
            allowed.append(answer["user_id"])
        cls._save("matrix.json", config)
        if admin["server_name"] == cls.overlay_name():
            where = f"with the overlay on): sign in to https://{admin['server_name']}:{TLS_PORT}"
        else:
            where = f"with Tailscale on): sign in to https://{admin['server_name']}"
        print(
            f"✅ Created {answer['user_id']}. Its password, shown this once and stored nowhere:\n\n"
            f"    {password}\n\n"
            f"In Element X on your phone ({where} as "
            f"{localpart} with that password, then accept the invite from Ragon. "
            "`ragon chat start` (or a restart of it) opens the room."
        )
        return 0

    @classmethod
    def _take_down_front(cls) -> None:
        """Stops whatever offers the homeserver outside this machine.

        That is the TLS unit on the overlay for an overlay-named homeserver, `tailscale serve` for a
        tailnet-named one.
        """
        name = cls._load("matrix-admin.json").get("server_name")
        if name and name == cls.overlay_name():
            cls.stop_overlay()
        else:
            cls._run(["tailscale", "serve", "--https=443", "off"], timeout=60)

    @classmethod
    def stop(cls) -> int:
        """Turns the homeserver off until the next `start`.

        `tailscale serve` no longer offers it, the proxy socket is disabled (so a reboot does not
        listen again) and the container is stopped (`unless-stopped` keeps it stopped). The
        accounts and messages stay in the volume.
        """
        cls._take_down_front()
        cls._run(["systemctl", "--user", "disable", "--now", PROXY_SOCKET_UNIT], timeout=60)
        cls._run(["systemctl", "--user", "stop", PROXY_SERVICE_UNIT], timeout=60)
        cls._run(["docker", "stop", MATRIX_CONTAINER], timeout=660)
        print("✅ The Matrix homeserver is stopped; its accounts and messages are kept.")
        return 0

    @classmethod
    def status(cls) -> int:
        """Prints the homeserver's state; returns the exit code."""
        admin = cls._load("matrix-admin.json")
        config = cls._load("matrix.json")
        if not admin:
            print("Matrix: not set up (`ragon-admin matrix start`).")
            return 0
        print(f"Server name: {admin['server_name']}")
        print(
            f"Container:   {'running' if cls.container_running() else 'stopped'} ({MATRIX_CONTAINER})"
        )
        network = (
            "sidecar network, push notifications can leave"
            if admin.get("push")
            else "internal, no route out"
        )
        print(f"Network:     {network}")
        if admin["server_name"] == cls.overlay_name():
            running = cls._run(["systemctl", "--user", "is-active", TLS_SERVICE_UNIT], timeout=30)
            serving = "serving" if running.stdout.strip() == UNIT_ACTIVE else "not serving"
            front = f"Overlay:     https://{admin['server_name']}:{TLS_PORT} ({serving})"
        else:
            front = f"Tailscale:   {cls.tailscale_name() or 'not running'}"
        print(front)
        print(f"Bot:         {config.get('user_id', 'not created')}")
        print(
            f"Allowed:     {', '.join(config.get('allowed', [])) or 'nobody yet (`ragon-admin matrix add-user <name>`)'}"  # noqa: E501
        )
        return 0

    @classmethod
    def set_push(cls, on: bool) -> int:
        """Moves the container between the internal network and the sidecar network."""
        admin = cls._load("matrix-admin.json")
        if not admin:
            print("❌ No homeserver yet: run `ragon-admin matrix start` first.")
            return 1
        admin["push"] = on
        cls._save("matrix-admin.json", admin)
        cls._run(["docker", "rm", "-f", MATRIX_CONTAINER], timeout=660)
        if on:
            print(
                "⚠️  Push on: the homeserver can now reach Element's push gateway. A push carries an event id, "
                "not the message, but it does say that a message arrived, and when."
            )
        return cls.start()

    @classmethod
    def remove(cls, confirmed: bool) -> int:
        """Deletes the homeserver, its volume (every account and message) and the bridge's Matrix files."""
        if not confirmed:
            print(
                "This deletes the homeserver, every account on it and every message. Run it again with --yes."
            )
            return 1
        cls._take_down_front()
        cls._run(["systemctl", "--user", "disable", "--now", PROXY_SOCKET_UNIT], timeout=60)
        cls._run(["systemctl", "--user", "stop", PROXY_SERVICE_UNIT], timeout=60)
        cls._run(["docker", "rm", "-f", MATRIX_CONTAINER], timeout=660)
        cls._run(["docker", "volume", "rm", MATRIX_VOLUME], timeout=120)
        cls._run(["docker", "network", "rm", MATRIX_NETWORK], timeout=60)
        for name in ("matrix.json", "matrix-admin.json", "matrix-state.json"):
            with contextlib.suppress(FileNotFoundError):
                pathlib.Path(cls._path(name)).unlink()
        for name in (PROXY_SOCKET_UNIT, PROXY_SERVICE_UNIT, TLS_SERVICE_UNIT):
            with contextlib.suppress(FileNotFoundError):
                (pathlib.Path(cls.unit_dir()) / name).unlink()
        print("✅ The Matrix homeserver and its accounts are deleted.")
        return 0
