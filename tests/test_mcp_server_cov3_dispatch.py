"""Branch tests for the MCP server's tool dispatch and stdio loop: every tool body is a stand-in."""

import asyncio
import io
import json
import os
import sys
from unittest.mock import MagicMock

import pytest

from dreamference.mcp_server import MCPServer
from dreamference.mcp_server import mcp_server as mcp_module
from dreamference.mcp_server.web_tools import WebTools


@pytest.fixture
def ide(monkeypatch):
    """A fresh IDE state in place of the module's global."""
    state = MagicMock()
    state.active_file = None
    state.cursor_line = 3
    state.cursor_column = 4
    state.selection_text = "sel"
    state.diagnostics = [{"file": "a.py", "m": 1}, {"file": "b.py", "m": 2}]
    state.open_files = ["a.py", "b.py"]
    monkeypatch.setattr(mcp_module, "global_ide_state", state)
    return state


def test_active_editor_without_a_file_says_so(ide):
    outcome = MCPServer().execute_tool("ide_get_active_editor", {})
    assert outcome["active_file"] == "No active file"
    assert (outcome["line"], outcome["column"], outcome["selection"]) == (3, 4, "sel")


def test_diagnostics_are_filtered_by_file_only_when_one_is_named(ide):
    server = MCPServer()
    assert server.execute_tool("ide_get_diagnostics", {"file_path": "b.py"}) == [
        {"file": "b.py", "m": 2}
    ]
    assert len(server.execute_tool("ide_get_diagnostics", {})) == 2


def test_open_files_and_open_file_report_the_state(ide):
    server = MCPServer()
    assert server.execute_tool("ide_get_open_files", {}) == {"open_files": ["a.py", "b.py"]}
    outcome = server.execute_tool("ide_open_file", {"file_path": "x.py", "line": 7})
    ide.set_active_editor.assert_called_once_with("x.py", 7, 1)
    assert outcome["status"] == "success"


def test_apply_diff_never_claims_an_edit_landed():
    outcome = MCPServer().execute_tool("ide_apply_diff", {"file_path": "x.py"})
    assert outcome["status"] == "not_applied" and "x.py is unchanged" in outcome["message"]


def test_unknown_tool_is_an_error_not_a_crash():
    assert "not implemented" in MCPServer().execute_tool("nope", {})["error"]


def test_web_tools_get_their_arguments_mapped(monkeypatch):
    search, fetch = MagicMock(return_value={"r": 1}), MagicMock(return_value={"f": 1})
    monkeypatch.setattr(WebTools, "search", search)
    monkeypatch.setattr(WebTools, "fetch", fetch)
    server = MCPServer()
    assert server.execute_tool("web_search", {"query": "q", "category": "it"}) == {"r": 1}
    search.assert_called_once_with("q", 8, "it", "auto", "")
    search.reset_mock()
    server.execute_tool("web_search", {"query": "q", "time_range": "day", "max_results": 2})
    search.assert_called_once_with("q", 2, "general", "auto", "day")
    server.execute_tool("web_fetch", {"url": "https://example.org"})
    fetch.assert_called_once_with("https://example.org", 20000)


def test_code_search_falls_back_to_the_context_engine_when_not_indexed(monkeypatch):
    from dreamference.mcp_server.code_index_search import CodeIndexSearch

    monkeypatch.setattr(CodeIndexSearch, "search", classmethod(lambda c, q, top_k: None))
    server = MCPServer()
    server._context_engine = MagicMock()
    server._context_engine.search_code.return_value = [{"hit": 1}]
    assert server.execute_tool("workspace_search_code", {"query": "q", "top_k": 2}) == [{"hit": 1}]
    server._context_engine.index_workspace.assert_called_once()
    server._context_engine.search_code.assert_called_once_with("q", top_k=2)


