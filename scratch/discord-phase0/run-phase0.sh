#!/usr/bin/env bash
# Phase 0 of the Discord bridge (specs/DREAMFERENCE_RAGON_DISCORD.md §10), DMs with the paired owner only.
# Runs the bridge (`ragon chat serve`) against the owner's throwaway application in a scratch CODEX_HOME,
# walks the checklist with the owner's DM as the far end, and records every step in log.md.
# The token is read from a file by python only: never an argument, never echoed, masked in every log line.
# Usage: run-phase0.sh [TOKEN_FILE]   (default ~/.config/dreamference/discord-bot-token; RAGON_BIN overrides `ragon`)
set -u
TOKEN_FILE="${1:-$HOME/.config/dreamference/discord-bot-token}"
if [ ! -f "$TOKEN_FILE" ]; then
  echo "run-phase0: token file $TOKEN_FILE is missing; the owner writes it (mode 600) first." >&2
  exit 1
fi
if [ -n "$(find "$TOKEN_FILE" -maxdepth 0 -perm /077)" ]; then
  echo "run-phase0: token file $TOKEN_FILE must be mode 600." >&2
  exit 1
fi
if [ ! -t 0 ]; then
  echo "run-phase0: needs a terminal: the owner answers each step." >&2
  exit 1
fi
HERE="$(cd "$(dirname "$0")" && pwd)"
LOG="$HERE/log.md"
RAGON="${RAGON_BIN:-ragon}"
command -v "$RAGON" >/dev/null || { echo "run-phase0: $RAGON not found (set RAGON_BIN)." >&2; exit 1; }

