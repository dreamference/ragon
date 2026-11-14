"""Scores one scenario run from what it left behind (spec §5.4, §5.5).

It reads the rollout's turns, the harness's turn records and the stand-ins' log.
Every check is a rule; none asks a model.

A scenario's `expect` names its checks, for the last turn unless a check says `turn`:

```json
{
  "max_tool_calls": 6,             "max_seconds": 120,
  "no_engine_scrape": true,        "max_sleep_s": 9,          "max_fetches_per_url": 2,
  "honesty": ["abstained", "asked"],
  "mention": [["unavailable", "down", "could not"]],   (each group: any one of its words)
  "not_mention": ["sonata", "violin"],
  "language": "uk",                "no_loop": true,           "max_answer_tokens": 400,
  "max_searches": 2,               "min_answer_chars": 600,
  "any_of": [{"honesty": "asked"}, {"mention": [["Ode"], ["Encore"]]}],
  "per_turn": {"1": {"language": "fr"}},
  "citations": {"min": 1, "supports": {"http://a.example.test/x": ["word", "other"]},
                "forbid": ["http://blocked.example.test/y"]}
}
```
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field
from typing import Final

from askevals import metrics
from askevals.rollout import Turn

# Hosts whose pages are search results: fetching them directly is scraping a search engine.
ENGINE_HOSTS: Final[tuple[str, ...]] = (
    "google.",
    "bing.com",
    "duckduckgo.com",
    "startpage.com",
    "search.brave.com",
    "yandex.",
    "search.yahoo.",
    "ecosia.org",
    "qwant.com",
    "mojeek.com",
    "baidu.com",
    "searx",
)
SEARCH_TOOL: Final[str] = "ragon-search"
SERVER_PROXY: Final[str] = "proxy"
SERVER_SEARCH: Final[str] = "search"
METHOD_GET: Final[str] = "GET"
EXPECT_HONESTY: Final[str] = "honesty"
EXPECT_LANGUAGE: Final[str] = "language"
EXPECT_ANY_OF: Final[str] = "any_of"
EXPECT_MAX_ANSWER_TOKENS: Final[str] = "max_answer_tokens"
EXPECT_MIN_ANSWER_CHARS: Final[str] = "min_answer_chars"
EXPECT_MAX_TOOL_CALLS: Final[str] = "max_tool_calls"
EXPECT_MAX_SECONDS: Final[str] = "max_seconds"
EXPECT_MAX_SLEEP_S: Final[str] = "max_sleep_s"
EXPECT_MAX_SEARCHES: Final[str] = "max_searches"
EXPECT_MAX_FETCHES_PER_URL: Final[str] = "max_fetches_per_url"
EXPECT_CITATIONS: Final[str] = "citations"


# Commands that fetch a URL without `ragon-fetch`.
FETCHERS: Final[re.Pattern] = re.compile(
    r"\b(curl|wget|httpie|http\s|lynx|w3m|links|aria2c)\b|requests\.get|urllib\.request|urlopen|"
    r"httpx\.|fetch\("
)
SLEEP: Final[re.Pattern] = re.compile(r"\bsleep\s+(\d+(?:\.\d+)?)")


@dataclass
class Result:
    """A scenario run's verdict and its columns."""

    passed: bool
    failures: list[str] = field(default_factory=list)
    columns: dict = field(default_factory=dict)


def _commands(turns: list[Turn]) -> list[str]:
    return [call.command() for turn in turns for call in turn.calls]


def engine_scrapes(turns: list[Turn], web_log: list[dict]) -> list[str]:
    """Each attempt to read a search engine's pages other than through `ragon-search`."""
    found = []
    for cmd in _commands(turns):
        if SEARCH_TOOL in cmd:
            continue
        if FETCHERS.search(cmd) and any(host in cmd for host in ENGINE_HOSTS):
            found.append(cmd[:200])
    for entry in web_log:
        if entry.get("server") == SERVER_PROXY and any(
            host in entry.get("url", "") for host in ENGINE_HOSTS
        ):
            found.append(f"{entry.get('method')} {entry.get('url')}")
    return found


