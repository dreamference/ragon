# ask-evals

The phone-chat assistant's own regression suite: [specs/DREAMFERENCE_RAGON_ASSISTANT_EVALS.md](../../specs/DREAMFERENCE_RAGON_ASSISTANT_EVALS.md) §5 (design), §9 (Phase 0) and §11 (what was built). It drives the real `ragon` against the model server, so it is not part of the test suite. Its harness is tested in `tests/test_ask_evals.py` with a stand-in `ragon` and no model.

```bash
.venv/bin/python scripts/ask_evals/run.py --plan                       # the scenarios; runs nothing
.venv/bin/python scripts/ask_evals/run.py --reps 1,2,3                 # every scenario, three times
.venv/bin/python scripts/ask_evals/run.py --only s6-,s7- --reps 1      # some families
.venv/bin/python scripts/ask_evals/run.py --rescore ~/.cache/dreamference/ask-evals/runs/<stamp>
```

- **Offline by construction.** Each session runs in `bwrap --unshare-net`: loopback only. The stand-in SearXNG and proxy (`askevals/standins.py`) listen there, and the model comes in through a Unix socket in `$XDG_RUNTIME_DIR`, joined to `127.0.0.1:<--model-port>` outside (`askevals/forward.py`). Codex's own sandbox (`workspace-write`) runs nested inside, as in production.
- **One session per scenario run**, with a fresh `CODEX_HOME` and scratch folder under `~/.cache/dreamference/ask-evals/runs/`, never `/tmp`. The first turn is `ragon exec --json`, each later one `ragon exec … resume --last`. The prompt is `ask` (`DREAMFERENCE_RAGON_PROMPT`).
- **The harness's own bound:** a turn that prints nothing for 180 s, or runs past its limit (600 s by default), is cut and counted.
- **Scenarios** are `scenarios/<id>.json`, all synthetic. A scenario holds:
  - `turns` (text, images drawn by `askevals/images.py`, an optional `faketime` offset);
  - `web` (what the stand-ins answer, faults included);
  - `expect` (the checks in `askevals/checks.py`).

  A turn's text is what the bridge would compose: an image whose text Tesseract read arrives as `[Screenshot text: …]`.
- **Scores are rules**, never a model: tool calls, seconds, search-engine scraping (by command and by the proxy's log), sleeps, fetches per URL, abstain/ask/answer, required and forbidden words, reply language, and repetition loops (`askevals/metrics.py`). A scenario fails when it fails in two of three repetitions.
- **Citations.** `expect.citations` (`min`, `supports` per URL, `forbid`) checks the answer's `[n]` source lines and bare URLs: each must be a page the session fetched (the proxy's log) and its claim must mention a word of that page. The summary ends with the share fetched and the share supporting; `--prompt NAME` runs every scenario under another named prompt for the side-by-side (family `s18-`).
- **The night slot.** `ASK_EVALS=1` in `scripts/swe_bench_ab.sh` runs the suite before the arms, with `--model-port 18000 --hold-gate "night N, assistant evals"`. It is off by default (the owner's D2).
- **Needs:** `bwrap` with the AppArmor profile (`ragon-admin host setup`), Pillow (the venv has it), and, for the stale-thread scenario, `faketime` (`apt install faketime`). Without `faketime` that scenario is reported as skipped, never passed.
