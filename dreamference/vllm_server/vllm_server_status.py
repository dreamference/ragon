"""vLLM Server Telemetry Status Dataclass for Dreamference.

This module defines the VLLMServerStatus dataclass used to report health, URL endpoints,
active models, and process IDs for local vLLM instances.
"""

from dataclasses import dataclass


@dataclass
class VLLMServerStatus:
    """Telemetry status report object for local vLLM server instance.

    Attributes:
        host (str): HTTP endpoint URL (e.g. 'http://localhost:8000').
        healthy (bool): True if /v1/models endpoint responds with HTTP 200 OK.
        models (List[str]): List of currently served model identifiers.
        pid (Optional[int]): Operating system process ID of the launched server process.
    """

    host: str
    healthy: bool
    models: list[str]
    pid: int | None
    loading_status: str | None = None
