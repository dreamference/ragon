import os
import shutil
import subprocess
from typing import Final
from urllib.parse import urlparse

from dreamference.config.ports import DEFAULT_MODEL_PORT

RAM_HEALTHY_GB: Final[float] = 5.0
RAM_UNHEALTHY_GB: Final[float] = 2.0
IOWAIT_HEALTHY_PCT: Final[float] = 10.0
IOWAIT_UNHEALTHY_PCT: Final[float] = 25.0
PROC_STAT_MIN_FIELDS: Final[int] = 6
STATS_MIN_SEPARATORS: Final[int] = 2


class ContainerDiagnostics:
    """Gathers detailed diagnostics including Docker container memory, CPU, block I/O.

    As well as system-wide I/O wait, swap metrics, and available RAM.
    """

    def __init__(self, host: str):
        self.host = host
        self._last_iowait_ticks: int = 0
        self._last_total_ticks: int = 0

    @staticmethod
    def _swap_info(container_name: str) -> str:
        """Read the container's swap and major-fault counters, or an empty string."""
        try:
            cg_result = subprocess.run(
                ["docker", "exec", container_name, "cat", "/sys/fs/cgroup/memory.stat"],
                capture_output=True,
                text=True,
                timeout=1,
            )
            if cg_result.returncode != 0:
                return ""
            counters = {"pgmajfault": "?", "pswpin": "?", "pswpout": "?"}
            for line in cg_result.stdout.splitlines():
                for key in counters:
                    if line.startswith(key):
                        counters[key] = line.split()[1]
                        break
            return f" | SwapIO: {counters['pswpin']}in {counters['pswpout']}out | MajFaults: {counters['pgmajfault']}"
        except Exception:
            return ""

    @staticmethod
    def _host_ram_gb() -> float:
        """Return MemAvailable in GB, or -1.0 when it cannot be read."""
        try:
            with open("/proc/meminfo") as f:
                for line in f:
                    if line.startswith("MemAvailable:"):
                        return int(line.split()[1]) / (1024 * 1024)
        except Exception:
            pass
        return -1.0

    @staticmethod
    def _ram_status(host_ram_gb: float) -> str:
        """Classify the available RAM."""
        if host_ram_gb > RAM_HEALTHY_GB:
            return "Healthy"
        if host_ram_gb > RAM_UNHEALTHY_GB:
            return "Unhealthy"
        if host_ram_gb >= 0:
            return "Dangerous"
        return "Unknown"

    def _iowait_pct(self) -> float:
        """Return the I/O wait share since the last call, or -1.0 when unknown."""
        iowait_pct = -1.0
        try:
            with open("/proc/stat") as f:
                cpu_line = f.readline()
            parts = cpu_line.split()
            if len(parts) >= PROC_STAT_MIN_FIELDS:
                total_ticks = sum(int(p) for p in parts[1:])
                iowait_ticks = int(parts[5])
                if self._last_total_ticks > 0:
                    dt = total_ticks - self._last_total_ticks
                    di = iowait_ticks - self._last_iowait_ticks
                    if dt > 0:
                        iowait_pct = (di / dt) * 100.0
                self._last_total_ticks = total_ticks
                self._last_iowait_ticks = iowait_ticks
        except Exception:
            pass
        return iowait_pct

    @staticmethod
    def _io_status(iowait_pct: float) -> str:
        """Classify the I/O wait share."""
        if iowait_pct < 0:
            return "Unknown"
        if iowait_pct < IOWAIT_HEALTHY_PCT:
            return "Healthy"
        if iowait_pct < IOWAIT_UNHEALTHY_PCT:
            return "Unhealthy"
        return "Dangerous"

    @staticmethod
    def _format_stats(stats: str, ram_str: str, io_str: str, swap_info: str) -> str:
        """Build the diagnostics line from `docker stats` output."""
        if stats.count("|") < STATS_MIN_SEPARATORS:
            return stats + swap_info
        mem, cpu, blkio = stats.split("|", 2)

        total_cpus = os.cpu_count() or 1
        cpus_limit = max(1.0, total_cpus * 0.7)
        cpu_limit_pct = cpus_limit * 100.0

        try:
            cpu_val = float(cpu.replace("%", "").strip())
            status = "Healthy" if cpu_val <= cpu_limit_pct + 0.1 else "Dangerous"
        except ValueError:
            status = "Unknown"

        return (
            f"{mem.strip()} | CPU: {cpu.strip()} / {cpu_limit_pct:.1f}% "
            f"({status}) | BlkIO: {blkio.strip()} | Host RAM Free: {ram_str} | IOWait: {io_str}{swap_info}"
        )

    def get_diagnostics_line(self) -> str | None:
        if not shutil.which("docker"):
            return None
        try:
            # Extract port from host for container name
            port = urlparse(self.host).port or DEFAULT_MODEL_PORT
            container_name = f"dreamference-vllm-{port}"
            result = subprocess.run(
                [
                    "docker",
                    "stats",
                    "--no-stream",
                    "--format",
                    "{{.MemUsage}}|{{.CPUPerc}}|{{.BlockIO}}",
                    container_name,
                ],
                capture_output=True,
                text=True,
                timeout=2,
            )
            swap_info = self._swap_info(container_name)

            host_ram_gb = self._host_ram_gb()
            ram_status = self._ram_status(host_ram_gb)
            ram_str = f"{host_ram_gb:.1f}GB ({ram_status})" if host_ram_gb >= 0 else "Unknown"

            iowait_pct = self._iowait_pct()
            io_status = self._io_status(iowait_pct)
            io_str = f"{iowait_pct:.1f}% ({io_status})" if iowait_pct >= 0 else "Unknown"

            if result.returncode == 0:
                return self._format_stats(result.stdout.strip(), ram_str, io_str, swap_info)
        except Exception:
            pass
        return None
