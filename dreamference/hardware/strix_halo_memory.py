"""The memory facts of an AMD Strix Halo."""

from dataclasses import dataclass


@dataclass(frozen=True)
class StrixHaloMemory:
    """What a Strix Halo's memory is, read from sysfs and /proc, in bytes.

    Attributes:
        vram_bytes (int): The firmware's VRAM carve-out (`mem_info_vram_total`); 0 when unreadable.
        gtt_bytes (int): The GTT window of system RAM the GPU can map (`mem_info_gtt_total`).
        ram_bytes (int): System RAM the kernel sees (`MemTotal`).
        cmdline_options (tuple[str, ...]): The memory-related options of /proc/cmdline.
    """

    vram_bytes: int
    gtt_bytes: int
    ram_bytes: int
    cmdline_options: tuple[str, ...]

    @property
    def usable_gib(self) -> float:
        """GiB the GPU can use: the carve-out plus the GTT window."""
        return (self.vram_bytes + self.gtt_bytes) / 2**30
