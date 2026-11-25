#!/usr/bin/env python3
"""A stand-in for `ragon` that both night runners drive the same way (`night_run.rs` and
`NightShiftBestOfN`): attempt k does what `FAKE_BON_PLAN[k-1]` says, read from the worktree's
`-a<k>` suffix. `good` writes hello.txt with "hi" (the test passes), `bad` with "bye" (it fails),
`GOOD`/`fine` pass in two ways a judge can tell apart, `none` changes nothing, `hang` sleeps and
`error` exits 1. Its thread id is fixed per attempt, so two runs write the same record."""
import json
import os
import re
import sys
import time

args = sys.argv[1:]
if args[0] == "sandbox":
    command = args[args.index("--") + 1:]
    os.environ["FAKE_RAGON_SANDBOXED"] = "1"
    os.execvp(command[0], command)
calls = os.environ.get("FAKE_RAGON_CALLS")
if calls:
    with open(calls, "a") as log:
        log.write(json.dumps(args) + "\n")
cwd = args[args.index("-C") + 1]
out = args[args.index("-o") + 1]
match = re.search(r"-a(\d+)$", cwd)
plan = json.loads(os.environ.get("FAKE_BON_PLAN", "[]"))
mode = plan[int(match.group(1)) - 1] if match and plan else "good"
attempt = match.group(1) if match else "0"
print(json.dumps({"type": "thread.started", "thread_id": f"thread-a{attempt}"}), flush=True)
if mode == "hang":
    time.sleep(600)
if mode == "error":
    sys.exit(1)
if mode != "none":
    text = {"good": "hi", "bad": "bye", "GOOD": "hi GOOD", "fine": "hi fine"}[mode]
    with open(os.path.join(cwd, "hello.txt"), "w") as handle:
        handle.write(text + "\n")
with open(out, "w") as handle:
    handle.write("Nothing needed changing." if mode == "none" else "Wrote hello.txt.")
print(json.dumps({"type": "turn.completed"}), flush=True)
