"""Read a machine's state before provisioning changes it.

This is specs/DREAMFERENCE_RAGON_FLEET.md §7.1, steps 1 and 2.

Ragon may not be installed there yet, so the probe is a plain POSIX shell script sent over the
session. It changes nothing and prints `key=value` lines, which `parse` reads. Every step of a
provisioning run decides from these readings whether it has anything to do, which is what makes a
second run on a finished node change nothing.
"""

import shlex
from typing import Final

_FLAG_ON: Final[str] = "1"
_SPACE: Final[str] = " "
_GB10: Final[str] = "GB10"


# The script's fixed part. Model folders and images are appended per run by `script`.
BASE_SCRIPT: Final[str] = r"""
v() { printf '%s=%s\n' "$1" "$2"; }
[ -f /etc/dgx-release ] && v dgx 1 || v dgx 0
v gpu "$(nvidia-smi --query-gpu=name --format=csv,noheader 2>/dev/null | head -n 1)"
v hostname "$(hostname)"
v bundle "$(cat "$HOME/.local/share/dreamference/ragon/VERSION" 2>/dev/null)"
[ -x "$HOME/.local/share/dreamference/venv/bin/ragon-admin" ] && v admin 1 || v admin 0
# DGX OS ships python3 without venv (python3-venv), which `install.sh` needs and, with no terminal
# for sudo, cannot install itself (measured on a fresh GX10).
if [ -x "$HOME/.local/share/dreamference/venv/bin/python" ] || ! command -v dpkg-query >/dev/null 2>&1 \
  || dpkg-query -W -f='${Status}' python3-venv 2>/dev/null | grep -q 'ok installed'; then v venv 1
else v venv 0; fi
v node_id "$(cat "$HOME/.config/dreamference/node-id" 2>/dev/null)"
id -nG 2>/dev/null | tr ' ' '\n' | grep -qx docker && v docker_group 1 || v docker_group 0
[ -f "/var/lib/systemd/linger/$(id -un)" ] && v linger 1 || v linger 0
f=/etc/avahi/services/ragon-node.service
if [ -f "$f" ] && [ "$(stat -c %U "$f")" = "$(id -un)" ]; then v avahi_file 1
else v avahi_file 0; fi
[ -s "$f" ] && v advertised 1 || v advertised 0
[ -f /etc/apparmor.d/ragon-bwrap ] && v sandbox_profile 1 || v sandbox_profile 0
v userns_restricted "$(cat /proc/sys/kernel/apparmor_restrict_unprivileged_userns 2>/dev/null)"
v telemetry "$(systemctl is-enabled nvidia-dgx-telemetry 2>/dev/null)"
v free_gb "$(df -Pk "$HOME" 2>/dev/null | awk 'NR==2 {printf "%d", $4/1048576}')"
for c in "$HOME/dreamference.toml" "$HOME/.config/dreamference/config.toml"; do
  m="$(sed -n 's/^model *= *"\(.*\)".*/\1/p' "$c" 2>/dev/null | head -n 1)"
  [ -n "$m" ] && break
done
v model "$m"
r="$(docker inspect -f '{{.State.Running}}' dreamference-searxng 2>/dev/null \
  || sg docker -c "docker inspect -f '{{.State.Running}}' dreamference-searxng" 2>/dev/null)"
v searxng "$r"
hub="${HF_HUB_CACHE:-${HUGGINGFACE_HUB_CACHE:-${HF_HOME:-$HOME/.cache/huggingface}/hub}}"
v hub "$hub"
"""


class FleetProbe:
    """The read-only state probe and its parser."""

    @classmethod
    def script(cls, folders: list[str], images: list[str]) -> str:
        """Args: folders: Hub-cache folder names to look for (`models--org--name`). images.

        Image references to look for.

        Returns:
            str: The shell script to run on the machine.
        """
        lines = [BASE_SCRIPT.strip()]
        for folder in folders:
            quoted = shlex.quote(folder)
            lines.append(
                f'[ -d "$hub"/{quoted}/snapshots ] && v model:{quoted} 1 || v model:{quoted} 0'
            )
        for image in images:
            quoted = shlex.quote(image)
            check = shlex.quote(f"docker image inspect {quoted} >/dev/null 2>&1")
            lines.append(
                f"( docker image inspect {quoted} >/dev/null 2>&1 || sg docker -c {check} ) "
                f"&& v image:{quoted} 1 || v image:{quoted} 0"
            )
        return "\n".join(lines) + "\n"

    @classmethod
    def parse(cls, output: str) -> dict[str, str]:
        """Args: output: What the script printed.

        Returns:
            Dict[str, str]: Each key's value; keys the script did not print are absent.
        """
        state: dict[str, str] = {}
        for line in output.splitlines():
            key, sep, value = line.partition("=")
            if sep and key and _SPACE not in key:
                state[key] = value.strip()
        return state

    @classmethod
    def is_gb10(cls, state: dict[str, str]) -> bool:
        """Args: state: A parsed probe.

        Returns:
            bool: True if the machine is a DGX Spark-class GB10 (`/etc/dgx-release`, or a GPU named
            GB10).
        """
        return state.get("dgx") == _FLAG_ON or _GB10 in state.get("gpu", "")

    @classmethod
    def missing_models(cls, state: dict[str, str], folders: list[str]) -> list[str]:
        """Args: state: A parsed probe. folders: The folders the node needs.

        Returns:
            List[str]: Those it lacks.
        """
        return [folder for folder in folders if state.get(f"model:{folder}") != _FLAG_ON]

    @classmethod
    def missing_images(cls, state: dict[str, str], images: list[str]) -> list[str]:
        """Args: state: A parsed probe. images: The images the node needs.

        Returns:
            List[str]: Those it lacks.
        """
        return [image for image in images if state.get(f"image:{image}") != _FLAG_ON]

    @classmethod
    def prepared(cls, state: dict[str, str]) -> bool:
        """Args: state: A parsed probe.

        Returns:
            bool: True if every root step `node prepare` does, apart from the host settings that
            `host check` reports, is already in place.
        """
        return (
            state.get("docker_group") == _FLAG_ON
            and state.get("linger") == _FLAG_ON
            and state.get("avahi_file") == _FLAG_ON
            and (
                state.get("sandbox_profile") == _FLAG_ON
                or state.get("userns_restricted") != _FLAG_ON
            )
            and state.get("telemetry", "") not in ("enabled", "static", "enabled-runtime")
        )
