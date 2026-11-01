"""The services around the agent that are not the agent.

the desktop app, the sidecars and the Google service.

The desktop app is a window onto `ragon app-server` (`desktop_installer`, `desktop_runner`).
`searxng_sidecar` and `sidecar_network` start the search container `ragon-search` and the MCP server
share, `searxng_settings` writes its settings and `searxng_status` reports its engines;
`image_search_sidecar` and `image_search_mcp` are image search and its `image_search` tool,
`speech_sidecar` the speech-to-text behind the microphone (RAGON_ASK §6, §7); `google_service`
starts the Google service, and `google_workspace_reader` is its read-only Drive and Calendar half
(Ragon's apps). `matrix_homeserver` runs the private Matrix homeserver `ragon chat` answers on
(RAGON_CHAT §5).
"""

from dreamference.chat.desktop_installer import DesktopInstaller
from dreamference.chat.desktop_runner import DesktopRunner
from dreamference.chat.docker_bridge import DockerBridge
from dreamference.chat.gmail_client import GmailClient
from dreamference.chat.gmail_credentials import GmailCredentials
from dreamference.chat.gmail_search_service import GmailSearchService
from dreamference.chat.google_service import GoogleService
from dreamference.chat.google_workspace_reader import GoogleWorkspaceReader
from dreamference.chat.image_search_mcp import ImageSearchMcp
from dreamference.chat.image_search_sidecar import ImageSearchSidecar
from dreamference.chat.matrix_homeserver import MatrixHomeserver
from dreamference.chat.searxng_engine_event import SearxngEngineEvent
from dreamference.chat.searxng_settings import SearxngSettings
from dreamference.chat.searxng_sidecar import SearxngSidecar
from dreamference.chat.searxng_status import SearxngStatus
from dreamference.chat.sidecar_network import SidecarNetwork
from dreamference.chat.speech_sidecar import SpeechSidecar

__all__ = [
    "DesktopInstaller",
    "DesktopRunner",
    "DockerBridge",
    "GmailClient",
    "GmailCredentials",
    "GmailSearchService",
    "GoogleService",
    "GoogleWorkspaceReader",
    "ImageSearchMcp",
    "ImageSearchSidecar",
    "MatrixHomeserver",
    "SearxngEngineEvent",
    "SearxngSettings",
    "SearxngSidecar",
    "SearxngStatus",
    "SidecarNetwork",
    "SpeechSidecar",
]
