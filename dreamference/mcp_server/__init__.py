"""The stdio MCP server for editors and its helpers."""

from dreamference.mcp_server.code_index_search import CodeIndexSearch
from dreamference.mcp_server.editor_selection import EditorSelection
from dreamference.mcp_server.ide_state import IDEState
from dreamference.mcp_server.mcp_server import MCPServer, global_ide_state, main
from dreamference.mcp_server.mcp_tool_registry import MCPToolRegistry

__all__ = [
    "CodeIndexSearch",
    "EditorSelection",
    "IDEState",
    "MCPServer",
    "MCPToolRegistry",
    "global_ide_state",
    "main",
]
