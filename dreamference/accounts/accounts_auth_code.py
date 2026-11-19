"""An authorization code waiting for its one exchange (AUTH §5.4)."""

from dataclasses import dataclass


@dataclass
class AuthCode:
    """An authorization code waiting to be exchanged once."""

    user: str
    client_id: str
    redirect_uri: str
    challenge: str
    expires_at: float
    machine: str = ""