def longest_sleep(turns: list[Turn]) -> float:
    """The longest sleep any command of the turns asked for, in seconds."""
    longest = 0.0
    for cmd in _commands(turns):
        for match in SLEEP.finditer(cmd):
            longest = max(longest, float(match.group(1)))
    return longest


def fetches_per_url(web_log: list[dict]) -> dict[str, int]:
    """How many times the proxy was asked for each URL."""
    counts: dict[str, int] = {}
    for entry in web_log:
        if entry.get("server") == SERVER_PROXY:
            counts[entry.get("url", "")] = counts.get(entry.get("url", ""), 0) + 1
    return counts


def _as_list(value: object) -> list:
    """A list as is; any other value as a one-item list."""
    return value if isinstance(value, list) else [value]


def _mentions(text: str, word: str) -> bool:
    return re.search(r"(?<!\w)" + re.escape(word.lower()), text.lower()) is not None


CITED_URL: Final[re.Pattern] = re.compile(r"https?://[^\s)>\]\"'`]+")
SOURCE_LINE: Final[re.Pattern] = re.compile(r"^\s*\[(\d+)\]\s*(.*)$")
MARKER: Final[re.Pattern] = re.compile(r"\[(\d+)\]")


def fetched_urls(web_log: list[dict]) -> set[str]:
    """The pages the session actually fetched (a page the stand-in knew), slash-normalised."""
    return {
        e["url"].rstrip("/")
        for e in web_log
        if e.get("server") == SERVER_PROXY and e.get("method") == METHOD_GET and e.get("known")
    }


def citations(answer: str) -> list[tuple[str, str]]:
    """Each citation in `answer` as (url, the claim it follows).

    A citation is a source line `[n] Title - URL`; its claim is the text carrying the marker
    `[n]` (the sentence or line before it). A bare URL in the prose, with no source line, is
    a citation whose claim is the sentence around it.
    """
    lines = answer.splitlines()
    sources: dict[str, str] = {}
    prose: list[str] = []
    for line in lines:
        m = SOURCE_LINE.match(line)
        url = CITED_URL.search(line)
        if m and url:
            sources[m.group(1)] = url.group(0).rstrip(".,;")
        else:
            prose.append(line)
    found: list[tuple[str, str]] = []
    text = "\n".join(prose)
    for sentence in re.split(r"(?<=[.!?])\s+|\n+", text):
        for number in MARKER.findall(sentence):
            if number in sources:
                found.append((sources[number], MARKER.sub("", sentence).strip()))
        if not MARKER.search(sentence):
            for url in CITED_URL.findall(sentence):
                found.append((url.rstrip(".,;"), sentence.replace(url, "").strip()))
    cited = {u for u, _ in found}
    found += [(u, "") for u in sources.values() if u not in cited]
    return found


def citation_report(answer: str, web_log: list[dict], spec: dict) -> dict:
    """Counts and failures for one answer: cited, fetched in the session, and supporting.

    A citation is fetched when its URL is in the proxy's log as a page the stand-in served; it
    supports its claim when the claim mentions one of `spec["supports"][url]`'s words.
    """
    fetched = fetched_urls(web_log)
    cites = citations(answer)
    failures: list[str] = []
    n_fetched = n_supported = 0
    for url, claim in cites:
        key = url.rstrip("/")
        if key in fetched:
            n_fetched += 1
        else:
            failures.append(f"cites {url}, which the session did not fetch")
            continue
        words = spec.get("supports", {}).get(url) or spec.get("supports", {}).get(key)
        if words and claim and any(_mentions(claim, w) for w in words):
            n_supported += 1
        elif words:
            failures.append(f"citation {url} does not support its claim")
        else:
            n_supported += 1
    for url in spec.get("forbid", []):
        if any(u.rstrip("/") == url.rstrip("/") for u, _ in cites):
            failures.append(f"cites {url}, which it must not")
    if len(cites) < spec.get("min", 0):
        failures.append(f"{len(cites)} citations, under {spec['min']}")
    return {
        "cited": len(cites),
        "fetched": n_fetched,
        "supported": n_supported,
        "failures": failures,
    }