def test_code_search_uses_the_index_and_skips_the_engine(monkeypatch):
    from dreamference.mcp_server.code_index_search import CodeIndexSearch

    monkeypatch.setattr(CodeIndexSearch, "search", classmethod(lambda c, q, top_k: [{"i": 1}]))
    server = MCPServer()
    server._context_engine = MagicMock()
    assert server.execute_tool("workspace_search_code", {"query": "q"}) == [{"i": 1}]
    server._context_engine.index_workspace.assert_not_called()


def test_context_engine_is_built_once_on_first_use(monkeypatch):
    built = []

    class Engine:
        def __init__(self):
            built.append(self)

    import dreamference.context_engine as package

    monkeypatch.setattr(package, "ContextEngine", Engine)
    server = MCPServer()
    assert built == []
    assert server.context_engine is server.context_engine
    assert len(built) == 1


def test_ping_and_unknown_method_answer_by_json_rpc():
    server = MCPServer()
    assert server.handle_request({"id": 1, "method": "ping"})["result"] == {}
    error = server.handle_request({"id": 2, "method": "bogus"})["error"]
    assert error["code"] == -32601 and "bogus" in error["message"]


def test_write_puts_one_line_on_stdout(monkeypatch):
    out = io.StringIO()
    monkeypatch.setattr(sys, "stdout", out)
    MCPServer._write({"a": 1})
    assert out.getvalue() == '{"a": 1}\n'


def _drive_stdio(monkeypatch, payload: bytes) -> list[dict]:
    """Runs the real loop over a pipe holding `payload`; returns what it wrote."""
    read_end, write_end = os.pipe()
    os.write(write_end, payload)
    os.close(write_end)
    monkeypatch.setattr(sys, "stdin", os.fdopen(read_end, "rb", buffering=0))
    written: list[dict] = []
    monkeypatch.setattr(MCPServer, "_write", staticmethod(written.append))
    asyncio.run(MCPServer().run_stdio_async())
    return written


def test_stdio_loop_answers_requests_and_never_notifications(monkeypatch):
    lines = [
        {"jsonrpc": "2.0", "id": 1, "method": "ping"},
        {"jsonrpc": "2.0", "method": "notifications/initialized"},
    ]
    payload = "\n".join(json.dumps(line) for line in lines).encode() + b"\n"
    assert _drive_stdio(monkeypatch, payload) == [{"jsonrpc": "2.0", "id": 1, "result": {}}]


def test_stdio_loop_reports_bad_lines_and_goes_on(monkeypatch):
    payload = b"\n   \nnot json\n\xff\xfe\n" + b'{"jsonrpc":"2.0","id":9,"method":"ping"}\n'
    written = _drive_stdio(monkeypatch, payload)
    assert [w["error"]["code"] for w in written[:2]] == [-32700, -32700]
    assert written[2]["id"] == 9


def test_stdio_loop_reports_an_overlong_line_and_goes_on(monkeypatch):
    monkeypatch.setattr(mcp_module, "LINE_LIMIT", 64)
    payload = b"x" * 500 + b"\n" + b'{"jsonrpc":"2.0","id":5,"method":"ping"}\n'
    written = _drive_stdio(monkeypatch, payload)
    assert written[0]["error"]["message"].startswith("Parse error: line too long")
    assert written[-1]["id"] == 5


def test_stdio_loop_carries_the_id_of_a_failing_request(monkeypatch):
    async def boom(self, request):
        raise RuntimeError("tool broke")

    monkeypatch.setattr(MCPServer, "handle_request_async", boom)
    written = _drive_stdio(monkeypatch, b'{"jsonrpc":"2.0","id":4,"method":"ping"}\n')
    assert written[0]["id"] == 4 and written[0]["error"]["code"] == -32603
    assert "tool broke" in written[0]["error"]["message"]


def test_main_runs_the_stdio_loop(monkeypatch):
    run = MagicMock()
    monkeypatch.setattr(MCPServer, "run_stdio", run)
    mcp_module.main()
    run.assert_called_once()
