"""The suite's pure judgements: what a received answer shows, with no client and no network.

Telegram answers arrive as TDLib objects (`messageText` with entities, or `messageRichMessage`
with page blocks); Signal answers as text with style ranges. Everything here reads those shapes.
"""

from __future__ import annotations

import re
from collections.abc import Iterator
from typing import Any, Final

CLARIFYING: Final = (
    "did you mean",
    "do you mean",
    "could you clarify",
    "can you clarify",
    "could you confirm",
    "please clarify",
    "which one do you",
    "what do you mean",
    "are you referring",
    "could you specify",
)
ATTACHMENT: Final = (
    "attachment",
    "you attached",
    "the attached",
    "attached file",
    "attached image",
    "attached document",
    "can't see an image",
    "cannot see an image",
    "can't see the image",
    "no image",
)
STOPPED_MARK: Final = "(stopped)"
# TDLib object types (the `@type` field) the judgements read.
MESSAGE_TEXT: Final = "messageText"
RICH_MESSAGE: Final = "messageRichMessage"
RICH_TEXT_PLAIN: Final = "richTextPlain"
INLINE_KEYBOARD: Final = "replyMarkupInlineKeyboard"
BLOCK_TABLE: Final = "pageBlockTable"
BLOCK_LIST: Final = "pageBlockList"
BLOCK_LIST_ITEM: Final = "pageBlockListItem"
BLOCK_PREFORMATTED: Final = "pageBlockPreformatted"
PRE_CODE: Final = "textEntityTypePreCode"
# Separates a link's scheme from the rest.
SCHEME_SEPARATOR: Final = ":"
# A reply shorter than this that ends in a question mark counts as a clarifying question.
SHORT_QUESTION: Final = 400
# The notice a bridge sends after Stop when the answer had no words yet.
STOPPED_NOTICE: Final = "Stopped."
# A Markdown code fence left in the text.
FENCE: Final = "```"
# Ragon's own web tools, as they appear in a command line.
RAGON_SEARCH: Final = "ragon-search"
RAGON_FETCH: Final = "ragon-fetch"
# Draft lengths up to this many characters are placeholders, not text.
PLACEHOLDER_MAX: Final = 20


def strip_mark(text: str) -> str:
    """A bridge message without the bird the Signal bridge puts before each one."""
    return text.removeprefix("🐦").lstrip()


def walk(value: object) -> Iterator[dict[str, Any]]:
    """Every TDLib object inside `value`, depth first."""
    if isinstance(value, dict):
        yield value
        for item in value.values():
            yield from walk(item)
    elif isinstance(value, list):
        for item in value:
            yield from walk(item)


def plain_text(content: dict[str, Any]) -> str:
    """The readable text of a message's content: `messageText`'s text, or a rich message's words."""
    kind = content.get("@type")
    if kind == MESSAGE_TEXT:
        return content.get("text", {}).get("text", "")
    if kind in ("messageVoiceNote", "messageDocument", "messagePhoto"):
        return content.get("caption", {}).get("text", "")
    words = [
        obj["text"]
        for obj in walk(content)
        if obj.get("@type") == RICH_TEXT_PLAIN and isinstance(obj.get("text"), str)
    ]
    return " ".join(words)


def links(content: dict[str, Any]) -> list[str]:
    """Every link target a message carries: text entities' URLs and rich-text URLs."""
    found = []
    for obj in walk(content):
        if obj.get("@type") in (
            "textEntityTypeTextUrl",
            "richTextUrl",
            "richTextAnchorLink",
        ) and isinstance(obj.get("url"), str):
            found.append(obj["url"])
    return found


def dangerous_links(content: dict[str, Any]) -> list[str]:
    """Link targets with a scheme other than http, https, mailto or tg."""
    bad = []
    for url in links(content):
        scheme = (
            url.split(SCHEME_SEPARATOR, 1)[0].strip().lower() if SCHEME_SEPARATOR in url else ""
        )
        if scheme not in ("http", "https", "mailto", "tg"):
            bad.append(url)
    return bad


def inline_buttons(message: dict[str, Any]) -> list[dict[str, Any]]:
    """The inline keyboard's buttons of a message (none when it has no inline keyboard)."""
    markup = message.get("reply_markup") or {}
    if markup.get("@type") != INLINE_KEYBOARD:
        return []
    return [button for row in markup.get("rows", []) for button in row]


