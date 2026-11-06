"""Ports and default endpoints shared across Dreamference.

One home for the numbers the CLI, the model server's manager, the model gate, the node and the
forge all spell: the model server's public port, the loopback port the engine sits on behind the
model gate, the port `ragon web` is served on and SearXNG's port. This module imports nothing, so
any module may import it.
"""

from typing import Final

DEFAULT_MODEL_PORT: Final[int] = 8000
# The engine's port is the public one plus this, on loopback only (8000 -> 18000).
ENGINE_PORT_OFFSET: Final[int] = 10000
ENGINE_PORT: Final[int] = DEFAULT_MODEL_PORT + ENGINE_PORT_OFFSET
WEB_PORT: Final[int] = 3100
SEARXNG_PORT: Final[int] = 8888
DEFAULT_VLLM_HOST: Final[str] = f"http://localhost:{DEFAULT_MODEL_PORT}"
