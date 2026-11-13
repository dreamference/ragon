"""Single-stream decode and prefill speed of the running model server, with a correctness check.

The method (specs/DREAMFERENCE_INFERENCE.md §5.3): one stream,
temperature 0, thinking off, a warm-up request per prompt, then the median of three, decode counted
net of time to first token. A comparison measures every arm with this script on the same night.
The A/B of specs/DREAMFERENCE_MODELS.md §2.2 runs it on both checkpoints.

Speed is worthless without evidence the output is still right (on SM121 a wrong kernel corrupts
silently), so each answer is checked: the prose must be words, the code must compile, the JSON must
parse as the asked-for array. A failed check is reported and makes the exit status 1.

    python3 scripts/decode_speed.py [--host http://localhost:8000] [--runs 3] [--out result.json]

`--out` writes the same `result.json` as `ragon-profile` (schema 1, specs/DREAMFERENCE_PROFILING.md),
so the two tools' files compare with one reader; this script fills the fields it measures and leaves
the machine and acceptance figures `null`.

Standard library only, so it runs from any Python. It talks to the model server and nothing else.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import statistics
import sys
import time
import urllib.request
import uuid
from pathlib import Path
from typing import Any, Final

PROMPTS: Final[dict[str, str]] = {
    "prose": "Write a 500-word essay on the history of the printing press, in plain paragraphs.",
    "code": (
        "Write a Python implementation of a red-black tree with insert, delete and search, "
        "as one module. Reply with the code only, in one ```python block."
    ),
    "json": (
        "Return a JSON array of 25 objects describing fictional people, each with the keys id "
        "(an integer), name, email, city and age (an integer). Reply with the JSON only."
    ),
}
MAX_TOKENS: Final[int] = 512
# The prefill probe: 1,100 sentences are about 13,400 fresh tokens with Qwen3.8's tokenizer, the size
# of the production figure's prompt. A random first line keeps the prefix cache from serving any of it.
PREFILL_SENTENCES: Final[int] = 1_100

# The SSE sentinel that ends every stream; its payload is the terminator of the protocol, not data.
SSE_DONE: Final[str] = "data: [DONE]"
# The answer the prefill probe asks for, so its correctness check does not carry a magic string.
PREFILL_EXPECTED: Final[str] = "item42"
# What each kind of answer must contain.
PROSE_MIN_WORDS: Final[int] = 200
PROSE_TOPIC: Final[str] = "print"
CODE_CLASS: Final[str] = "class"
CODE_DEF: Final[str] = "def"
PEOPLE_NAME_KEY: Final[str] = "name"
# The kinds of answers, so the dispatcher does not carry magic strings.
KIND_PROSE: Final[str] = "prose"
KIND_CODE: Final[str] = "code"


def post_stream(host: str, model: str, content: str, max_tokens: int) -> dict[str, Any]:
    """One streamed chat completion: TTFT, decode time, tokens and the text."""
    body = json.dumps(
        {
            "model": model,
            "messages": [{"role": "user", "content": content}],
            "max_tokens": max_tokens,
            "temperature": 0,
            "stream": True,
            "stream_options": {"include_usage": True},
            "chat_template_kwargs": {"enable_thinking": False},
        }
    ).encode()
    request = urllib.request.Request(
        f"{host}/v1/chat/completions", data=body, headers={"Content-Type": "application/json"}
    )
    start = time.monotonic()
    text, usage, first, end = _read_stream(request)
    first = first or end
    tokens = int(usage.get("completion_tokens") or 0)
    decode_s = end - first
    return {
        "ttft_s": round(first - start, 3),
        "decode_s": round(decode_s, 3),
        "completion_tokens": tokens,
        "prompt_tokens": int(usage.get("prompt_tokens") or 0),
        "decode_tps": round((tokens - 1) / decode_s, 2) if tokens > 1 and decode_s > 0 else 0.0,
        "text": "".join(text),
    }


def _read_stream(
    request: urllib.request.Request,
) -> tuple[list[str], dict[str, Any], float, float]:
    """Drain the SSE response: the text pieces, the usage block, TTFT and the end time."""
    first: float | None = None
    text: list[str] = []
    usage: dict[str, Any] = {}
    with urllib.request.urlopen(request, timeout=900) as response:
        for raw in response:
            chunk, piece = _sse_line(raw)
            if chunk is None:
                continue
            if chunk.get("usage"):
                usage = chunk["usage"]
            if piece:
                first = _note_piece(piece, first, text)
    end = time.monotonic()
    return text, usage, first or end, end


def _note_piece(piece: str, first: float | None, text: list[str]) -> float | None:
    """Stash a content piece, stamping the first token's arrival time when it comes."""
    if first is None:
        first = time.monotonic()
    text.append(piece)
    return first


