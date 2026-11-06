"""Wi-Fi power saving, which loses the multicast a node is found by.

Used by `ragon-admin host check|setup` and `ragon-admin node enable`.

A client finds its node by an mDNS browse for `_ragon-node._tcp` (docs/dev/node.md), and mDNS
is multicast. A Wi-Fi radio in power-saving mode sleeps between beacons and its access point
buffers multicast for it until the next DTIM beacon, and on the MediaTek MT7925 both GB10 machines
here use that delivery loses most of it: measured, up to ~80% of the queries sent to
a node went unanswered, so `node-b.local` did not resolve and a client could not find its
node. NetworkManager turns power saving on for every Wi-Fi connection unless the connection says
otherwise (Ubuntu ships `wifi.powersave = 3` in
`/etc/NetworkManager/conf.d/default-wifi-powersave-on.conf`, and a connection's own setting is
`default`), so a GB10 on Wi-Fi has it on out of the box.

The fix is per connection and has two halves: `nmcli connection modify <connection>
802-11-wireless.powersave 2` keeps it off across reconnects and reboots, and `iw dev <interface>
set power_save off` turns it off now, with no reconnect (a reconnect would drop an SSH session
that runs the setup). The global file is left alone: the connection's setting overrides it.

Only connected Wi-Fi devices count. A machine on Ethernet only, or without NetworkManager, has
nothing to report, and is told nothing.
"""

import os
import shutil
import subprocess
from typing import Any, Final

# The name of the step in `HostSafetySetup.steps()`, which `node prepare` also runs: the one change
# to NetworkManager it makes (`is_fix_command`).
STEP_NAME: Final[str] = "turn off Wi-Fi power saving"
# NetworkManager's `802-11-wireless.powersave` values: 0 default, 1 ignore, 2 disable, 3 enable.
# `nmcli -g` prints the keyword; older versions print the number.
DISABLED: Final[tuple] = ("disable", "2")
DISABLE_VALUE: Final[str] = "2"
# How long one nmcli or iw reading may take.
READ_SECONDS: Final[int] = 10
# nmcli's terse `device status` row: `DEVICE:TYPE:STATE`. The device and the state hold no colon,
# so three fields is the minimum a connected Wi-Fi line can have, and `wifi` is the type's own
# spelling, not a prefix of it.
STATUS_FIELDS: Final[int] = 3
WIFI_TYPE: Final[str] = "wifi"
# `is_fix_command` compares against whole argvs, not prefixes: the fix commands have exactly six
# parts, and `nmcli`/`iw` are the programs' own names, not prefixes of options.
FIX_COMMAND_PARTS: Final[int] = 6
NMCLI: Final[str] = "nmcli"
IW: Final[str] = "iw"


