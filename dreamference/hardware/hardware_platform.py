"""The machine platforms a Ragon node runs on."""

from enum import Enum


class HardwarePlatform(Enum):
    """A node platform, found from the PCI bus and the compute driver's topology, never a vendor string."""

    GB10 = "gb10"
    STRIX_HALO_LINUX = "strix-halo-linux"