def _sse_line(raw: bytes) -> tuple[dict[str, Any] | None, str]:
    """One SSE line of the stream: the parsed chunk and its content, or `(None, "")` to skip."""
    line = raw.decode().strip()
    if not line.startswith("data: ") or line == SSE_DONE:
        return None, ""
    chunk: dict[str, Any] = json.loads(line[6:])
    piece = "".join(
        (choice.get("delta") or {}).get("content") or "" for choice in chunk.get("choices") or []
    )
    return chunk, piece


def correct(kind: str, text: str) -> tuple[bool, str]:
    """Whether an answer is plausibly right, and why not."""
    if kind == KIND_PROSE:
        return _prose_correct(text)
    if kind == KIND_CODE:
        return _code_correct(text)
    return _json_correct(text)


def _prose_correct(text: str) -> tuple[bool, str]:
    words = re.findall(r"[A-Za-z]{2,}", text)
    if len(words) < PROSE_MIN_WORDS or PROSE_TOPIC not in text.lower():
        return False, f"{len(words)} words, or the topic is missing"
    return True, ""


def _code_correct(text: str) -> tuple[bool, str]:
    match = re.search(r"```(?:python)?\n(.*?)(?:```|$)", text, re.S)
    source = match.group(1) if match else text
    # A reply cut at max_tokens may end mid-statement; compile what is complete.
    lines = source.splitlines()
    for cut in range(len(lines), max(len(lines) - 15, 0), -1):
        try:
            compile("\n".join(lines[:cut]), "<answer>", "exec")
        except SyntaxError:
            continue
        if CODE_CLASS in source and CODE_DEF in source:
            return True, ""
        return False, "no class or function definitions"
    return False, "the code does not compile"


def _json_correct(text: str) -> tuple[bool, str]:
    match = re.search(r"\[.*", text, re.S)
    candidate = match.group(0) if match else text
    candidate = candidate.split("```")[0]
    try:
        data = json.loads(candidate)
    except ValueError:
        # Cut at max_tokens: the objects that are complete must still parse.
        cut = candidate.rfind("},")
        try:
            data = json.loads(candidate[: cut + 1] + "]") if cut > 0 else None
        except ValueError:
            data = None
    if not _people(data):
        return False, "not a JSON array of people"
    return True, ""


def _people(data: object) -> bool:
    """Whether the parsed answer is a non-empty JSON array of people, each with the name key."""
    return (
        isinstance(data, list)
        and bool(data)
        and all(isinstance(o, dict) and PEOPLE_NAME_KEY in o for o in data)
    )


def server_info(host: str) -> dict[str, Any]:
    """SGLang's own speculative acceptance figure, when the server reports one."""
    try:
        with urllib.request.urlopen(f"{host}/get_server_info", timeout=10) as response:
            info = json.loads(response.read())
    except Exception:  # noqa: BLE001 - a best-effort probe, any failure means no figure
        return {}
    states = info.get("internal_states") or [{}]
    value = (states[0] or {}).get("avg_spec_accept_length")
    return {"avg_spec_accept_length": value} if value is not None else {}


def served_model(host: str) -> tuple[str, int | None]:
    """The id of the model the server reports and its max context, when it reports one."""
    with urllib.request.urlopen(f"{host}/v1/models", timeout=10) as response:
        entry = json.loads(response.read())["data"][0]
    return entry["id"], entry.get("max_model_len")