class WifiPowerSave:
    """Reads whether a connected Wi-Fi interface has power saving on, and builds the fix."""

    @classmethod
    def problems(cls) -> list[dict[str, Any]]:
        """Lists the connected Wi-Fi interfaces whose power saving is not off for good.

        Returns:
            List[Dict[str, Any]]: One entry per interface: `device`, `connection`, `setting` (the
            connection's `802-11-wireless.powersave` as nmcli prints it), `radio` (True when `iw`
            says power saving is on now, False when off, None when it cannot say) and `commands`
            (argv lists to run as root, without `sudo`). Empty on Ethernet only, with power saving
            already off, or without NetworkManager.
        """
        found: list[dict[str, Any]] = []
        for interface in cls.interfaces():
            setting_off = interface["setting"].split(" ", 1)[0].lower() in DISABLED
            radio = interface["radio"]
            if setting_off and radio is not True:
                continue
            commands: list[list[str]] = []
            if not setting_off:
                commands.append(
                    [
                        "nmcli",
                        "connection",
                        "modify",
                        interface["connection"],
                        "802-11-wireless.powersave",
                        DISABLE_VALUE,
                    ]
                )
            if radio is True:
                commands.append(["iw", "dev", interface["device"], "set", "power_save", "off"])
            found.append({**interface, "commands": commands})
        return found

    @classmethod
    def interfaces(cls) -> list[dict[str, Any]]:
        """Reads every connected Wi-Fi interface NetworkManager manages.

        Returns:
            List[Dict[str, Any]]: `device`, `connection`, `setting` and `radio` for each (see
            `problems`); empty when nmcli is missing, NetworkManager is not running, or no Wi-Fi
            interface is connected.
        """
        listing = cls._read(["nmcli", "-t", "-f", "DEVICE,TYPE,STATE", "device", "status"])
        if listing is None:
            return []
        found: list[dict[str, Any]] = []
        for line in listing.splitlines():
            # Device names, types and states hold no colon, so the terse fields split cleanly.
            fields = line.split(":")
            if (
                len(fields) < STATUS_FIELDS
                or fields[1] != WIFI_TYPE
                or not fields[2].startswith("connected")
            ):
                continue
            device = fields[0]
            # `-e no`: terse output would hand back a name with `:` or `\` escaped, and the
            # escaped name is not the connection's.
            connection = cls._read(
                ["nmcli", "-e", "no", "-g", "GENERAL.CONNECTION", "device", "show", device]
            )
            if not connection or not connection.strip():
                continue
            connection = connection.strip()
            setting = cls._read(
                ["nmcli", "-g", "802-11-wireless.powersave", "connection", "show", connection]
            )
            found.append(
                {
                    "device": device,
                    "connection": connection,
                    "setting": (setting or "").strip() or "unknown",
                    "radio": cls.radio_power_save(device),
                }
            )
        return found

    @classmethod
    def radio_power_save(cls, device: str) -> bool | None:
        """The power-save setting of a Wi-Fi radio.

        Args:
            device: The Wi-Fi interface.

        Returns:
            Optional[bool]: Whether the radio is in power-saving mode now (`iw dev <device> get
            power_save`), or None when iw is missing or does not say.
        """
        output = cls._read(["iw", "dev", device, "get", "power_save"])
        if output is None:
            return None
        text = output.strip().lower()
        if text.endswith(": on"):
            return True
        if text.endswith(": off"):
            return False
        return None

    @classmethod
    def step(cls) -> dict[str, Any] | None:
        """The fix as a `HostSafetySetup` step.

        Returns:
            Optional[Dict[str, Any]]: `name`, `why` and `commands`, or None when nothing is to fix.
        """
        found = cls.problems()
        if not found:
            return None
        why = (
            f"power saving is on for {cls._named(found)}, and it loses the multicast mDNS that "
            f"clients find this node by"
        )
        if any(problem["radio"] is None for problem in found):
            why += " (iw cannot turn it off now here, so the change takes effect at the next reconnect)"
        return {
            "name": STEP_NAME,
            "why": why,
            "commands": [command for problem in found for command in problem["commands"]],
        }

    @classmethod
    def warning(cls) -> str | None:
        """The warning `ragon-admin node enable` prints, with the commands that fix it.

        Returns:
            Optional[str]: The text, or None when power saving is off or there is no Wi-Fi.
        """
        found = cls.problems()
        if not found:
            return None
        import shlex

        lines = "\n".join(
            "     sudo " + " ".join(shlex.quote(part) for part in command)
            for problem in found
            for command in problem["commands"]
        )
        named = cls._named(found)
        return (
            f"⚠️  Wi-Fi power saving is on for {named}. It loses the multicast mDNS that clients find\n"
            f"   this node by (up to ~80% of queries went unanswered on a GB10), so a client may not find it.\n"
            f"   `ragon-admin host setup` turns it off, or run:\n"
            f"{lines}"
        )

    @classmethod
    def is_fix_command(cls, argv: list[str]) -> bool:
        """Whether a command is exactly one of the two this class builds, and nothing else.

        `node prepare` refuses every other nmcli command (and every other use of iw).

        Args:
            argv: The command, without `sudo`.

        Returns:
            bool: True for `nmcli connection modify <connection> 802-11-wireless.powersave 2` and
            `iw dev <interface> set power_save off`, with a name that cannot pass for an option.
        """
        parts = [str(part) for part in argv]
        if len(parts) != FIX_COMMAND_PARTS:
            return False
        if parts[0] == NMCLI:
            name, shape = parts[3], parts[:3] + parts[4:]
            expected = [NMCLI, "connection", "modify", "802-11-wireless.powersave", DISABLE_VALUE]
        elif parts[0] == IW:
            name, shape = parts[2], parts[:2] + parts[3:]
            expected = [IW, "dev", "set", "power_save", "off"]
        else:
            return False
        return shape == expected and bool(name) and not name.startswith("-")

    @classmethod
    def _named(cls, found: list[dict[str, Any]]) -> str:
        return ", ".join(f"{problem['device']} ({problem['connection']})" for problem in found)

    @classmethod
    def _read(cls, argv: list[str]) -> str | None:
        """Runs one reading command; the one place this class runs anything.

        Args:
            argv: The command, which changes nothing.

        Returns:
            Optional[str]: Its output, or None when the program is missing, fails or times out.
        """
        # iw is in /usr/sbin, which not every user's PATH has.
        program = shutil.which(argv[0]) or shutil.which(argv[0], path="/usr/sbin:/sbin")
        if program is None:
            return None
        try:
            result = subprocess.run(
                [program, *argv[1:]],
                capture_output=True,
                text=True,
                timeout=READ_SECONDS,
                stdin=subprocess.DEVNULL,
                check=False,
                env={**os.environ, "LC_ALL": "C"},
            )
        except (OSError, subprocess.SubprocessError):
            return None
        if result.returncode != 0:
            return None
        return result.stdout
