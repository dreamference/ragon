"""Code search through `ragon-code`, the code index router.

This module provides the CodeIndexSearch class, which answers the MCP server's
`workspace_search_code` from the router's index (specs/DREAMFERENCE_RAGON_CODE_INDEX.md §8)
instead of building the `dreamference` context engine. The router reads two SQLite files and
exits in milliseconds; the context engine walks the workspace and loads an embedding model on
first use. The engine remains the fallback where `ragon-code` is not installed or the workspace
has no index yet.
"""

import json
import os
import pathlib
import subprocess
from typing import Any, Final

from dreamference.runner.codex_branded_builder import INSTALL_DIR

# `ragon-code` exits with this when the workspace has no index (and prints what to run).
NO_INDEX_EXIT: Final[int] = 3

SEARCH_TIMEOUT_S: Final[int] = 20


class CodeIndexSearch:
    """Runs `ragon-code search` and shapes its rows for MCP clients."""

    @classmethod
    def executable(cls) -> str | None:
        """Locates the installed `ragon-code`.

        Only Ragon's own install directory counts, as for `ragon`: a `ragon-code` found on
        PATH could be another build, whose answers this module would pass on unchecked.

        Returns:
            Optional[str]: Absolute path to the binary, or None if it has not been built.
        """
        path = str(pathlib.Path(INSTALL_DIR) / "bin" / "ragon-code")
        return path if pathlib.Path(path).is_file() and os.access(path, os.X_OK) else None

    @classmethod
    def search(
        cls, query: str, top_k: int = 5, cwd: str | None = None
    ) -> list[dict[str, Any]] | None:
        """Searches the workspace's code index for definitions matching the words.

        Args:
            query (str): Words to match against names, qualified names and bodies.
            top_k (int): Number of results to return.
            cwd (Optional[str]): The workspace; defaults to the current directory.

        Returns:
            Optional[List[Dict[str, Any]]]: One record per definition (`rel_path`, `line`, `kind`,
            `symbol`, `source`), best first; None when the router cannot answer here (not
            installed, no index for this workspace, or a failed call), so the caller falls back.
        """
        binary = cls.executable()
        if binary is None or not query.strip():
            return None
        try:
            result = subprocess.run(
                # `--` before the words: a word such as `--path` is a word, not an option. The
                # router answers 40 rows unless told otherwise, and at most 200.
                [
                    binary,
                    "search",
                    "--json",
                    "--limit",
                    str(min(max(top_k, 1), 200)),
                    "--",
                    *query.split(),
                ],
                cwd=cwd or str(pathlib.Path.cwd()),
                capture_output=True,
                text=True,
                stdin=subprocess.DEVNULL,
                timeout=SEARCH_TIMEOUT_S,
            )
        except (OSError, subprocess.SubprocessError):
            return None
        if result.returncode != 0:
            # NO_INDEX_EXIT, or an error: either way the engine answers instead.
            return None
        try:
            answer = json.loads(result.stdout)
        except ValueError:
            return None
        records: list[dict[str, Any]] = []
        for row in answer.get("rows", [])[: max(0, top_k)]:
            kind, _, symbol = str(row.get("detail", "")).partition(" ")
            records.append(
                {
                    "rel_path": row.get("path"),
                    "line": row.get("line"),
                    "kind": kind,
                    "symbol": symbol,
                    "source": "ragon-code",
                }
            )
        return records