def result_json(
    model: str,
    host: str,
    runs: int,
    kinds: dict[str, dict[str, Any]],
    prefill: dict[str, Any] | None,
    failures: int,
) -> dict[str, Any]:
    """The `result.json` of ragon-profile (schema 1), single-stream, from this script's figures."""
    figures: dict[str, Any] = {}
    for kind, entry in kinds.items():
        figures[kind] = {
            "ttft_s": {"median": entry["ttft_s"], "spread_pct": None, "n": runs},
            "cache_state": None,
            "prompt_tokens": None,
            "reused_tokens": None,
            "prefill_ms_per_1k": None,
            "decode_tok_s": entry["decode_tps"],
            "inter_token_gap_ms": None,
            "tokens_per_pass": None,
            "acceptance_pct": None,
            "draft_ms_per_step": None,
            "verify_ms_per_step": None,
            "step_period_ms": None,
            "energy_j_per_token": None,
        }
    if prefill:
        figures["prefill"] = {
            "prompt_tokens": prefill["prompt_tokens"],
            "ttft_s": prefill["ttft_s"],
            "prefill_tok_s": prefill["prefill_tps"],
        }
    same = sum(1 for e in kinds.values() if e["identical_runs"])
    state = "clean" if not failures else f"{failures} correctness check(s) failed"
    manifest = "\n".join(sorted(PROMPTS.values()))
    return {
        "schema": 1,
        "build": {"ragon_version": "n/a", "engine": "n/a", "model": model, "drafter": "n/a"},
        "flags": "production",
        "shape": {
            "name": "one-stream",
            "reps_counted": runs,
            "warmup_reps": 1,
            "streams": 1,
            "long_prefix_chars": None,
        },
        "workload_sha256": hashlib.sha256(manifest.encode()).hexdigest(),
        "window": "no window",
        "started_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "machine": {"node": host, "summary": None, "sm_clock_normal_mhz": None},
        "state": state,
        "identical": f"{same}/{len(kinds)}",
        "figures": figures,
        "verdict": (
            "no verdict (single arm; compare needs two runs)"
            if not failures
            else f"no verdict ({state})"
        ),
    }


def main(argv: list[str] | None = None) -> int:
    """Run the three prompts and the prefill probe, report to stdout, exit 1 on any wrong answer."""
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--host", default="http://localhost:8000")
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--out", default=None, help="write the full result as JSON here")
    parser.add_argument(
        "--no-prefill", action="store_true", help="skip the 13K-token prefill probe"
    )
    args = parser.parse_args(argv)
    model, context = served_model(args.host)
    result: dict[str, Any] = {
        "model": model,
        "max_model_len": context,
        "host": args.host,
        "when": time.strftime("%Y-%m-%dT%H:%M:%S%z"),
        "runs": args.runs,
        "kinds": {},
    }
    failures = 0
    print(  # noqa: T201 - the report on stdout
        f"Model {model}: single stream, temperature 0, thinking off, median of {args.runs} after a warm-up"
    )
    for kind, prompt in PROMPTS.items():
        post_stream(args.host, model, prompt, MAX_TOKENS)
        before = server_info(args.host)
        runs = [post_stream(args.host, model, prompt, MAX_TOKENS) for _ in range(args.runs)]
        ok, why = correct(kind, runs[-1]["text"])
        failures += 0 if ok else 1
        identical = len({r["text"] for r in runs}) == 1
        entry = {
            "decode_tps": statistics.median(r["decode_tps"] for r in runs),
            "ttft_s": statistics.median(r["ttft_s"] for r in runs),
            "completion_tokens": [r["completion_tokens"] for r in runs],
            "correct": ok,
            "why": why,
            "identical_runs": identical,
            "accept": server_info(args.host) or before,
            "sample": runs[-1]["text"][:400],
        }
        result["kinds"][kind] = entry
        print(  # noqa: T201 - the report on stdout
            f"  {kind:<6} {entry['decode_tps']:6.1f} tok/s  TTFT {entry['ttft_s']:.2f} s  "
            f"tokens {entry['completion_tokens']}  {'correct' if ok else 'WRONG: ' + why}"
            f"{'' if identical else '  (runs differ)'}"
        )
    if not args.no_prefill:
        filler = " ".join(f"item{i} is followed by item{i + 1}." for i in range(PREFILL_SENTENCES))
        prompt = f"Session {uuid.uuid4()}.\n{filler}\nWhich item follows item41? Answer with the item only."
        run = post_stream(args.host, model, prompt, 8)
        tps = run["prompt_tokens"] / run["ttft_s"] if run["ttft_s"] > 0 else 0.0
        ok = PREFILL_EXPECTED in run["text"]
        failures += 0 if ok else 1
        result["prefill"] = {
            "prompt_tokens": run["prompt_tokens"],
            "ttft_s": run["ttft_s"],
            "prefill_tps": round(tps, 1),
            "answer": run["text"][:80],
            "correct": ok,
        }
        print(  # noqa: T201 - the report on stdout
            f"  prefill {run['prompt_tokens']} tokens in {run['ttft_s']:.1f} s: {tps:,.0f} tok/s  "
            f"{'correct' if ok else 'WRONG: ' + run['text'][:40]!r}"
        )
    if args.out:
        with Path(args.out).open("w") as handle:
            json.dump(
                result_json(
                    model, args.host, args.runs, result["kinds"], result.get("prefill"), failures
                ),
                handle,
                indent=2,
            )
            handle.write("\n")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
