"""Web access tools for Ragon's MCP server.

`web_search` and `web_fetch` run the same programs `ragon` runs, `ragon-search` and `ragon-fetch`
(`ragon-web-rs/`), with `--json`, and hand back what they print. There is one implementation of the
ranking (duplicates merged, documentation first, no host filling the list), of the page reading
(headings, code blocks and tables kept, menus dropped, a page drawn by scripts said so), of the
`source` line to cite, and of the failure wording, so an IDE reaching the web through MCP gets what
the terminal agent gets. An earlier copy of all that in Python had fallen behind.

When a program cannot be found or cannot run, the tool says so and what installs it; it does not
fall back to a second implementation that would answer differently. Both programs follow the air-gap
level themselves; the check made here as well only spares the process when nothing may be sent.
"""

import json
import shutil
import subprocess
from pathlib import Path
from typing import Any, Final

# The most a call may take. The programs bound each request themselves; this bounds the process.
PROCESS_TIMEOUT_S: Final[float] = 150.0
# Where `ragon-admin codex build` puts them when PATH does not name them.
BINARY_DIRS: Final[tuple[str, ...]] = ("~/.local/bin", "~/.local/share/dreamference/ragon/bin")
MISSING_BINARY_HINT: Final[str] = (
    "Install it with `ragon-admin codex build` (it builds ragon, ragon-search and ragon-fetch)."
)
# Returned, with nothing sent, when the configured air-gap level is `on`. The web commands' text
# names `/airgapped`, a slash command an IDE's MCP client does not have, so this names the setting.
AIRGAPPED_ON_MESSAGE: Final[str] = (
    "Web access is off on this machine (ragon_airgapped = on). Only the user can change that, "
    "with `ragon airgapped default off` or DREAMFERENCE_RAGON_AIRGAPPED."
)
AIRGAPPED_ON: Final[str] = "on"
AUTO_LANGUAGE: Final[str] = "auto"
FINAL_URL: Final[str] = "final_url"
DEFAULT_MAX_CHARS: Final[int] = 20_000
MAX_CHARS_CEILING: Final[int] = 100_000


class WebTools:
    """Fetch and search primitives exposed to the agent over MCP, by way of the web programs.

    A classmethod namespace, like the other stateless helpers in this codebase.
    """

    @classmethod
    def airgapped_level(cls) -> str:
        """The air-gap level in force for this server.

        There is no session here, so it is the configured one (environment, then the stricter of the
        two configuration files), as the one reader resolves it, and `on` whenever `ragon` cannot
        answer.

        Returns:
            str: `off` or `on`.
        """
        from dreamference.config import DreamferenceConfig

        return DreamferenceConfig.resolve_airgapped_level()

    @classmethod
    def _binary(cls, name: str) -> str | None:
        """Finds one of the web programs.

        Args:
            name (str): `ragon-search` or `ragon-fetch`.

        Returns:
            str | None: Its path, or None when it is installed nowhere known.
        """
        found = shutil.which(name)
        if found:
            return found
        for directory in BINARY_DIRS:
            candidate = Path(directory).expanduser() / name
            if candidate.is_file():
                return str(candidate)
        return None

    @classmethod
    def _run(cls, name: str, arguments: list[str], echo: dict[str, str]) -> dict[str, Any]:
        """Runs a web program with `--json` and returns what it reported.

        Args:
            name (str): `ragon-search` or `ragon-fetch`.
            arguments (list[str]): Its arguments after `--json`.
            echo (dict[str, str]): Fields every failure answer carries (the query or the URL).

        Returns:
            dict[str, Any]: The program's JSON object, or {**echo, 'error': ...} when it is
                missing, timed out, or printed nothing readable. A program's own error object
                (exit 1) is returned as it printed it.
        """
        binary = cls._binary(name)
        if binary is None:
            return {**echo, "error": f"{name} is not installed. {MISSING_BINARY_HINT}"}
        try:
            done = subprocess.run(
                [binary, "--json", *arguments],
                capture_output=True,
                text=True,
                timeout=PROCESS_TIMEOUT_S,
                stdin=subprocess.DEVNULL,
                check=False,
            )
        except subprocess.TimeoutExpired:
            return {**echo, "error": f"{name} took longer than {PROCESS_TIMEOUT_S:.0f} s"}
        except OSError as error:
            return {**echo, "error": f"{name} could not run: {error}. {MISSING_BINARY_HINT}"}
        try:
            reported = json.loads(done.stdout)
        except ValueError:
            reported = None
        if isinstance(reported, dict):
            return reported
        detail = (done.stderr or done.stdout).strip()[:300]
        return {**echo, "error": f"{name} failed (exit {done.returncode}): {detail}"}

    @classmethod
    def fetch(cls, url: str, max_chars: int = DEFAULT_MAX_CHARS) -> dict[str, Any]:
        """Retrieves a web page and returns its readable text, as `ragon-fetch --json` does.

        Args:
            url (str): Absolute http(s) URL.
            max_chars (int): Maximum characters of text to return; clamped to MAX_CHARS_CEILING.

        Returns:
            dict[str, Any]: What `ragon-fetch --json` prints (url, final_url, source, status,
                content_type, title, text, truncated), or {'error': ...} on failure. At air-gap
                level `on` the error is AIRGAPPED_ON_MESSAGE and nothing is run.
        """
        if cls.airgapped_level() == AIRGAPPED_ON:
            return {"url": url, "airgapped": "on", "error": AIRGAPPED_ON_MESSAGE}
        limit = max(1, min(int(max_chars), MAX_CHARS_CEILING))
        page = cls._run("ragon-fetch", ["--max-chars", str(limit), "--", url], {"url": url})
        if FINAL_URL in page:
            # The line to cite, the same address `ragon-fetch` prints as `Source:`.
            page.setdefault("source", page[FINAL_URL])
        return page

    @classmethod
    def search(
        cls,
        query: str,
        max_results: int = 8,
        categories: str = "general",
        language: str = "auto",
        time_range: str = "",
    ) -> dict[str, Any]:
        """Searches the web as `ragon-search --json` does.

        Args:
            query (str): Search terms.
            max_results (int): Maximum results to return.
            categories (str): A category, e.g. 'general', 'it', 'news', 'science', 'social'.
            language (str): Result language code, or 'auto' to detect it from the query.
            time_range (str): 'day', 'week', 'month' or 'year' for recent results only; '' for any.

        Returns:
            dict[str, Any]: What `ragon-search --json` prints (query, result_count, answers,
                results), or {'error': ...} naming the endpoint and how to start it. At air-gap
                level `on` the error is AIRGAPPED_ON_MESSAGE and nothing is run.
        """
        if cls.airgapped_level() == AIRGAPPED_ON:
            return {"query": query, "airgapped": "on", "error": AIRGAPPED_ON_MESSAGE}
        if not query.strip():
            return {"query": query, "error": "empty query"}
        arguments = ["-n", str(max(1, int(max_results))), "-c", categories or "general"]
        if language and language != AUTO_LANGUAGE:
            arguments += ["-l", language]
        if time_range:
            arguments += ["-t", time_range]
        return cls._run("ragon-search", [*arguments, "--", query], {"query": query})