def rich_features(content: dict[str, Any]) -> dict[str, Any]:
    """What a rich message is built from: table, nested list, code languages, links, block count."""
    blocks = (
        content.get("message", {}).get("blocks", []) if content.get("@type") == RICH_MESSAGE else []
    )
    features: dict[str, Any] = {
        "rich": content.get("@type") == RICH_MESSAGE,
        "blocks": len(blocks),
        "table": False,
        "list": False,
        "nested_list": False,
        "code": [],
        "links": len(links(content)),
    }
    for obj in walk(blocks):
        kind = obj.get("@type")
        if kind == BLOCK_TABLE:
            features["table"] = True
        elif kind == BLOCK_LIST:
            features["list"] = True
        elif kind == BLOCK_LIST_ITEM and any(
            inner.get("@type") == BLOCK_LIST for inner in walk(obj.get("blocks", []))
        ):
            features["nested_list"] = True
        elif kind == BLOCK_PREFORMATTED:
            features["code"].append(obj.get("language", ""))
    if content.get("@type") == MESSAGE_TEXT:
        for entity in content.get("text", {}).get("entities", []):
            entity_type = entity.get("type", {})
            if entity_type.get("@type") == PRE_CODE:
                features["code"].append(entity_type.get("language", ""))
    return features


def clarifying_question(text: str) -> bool:
    """Says whether an answer asks the user what they meant instead of answering."""
    lower = text.lower()
    if any(phrase in lower for phrase in CLARIFYING):
        return True
    stripped = lower.rstrip()
    return len(stripped) < SHORT_QUESTION and stripped.endswith("?")


def mentions_attachment(text: str) -> bool:
    """Says whether an answer talks about an attachment or image the user never sent."""
    lower = text.lower()
    return any(phrase in lower for phrase in ATTACHMENT)


def word_count(text: str) -> int:
    """Words in a text."""
    return len(re.findall(r"\w+", text))


def stopped(text: str) -> bool:
    """Says whether a message is a stopped answer the bridge kept (it ends "(stopped)")."""
    return text.rstrip().endswith(STOPPED_MARK)


def stop_kept(texts: list[str], wrote: bool) -> tuple[bool, str]:
    """Judges what a bridge sent after Stop.

    Right is the partial marked "(stopped)" when the answer had words, else the "Stopped." notice
    (the bridges send no empty partial).

    Args:
        texts: The bridge's messages since the question.
        wrote: Whether the answer's words had reached the messenger before Stop.

    Returns:
        tuple[bool, str]: Whether it is right, and what was seen or missing.
    """
    if any(stopped(text) for text in texts):
        return True, "partial kept as (stopped)"
    if wrote:
        return False, "no message ending (stopped) although the answer had words"
    if any(text.strip().lstrip("\N{BIRD}").strip() == STOPPED_NOTICE for text in texts):
        return True, "nothing written before Stop: the Stopped. notice"
    return False, "no message ending (stopped) and no Stopped. notice"


def signal_styles(styles: list[Any]) -> set[str]:
    """The style names of a Signal message's text styles (`BOLD`, `MONOSPACE`, …)."""
    names = set()
    for style in styles:
        if isinstance(style, dict) and style.get("style"):
            names.add(str(style["style"]).upper())
        elif isinstance(style, str):
            # signal-cli prints "start:length:STYLE".
            names.add(style.rsplit(":", 1)[-1].upper())
    return names


def leftover_markdown(text: str) -> list[str]:
    """Markdown syntax that should have become styles: `**bold**`, fences, table rules."""
    found = []
    if re.search(r"\*\*[^*\n]+\*\*", text):
        found.append("**")
    if FENCE in text:
        found.append(FENCE)
    if re.search(r"^\s*\|?\s*:?-{3,}:?\s*\|", text, re.MULTILINE):
        found.append("table rule")
    return found


def uses_curl(commands: list[str]) -> bool:
    """Says whether any of the agent's commands ran `curl` or `wget`."""
    return any(re.search(r"(^|[\s;|&(])(curl|wget)\s", " " + command) for command in commands)


def uses_ragon_search(commands: list[str]) -> bool:
    """Says whether any of the agent's commands ran `ragon-search` or `ragon-fetch`."""
    return any(RAGON_SEARCH in command or RAGON_FETCH in command for command in commands)


def growth(lengths: list[int]) -> list[int]:
    """The successive draft lengths at which a draft's text grew (placeholders of 20 characters or fewer left out)."""
    grown: list[int] = []
    for length in lengths:
        if length > PLACEHOLDER_MAX and (not grown or length > grown[-1]):
            grown.append(length)
    return grown


def searched_first(commands: list[str]) -> bool:
    """Says whether the agent went to `ragon-search`/`ragon-fetch` before any `curl` or `wget`."""
    for command in commands:
        if uses_ragon_search([command]):
            return True
        if uses_curl([command]):
            return False
    return False


def median(values: list[float]) -> float | None:
    """The median, or None for no values."""
    ordered = sorted(values)
    if not ordered:
        return None
    middle = len(ordered) // 2
    return ordered[middle] if len(ordered) % 2 else (ordered[middle - 1] + ordered[middle]) / 2