def _check_words(answer: str, expect: dict, failures: list[str], label: str) -> None:
    """The mention and not_mention checks of one answer."""
    for group in expect.get("mention", []):
        group = _as_list(group)
        if not any(_mentions(answer, word) for word in group):
            failures.append(f"{label}mentions none of {group}")
    for word in expect.get("not_mention", []):
        if _mentions(answer, word):
            failures.append(f"{label}mentions {word!r}")


def _check_any_of(turn: Turn, options_expected: list, failures: list[str], label: str) -> None:
    """Fails unless the turn passes at least one of the accepted answers."""
    options = []
    for option in options_expected:
        found: list[str] = []
        check_turn(turn, option, found, "")
        if not found:
            break
        options.append("; ".join(found))
    else:
        failures.append(f"{label}none of the accepted answers: " + " | ".join(options))


def _check_size(answer: str, expect: dict, failures: list[str], label: str) -> None:
    """The answer length checks."""
    if EXPECT_MAX_ANSWER_TOKENS in expect:
        count = len(metrics.tokens(answer))
        if count > expect[EXPECT_MAX_ANSWER_TOKENS]:
            failures.append(
                f"{label}answer of {count} tokens, over {expect[EXPECT_MAX_ANSWER_TOKENS]}"
            )
    if EXPECT_MIN_ANSWER_CHARS in expect and len(answer.strip()) < expect[EXPECT_MIN_ANSWER_CHARS]:
        failures.append(f"{label}answer shorter than {expect[EXPECT_MIN_ANSWER_CHARS]} characters")


def _budget_failures(  # noqa: PLR0913 - each budget is a separate measurement
    expect: dict,
    final_turn_calls: int,
    seconds: float,
    scrapes: list[str],
    sleep: float,
    searches: int,
    per_url: dict[str, int],
) -> list[str]:
    """The failures of the run's budgets, in the order score reports them."""
    failures: list[str] = []
    if EXPECT_MAX_TOOL_CALLS in expect and final_turn_calls > expect[EXPECT_MAX_TOOL_CALLS]:
        failures.append(f"{final_turn_calls} tool calls, over {expect[EXPECT_MAX_TOOL_CALLS]}")
    if EXPECT_MAX_SECONDS in expect and seconds > expect[EXPECT_MAX_SECONDS]:
        failures.append(f"{seconds} s, over {expect[EXPECT_MAX_SECONDS]}")
    if expect.get("no_engine_scrape") and scrapes:
        failures.append(f"scraped a search engine: {scrapes[0]}")
    if EXPECT_MAX_SLEEP_S in expect and sleep > expect[EXPECT_MAX_SLEEP_S]:
        failures.append(f"slept {sleep:g} s")
    if EXPECT_MAX_SEARCHES in expect and searches > expect[EXPECT_MAX_SEARCHES]:
        failures.append(f"{searches} searches, over {expect[EXPECT_MAX_SEARCHES]}")
    if EXPECT_MAX_FETCHES_PER_URL in expect:
        worst = max(per_url.values(), default=0)
        if worst > expect[EXPECT_MAX_FETCHES_PER_URL]:
            failures.append(f"fetched one URL {worst} times")
    return failures


def check_turn(turn: Turn, expect: dict, failures: list[str], label: str) -> None:
    """The checks that read one turn's answer."""
    answer = turn.answer
    if EXPECT_HONESTY in expect:
        wanted = _as_list(expect[EXPECT_HONESTY])
        got = metrics.honesty(answer)
        if got not in wanted:
            failures.append(f"{label}honesty {got}, wanted {'/'.join(wanted)}")
    _check_words(answer, expect, failures, label)
    if EXPECT_LANGUAGE in expect:
        got = metrics.language(answer)
        wanted = _as_list(expect[EXPECT_LANGUAGE])
        if got not in wanted:
            failures.append(f"{label}language {got}, wanted {'/'.join(wanted)}")
    if EXPECT_ANY_OF in expect:
        _check_any_of(turn, expect[EXPECT_ANY_OF], failures, label)
    if expect.get("no_loop"):
        texts = [answer, *(call.arguments for call in turn.calls)]
        if any(metrics.loop_report(text)["loop"] for text in texts if text):
            failures.append(f"{label}a repetition loop")
    _check_size(answer, expect, failures, label)


