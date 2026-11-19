"""One row of the device table (AUTH §5.5)."""

from dataclasses import dataclass, field


@dataclass
class DeviceEntry:
    """One row of the table of codes."""

    device_id: str
    device_code: str
    user_code: str
    client_id: str
    shape: str
    expires_at: float
    user: str | None = None
    denied: bool = False
    last_poll: float = 0.0
    auth_code: str | None = None
    challenge: str = ""
    verifier: str = ""
    machine: str = field(default="")
