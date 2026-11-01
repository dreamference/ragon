"""Branch tests: files the context engine must not read in, and a code index that cannot answer."""

import subprocess
from unittest.mock import MagicMock

from dreamference.context_engine import ContextEngine
from dreamference.context_engine import context_engine as module
from dreamference.mcp_server import CodeIndexSearch


def _engine(tmp_path) -> ContextEngine:
    return ContextEngine(workspace_root=str(tmp_path))


def test_a_file_over_the_size_ceiling_is_never_read(tmp_path, monkeypatch):
    big = tmp_path / "big.py"
    big.write_text("x = 1\n" * 10)
    monkeypatch.setattr(module, "MAX_FILE_BYTES", 5)
    engine = _engine(tmp_path)
    engine.tokenize = MagicMock()
    assert engine._process_single_file(big) is None
    engine.tokenize.assert_not_called()


def test_a_binary_file_is_skipped_by_its_first_bytes(tmp_path):
    blob = tmp_path / "data.py"
    blob.write_bytes(b"abc\0def")
    assert _engine(tmp_path)._process_single_file(blob) is None


def test_a_text_file_is_indexed_with_its_symbols(tmp_path):
    source = tmp_path / "mod.py"
    source.write_text("def alpha():\n    return 1\n")
    indexed, content = _engine(tmp_path)._process_single_file(source)
    assert indexed.rel_path == "mod.py" and "alpha" in content


def test_a_failing_code_index_call_falls_back_instead_of_raising(monkeypatch):
    monkeypatch.setattr(CodeIndexSearch, "executable", classmethod(lambda cls: "/bin/ragon-code"))
    for error in (OSError("gone"), subprocess.TimeoutExpired("ragon-code", 1)):
        monkeypatch.setattr(subprocess, "run", MagicMock(side_effect=error))
        assert CodeIndexSearch.search("alpha beta") is None


def test_a_blank_query_or_a_missing_binary_means_the_index_cannot_answer(monkeypatch):
    monkeypatch.setattr(CodeIndexSearch, "executable", classmethod(lambda cls: "/bin/ragon-code"))
    run = MagicMock()
    monkeypatch.setattr(subprocess, "run", run)
    assert CodeIndexSearch.search("   ") is None
    monkeypatch.setattr(CodeIndexSearch, "executable", classmethod(lambda cls: None))
    assert CodeIndexSearch.search("alpha") is None
    run.assert_not_called()