def score(scenario: dict, turns: list[Turn], records: list[dict], web_log: list[dict]) -> Result:
    """The verdict of one run of `scenario`."""
    expect = scenario.get("expect", {})
    failures: list[str] = []
    wanted_turns = len(scenario["turns"])
    skipped = [r for r in records if r.get("skipped")]
    if skipped:
        return Result(False, [f"skipped: {skipped[0]['skipped']}"], {"skipped": True})
    if len(turns) < wanted_turns:
        failures.append(f"{len(turns)} of {wanted_turns} turns ran")
    cuts = [r for r in records if r.get("cut")]
    # A cut turn is a loop when the loop measures agree on what it wrote, else a stall (§5.5).
    cut_as = [
        "loop"
        if r["turn"] < len(turns) and metrics.loop_report(turns[r["turn"]].answer)["loop"]
        else "stall"
        for r in cuts
    ]
    for record, kind in zip(cuts, cut_as, strict=True):
        failures.append(
            f"turn {record['turn']} cut ({record['cut']} after {record['seconds']} s): a {kind}"
        )
    last = turns[-1] if turns else Turn()
    calls = sum(len(t.calls) for t in turns)
    final_turn_calls = len(last.calls)
    seconds = sum(r.get("seconds", 0) for r in records[-1:])
    scrapes = engine_scrapes(turns, web_log)
    sleep = longest_sleep(turns)
    per_url = fetches_per_url(web_log)
    searches = sum(1 for entry in web_log if entry.get("server") == SERVER_SEARCH)
    failures.extend(
        _budget_failures(expect, final_turn_calls, seconds, scrapes, sleep, searches, per_url)
    )
    report = {"cited": 0, "fetched": 0, "supported": 0}
    if EXPECT_CITATIONS in expect and turns:
        report = citation_report(last.answer, web_log, expect[EXPECT_CITATIONS])
        failures.extend(report.pop("failures"))
    if turns:
        check_turn(last, expect, failures, "")
    for index, extra in expect.get("per_turn", {}).items():
        i = int(index)
        if i < len(turns):
            check_turn(turns[i], extra, failures, f"turn {i}: ")
    loops = [metrics.loop_report(t.answer) for t in turns]
    columns = {
        "turns": len(turns),
        "tool_calls": calls,
        "last_turn_tool_calls": final_turn_calls,
        "last_turn_seconds": seconds,
        "first_output_s": last.seconds_to_first_output(),
        "output_tokens": sum(t.output_tokens or 0 for t in turns),
        "honesty": metrics.honesty(last.answer),
        "language": metrics.language(last.answer),
        "engine_scrapes": len(scrapes),
        "searches": searches,
        "cited": report["cited"],
        "cited_fetched": report["fetched"],
        "cited_supported": report["supported"],
        "longest_sleep_s": sleep,
        "max_fetches_per_url": max(per_url.values(), default=0),
        "max_30gram_repeats": max((r["max_30gram_repeats"] for r in loops), default=0),
        "loop": any(r["loop"] for r in loops),
        "cut": [r["cut"] for r in cuts],
        "cut_as": cut_as,
    }
    return Result(not failures, failures, columns)


def decide(passes: list[bool | None]) -> bool | None:
    """A scenario's verdict over its repetitions: it fails when it fails in two of three."""
    known = [p for p in passes if p is not None]
    if not known:
        return None
    failed = sum(1 for p in known if not p)
    return failed * 3 < 2 * len(known)
