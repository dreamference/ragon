"""Ragon by Dreamference - Autonomous local agentic coding engine for NVIDIA GB10."""

import re
from importlib import metadata

# The version pip installed, in the release's spelling (`0.9.9-rc.1`, the tag and the `ragon`
# binary's RAGON_VERSION), not PEP 440's (`0.9.9rc1`): the node advertises it and clients compare
# it with their own. The release workflow stamps only setup.py, so a literal here went stale (0.9.9 rc.1
# nodes said 0.9.0). Source checkouts without an install fall back to setup.py's version.
_SOURCE_VERSION = "0.9.0"

_PEP440 = re.compile(
    r"^(?P<base>\d+(?:\.\d+)*)(?:(?P<pre>a|b|rc)(?P<pre_n>\d+))?(?:\.dev(?P<dev_n>\d+))?$"
)
_PRE_WORD = {"a": "alpha", "b": "beta", "rc": "rc"}


def release_spelling(installed: str) -> str:
    """`0.9.9rc1` -> `0.9.9-rc.1`, `0.9.9.dev42` -> `0.9.9-dev.42`, `0.9.9` unchanged.

    Anything else (post-releases, local versions, pre plus dev) comes back as given.
    """
    match = _PEP440.match(installed)
    if match is None or (match["pre"] and match["dev_n"]):
        return installed
    if match["pre"]:
        return f"{match['base']}-{_PRE_WORD[match['pre']]}.{match['pre_n']}"
    if match["dev_n"]:
        return f"{match['base']}-dev.{match['dev_n']}"
    return match["base"]


def _installed_version() -> str:
    try:
        return release_spelling(metadata.version("dreamference"))
    except metadata.PackageNotFoundError:
        return _SOURCE_VERSION


__version__ = _installed_version()
