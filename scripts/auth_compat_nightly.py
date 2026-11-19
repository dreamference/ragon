#!/usr/bin/env python3
"""Nightly compatibility run against a real gate (specs/DREAMFERENCE_AUTH.md §18 Live).

Runs the SDK compatibility set (model list, a response, a stream, an error) with a test
user's key, then one upstream Codex turn, and prints one result line per check:

    PASS <check>
    FAIL <check>: <reason>

Exits 1 on any failure, 0 when every check passed. The key is read from the environment
variable named by ``--key-env`` (default ``AUTH_COMPAT_KEY``) and is never printed.
"""

import argparse
import json
import os
import subprocess
import sys
import urllib.error
import urllib.request
from collections.abc import Callable

TIMEOUT_S = 120
OK = 200
ERROR_KEY = "error"


def http(base: str, key: str, path: str, body: dict | None = None) -> tuple[int, bytes]:
    """Call the gate with the test user's Bearer key; return the status and the body."""
    data = None if body is None else json.dumps(body).encode()
    headers = {"Authorization": f"Bearer {key}"}
    if data is not None:
        headers["Content-Type"] = "application/json"
    request = urllib.request.Request(base + path, data=data, headers=headers)
    try:
        with urllib.request.urlopen(request, timeout=TIMEOUT_S) as response:
            return response.status, response.read()
    except urllib.error.HTTPError as error:
        return error.code, error.read()


def check_models(base: str, key: str, model: str) -> None:
    """The model list names the served model."""
    status, body = http(base, key, "/v1/models")
    assert status == OK, f"status {status}"
    ids = [m["id"] for m in json.loads(body)["data"]]
    assert model in ids, f"{model} not in {ids}"


def check_response(base: str, key: str, model: str) -> None:
    """A chat completion returns a message."""
    status, body = http(
        base,
        key,
        "/v1/chat/completions",
        {"model": model, "messages": [{"role": "user", "content": "Say ok."}], "max_tokens": 16},
    )
    assert status == OK, f"status {status}"
    assert json.loads(body)["choices"][0]["message"], "no message"


def check_stream(base: str, key: str, model: str) -> None:
    """A streamed completion ends with [DONE]."""
    status, body = http(
        base,
        key,
        "/v1/chat/completions",
        {
            "model": model,
            "stream": True,
            "messages": [{"role": "user", "content": "Say ok."}],
            "max_tokens": 16,
        },
    )
    assert status == OK, f"status {status}"
    assert body.strip().endswith(b"data: [DONE]"), "no [DONE]"


def check_error(base: str, key: str, model: str) -> None:
    """An unknown model gives the vendor's error shape."""
    status, body = http(
        base,
        key,
        "/v1/chat/completions",
        {"model": "no-such-model", "messages": [{"role": "user", "content": "x"}]},
    )
    assert status in (400, 404), f"status {status}"
    assert ERROR_KEY in json.loads(body), "no error object"


def check_sdk(base: str, key: str, model: str) -> None:
    """The vendor's Python SDK, unmodified, lists models and gets a response."""
    try:
        import openai
    except ImportError as error:
        raise AssertionError("the openai package is not installed") from error
    client = openai.OpenAI(base_url=base + "/v1", api_key=key, max_retries=0, timeout=TIMEOUT_S)
    assert any(m.id == model for m in client.models.list().data), "model not listed"
    reply = client.chat.completions.create(
        model=model, messages=[{"role": "user", "content": "Say ok."}], max_tokens=16
    )
    assert reply.choices[0].message, "no message"
    chunks = list(
        client.chat.completions.create(
            model=model,
            stream=True,
            messages=[{"role": "user", "content": "Say ok."}],
            max_tokens=16,
        )
    )
    assert chunks, "no stream chunks"
    try:
        client.chat.completions.create(
            model="no-such-model", messages=[{"role": "user", "content": "x"}]
        )
    except openai.APIStatusError:
        return
    raise AssertionError("an unknown model did not raise")


def check_codex(base: str, key: str, model: str, codex: str) -> None:
    """An upstream Codex runs one turn through the gate (spec §13 configuration)."""
    env = dict(os.environ, OPENAI_API_KEY=key, OPENAI_BASE_URL=base + "/v1")
    result = subprocess.run(
        [codex, "exec", "--skip-git-repo-check", "-m", model, "Reply with the word ok."],
        env=env,
        capture_output=True,
        text=True,
        timeout=TIMEOUT_S * 2,
        check=False,
    )
    assert result.returncode == 0, f"exit {result.returncode}"
    assert result.stdout.strip(), "no output"


def run_checks(base: str, key: str, model: str, codex: str) -> list[tuple[str, str | None]]:
    """Run every check; return (name, failure reason or None) per check, in order."""
    checks: list[tuple[str, Callable[[], None]]] = [
        ("models", lambda: check_models(base, key, model)),
        ("response", lambda: check_response(base, key, model)),
        ("stream", lambda: check_stream(base, key, model)),
        ("error", lambda: check_error(base, key, model)),
        ("sdk", lambda: check_sdk(base, key, model)),
        ("codex-turn", lambda: check_codex(base, key, model, codex)),
    ]
    results: list[tuple[str, str | None]] = []
    for name, fn in checks:
        try:
            fn()
            results.append((name, None))
        except Exception as error:  # noqa: BLE001  a difference of any kind is a failed check
            results.append((name, str(error).replace(key, "***") or type(error).__name__))
    return results


def main(argv: list[str] | None = None) -> int:
    """Run the nightly set; print one line per check; return 1 on any difference."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--base-url", required=True, help="the gate, e.g. https://node:8000")
    parser.add_argument("--model", required=True)
    parser.add_argument("--key-env", default="AUTH_COMPAT_KEY")
    parser.add_argument("--codex", default="codex", help="the upstream Codex binary")
    args = parser.parse_args(argv)
    key = os.environ.get(args.key_env, "")
    if not key:
        print(f"FAIL setup: {args.key_env} is not set")  # noqa: T201
        return 1
    results = run_checks(args.base_url.rstrip("/"), key, args.model, args.codex)
    for name, reason in results:
        print(f"PASS {name}" if reason is None else f"FAIL {name}: {reason}")  # noqa: T201
    return 1 if any(reason is not None for _, reason in results) else 0


if __name__ == "__main__":
    sys.exit(main())