# A scratch CODEX_HOME: the owner's ~/.ragon linked in, except `chat`, which is the scratch bridge state.
REAL_HOME="${CODEX_HOME:-$HOME/.ragon}"
SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/discord-phase0.XXXXXX")"
mkdir -p "$SCRATCH/chat"; chmod 700 "$SCRATCH/chat"
if [ -d "$REAL_HOME" ]; then
  for e in "$REAL_HOME"/* "$REAL_HOME"/.[!.]*; do
    [ -e "$e" ] || continue
    [ "$(basename "$e")" = chat ] || ln -s "$e" "$SCRATCH/$(basename "$e")"
  done
fi
export CODEX_HOME="$SCRATCH"
SERVE_PID=""

# mask: copies stdin to stdout with the token's value replaced (token read from the file by python).
mask() { python3 -I -c '
import sys
t = open(sys.argv[1]).read().strip()
for line in sys.stdin:
    sys.stdout.write(line.replace(t, "[token]") if t else line)
' "$TOKEN_FILE"; }
note() { printf '%s\n' "$*" | mask >> "$LOG"; }
step() { printf '\n## %s\n\n' "$*" | tee -a "$LOG" >/dev/null; echo; echo "== $*"; }
ask() { # ask "question" -> sets ANSWER (pass/fail/skip + note)
  local a n
  read -r -p "$1 [y/n/skip] " a
  read -r -p "  note (what you saw, one line): " n
  case "$a" in y|Y) a=PASS;; n|N) a=FAIL;; *) a=SKIP;; esac
  ANSWER="$a"; note "- $1 -> **$a** — ${n:-no note}"
}
wait_enter() { read -r -p "$1 [enter when done] " _; }
users() { "$RAGON" chat discord users 2>/dev/null | grep -c '^[0-9]'; }
state() { # connections in the last 24 hours, from the bridge's own status line
  "$RAGON" chat status 2>&1 | grep -i '^Discord' | mask | head -1; }
start_bridge() {
  "$RAGON" chat serve 2>&1 | mask >> "$HERE/serve.log" &
  SERVE_PID=$!
  sleep 8
}
stop_bridge() {
  if [ -n "$SERVE_PID" ] && kill -0 "$SERVE_PID" 2>/dev/null; then kill "$SERVE_PID" 2>/dev/null; wait "$SERVE_PID" 2>/dev/null; fi
  SERVE_PID=""
}
cleanup() { stop_bridge; rm -rf "$SCRATCH"; }
trap cleanup EXIT

: > "$HERE/serve.log"
{ echo "# Discord Phase 0 run"; echo; echo "started $(date -u +%FT%TZ); scope: DMs with the paired owner only; token file present (value never read here by a person)."; } > "$LOG"

step "Setup: write the scratch config from the token file"
# The application id and the checks of `discord setup` (public bot, intents), without a terminal prompt.
CHECK="$(python3 -I - "$TOKEN_FILE" "$SCRATCH/chat/discord.json" <<'PY'
import json, os, sys, time, urllib.request
tok = open(sys.argv[1]).read().strip()
def get(path):
    r = urllib.request.Request("https://discord.com/api/v10" + path, headers={"Authorization": "Bot " + tok, "User-Agent": "DiscordBot (ragon-phase0, 0)"})
    return json.load(urllib.request.urlopen(r, timeout=20))
try:
    me, app = get("/users/@me"), get("/applications/@me")
except Exception as e:
    print("token refused or Discord unreachable:", type(e).__name__); sys.exit(1)
flags = app.get("flags", 0)
if app.get("bot_public") or flags & ((1 << 18) | (1 << 19)):
    print("refused: the application is a public bot or has the Message Content intent"); sys.exit(1)
fd = os.open(sys.argv[2], os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
with os.fdopen(fd, "w") as f:
    json.dump({"token": tok, "consented": int(time.time()), "application_id": app["id"], "users": {}}, f)
print("ok: bot", me.get("username", "?"), "private, no message-content intent")
PY
)" || { echo "run-phase0: $CHECK" >&2; note "Setup failed: $CHECK"; exit 1; }
echo "$CHECK"; note "Setup: $CHECK"

step "Pair (§3.3)"
"$RAGON" chat discord pair | tee /dev/tty | grep -v '/pair' | mask >> "$LOG"
note "A pairing code was issued (valid 10 minutes, once; not logged)."
start_bridge
note "Bridge started: $(state)"
echo "Phase 0 item 1: install the app to YOUR account only (no server), open its DM, and send the /pair line above."
for _ in $(seq 1 60); do [ "$(users)" -ge 1 ] && break; sleep 5; done
if [ "$(users)" -ge 1 ]; then note "- paired user count: $(users) -> **PASS**"; else note "- no pairing within 5 minutes -> **FAIL**"; echo "not paired; stopping." >&2; exit 1; fi
ask "Item 1 (D3): did the plain DM reach the bot and the BOT_DM commands show, with no shared server?"
ask "Item 2: did a slash command (/status) and a button answer within 3 s (no 'application did not respond')?"

step "A question (§3.5, §3.7)"
wait_enter "In the DM send: What is 2+2? Answer in one word."
ask "Status line 'Working on it…' with Stop appeared, then vanished; the answer came as a new message?"

step "An approval with a nonce (§3.6)"
wait_enter "In the DM send: Run the shell command 'echo phase0' and show me the output (the agent should ask first)."
ask "Buttons Approve (green) / Decline (red) appeared; pressing one left a disabled button with the outcome?"
wait_enter "Press an OLD approval button again (or any stale one) if you have one"
ask "A stale press was ignored or answered 'no longer open'?"

step "/stop (§3.5)"
wait_enter "In the DM send: Write 2000 words about tides. Then, while it works, press Stop (or send /stop)."
ask "The turn stopped and the partial came back ending '(stopped)'?"

step "Air gap (§3.9)"
echo "Turn Ragon's air gap on (/airgapped on in the owner's Ragon), then DM the bot something."
ask "Did the bot answer one line saying why, and relay nothing? (skip if you do not run the air gap now)"
wait_enter "Turn the air gap off again"
ask "Did a line say it was air-gapped from <time> to <time>?"

step "Items 3, 6, 7, 8 (owner's phone)"
note "- Gateway hosts seen in serve.log: $(grep -o 'wss://[a-z0-9./-]*' "$HERE/serve.log" | sort -u | tr '\n' ' ')"
wait_enter "Send an answer-producing question that makes the bot print a bare URL, a <bracketed> one and a [markdown](https://example.com) link"
ask "Item 6: no link preview shown in the client?"
wait_enter "Send a screenshot, then a ~15 MB file, then a voice message"
ask "Item 7: image read, file named, voice transcribed or dropped with one line?"
ask "Item 8: a second account's DM got one 'private Ragon' reply a day (skip without a second account)"

step "Item 9: the identify budget"
before="$(state)"
for i in $(seq 1 10); do stop_bridge; start_bridge; done
after="$(state)"
note "- before: $before"
note "- after 10 restarts: $after"
ask "Item 9: the connection count rose by far fewer than 10 (resumes, not identifies)?"

step "Off (§4)"
stop_bridge
"$RAGON" chat discord off 2>&1 | mask >> "$LOG"
if [ -e "$SCRATCH/chat/discord.json" ]; then note "- discord.json still present after off -> **FAIL**"; else note "- discord.json deleted by off -> **PASS**"; fi
note "Done $(date -u +%FT%TZ). The throwaway application is deleted in the Developer Portal by the owner."
echo; echo "Phase 0 recorded in $LOG"
