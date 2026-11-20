#!/usr/bin/env bash
# The accounts end-to-end gate: specs/DREAMFERENCE_AUTH.md §17, serial tail row 6.
#
# A test primary and a test secondary, then every client: the first-run screen (sign in and
# create, empty password), login status, sessions, logout and logout --all, device auth, an API
# key with the vendor's SDK, the desktop app's form, ragon web's login and Account page,
# Forgejo sign-in, a Night Shift task with the user, a phone paired through the Discord bridge,
# a request through the secondary, a revoked token refused within 60 s, an unauthenticated
# request counted. A red run holds the release.
#
#   scripts/auth_e2e.sh --primary sixth-puffin --secondary fifth-puffin [--dry-run]
#                       [--evidence DIR] [--now "YYYY-MM-DD HH:MM"]
#
# Steps that need a person at a screen or a phone (E1, E8, E9, E10, E12, E13) read
# DIR/E<n>.env, `RESULT=pass` plus free-form notes; a missing file fails the step.
# Steps that run on the nodes are shell commands over ssh.
#
# --dry-run changes nothing and touches no machine: every step is listed, not run. It is never a
# gate pass. A real run refuses outside the GPU window 08:00-20:00 UTC, and stops with exit 3
# (blocked) when a node's install lacks the accounts commands.
#
# Exit: 0 every step passed (or was planned, in a dry run); 1 a step failed; 2 usage;
#       3 blocked (window, install or reachability).
set -u

PRIMARY=""; SECONDARY=""; DRY=0; EVIDENCE=""; NOW=""
WINDOW_START_H=8; WINDOW_END_H=20      # the GPU window of nights and days (UTC)
REVOKE_REFUSAL_SECONDS_MAX=60          # a revoked token is refused within the gate's cache TTL

usage() { echo "usage: $0 --primary HOST --secondary HOST [--dry-run] [--evidence DIR] [--now 'YYYY-MM-DD HH:MM']" >&2; exit 2; }
while [ $# -gt 0 ]; do
  case "$1" in
    --primary) PRIMARY="${2:-}"; shift 2 ;;
    --secondary) SECONDARY="${2:-}"; shift 2 ;;
    --evidence) EVIDENCE="${2:-}"; shift 2 ;;
    --now) NOW="${2:-}"; shift 2 ;;
    --dry-run) DRY=1; shift ;;
    *) usage ;;
  esac
done
[ -n "$PRIMARY" ] && [ -n "$SECONDARY" ] || usage
[ "$PRIMARY" != "$SECONDARY" ] || { echo "the primary and the secondary must differ" >&2; exit 2; }

PASS=0; FAIL=0; PLAN=0
RESULTS=""
record() { # id status text
  RESULTS="$RESULTS$1 $2 $3
"
  case "$2" in pass) PASS=$((PASS+1));; plan) PLAN=$((PLAN+1));; *) FAIL=$((FAIL+1));; esac
  echo "[$2] $1: $3"
}
on() { ssh -o BatchMode=yes -o ConnectTimeout=10 "$1" "bash -lc '$2'"; }

# step ID TEXT HOST COMMAND: a command run over ssh; the dry run lists it.
step() {
  if [ "$DRY" = 1 ]; then record "$1" plan "$2 (on $3: $4)"; return; fi
  if on "$3" "$4" >/dev/null 2>&1; then record "$1" pass "$2"; else record "$1" FAIL "$2"; fi
}
# evidence ID TEXT: a step a person drives; reads DIR/ID.env.
evidence() {
  if [ "$DRY" = 1 ]; then record "$1" plan "$2 (evidence: ${EVIDENCE:-DIR}/$1.env, RESULT=pass)"; return; fi
  if [ -n "$EVIDENCE" ] && grep -qx 'RESULT=pass' "$EVIDENCE/$1.env" 2>/dev/null; then
    record "$1" pass "$2"
  else
    record "$1" FAIL "$2 (no passing $1.env under ${EVIDENCE:-the evidence dir})"
  fi
}

# --- Preflight: the window, reachability, the install.
if [ "$DRY" = 0 ]; then
  hour=$(date -u ${NOW:+-d "$NOW"} +%H); hour=$((10#$hour))
  if [ "$hour" -lt "$WINDOW_START_H" ] || [ "$hour" -ge "$WINDOW_END_H" ]; then
    echo "blocked: outside the GPU window ${WINDOW_START_H}:00-${WINDOW_END_H}:00 UTC" >&2; exit 3
  fi
  for h in "$PRIMARY" "$SECONDARY"; do
    on "$h" 'true' >/dev/null 2>&1 || { echo "blocked: $h unreachable over ssh" >&2; exit 3; }
    on "$h" 'ragon-admin node migrate --help' >/dev/null 2>&1 \
      || { echo "blocked: $h has no accounts commands (install the build first)" >&2; exit 3; }
  done
fi

PRIM_URL="https://$PRIMARY.local:18100"
# --- The run.
step E0a "install: the build is on both nodes" "$PRIMARY" 'ragon-admin node user list'
step E0b "migrate: the primary question answered, the old tokens migrated" "$PRIMARY" 'ragon-admin node primary --self && ragon-admin node migrate'
step E0c "the secondary points at the primary" "$SECONDARY" "ragon-admin node primary $PRIMARY"
evidence E1 "first-run screen: sign in, and create an account with an empty password"
step E2 "login status shows the user and the issuer" "$PRIMARY" 'ragon whoami'
step E3 "sessions lists this device" "$PRIMARY" 'ragon sessions'
step E4 "logout ends this session; logout --all ends every session and keeps API keys" "$PRIMARY" 'ragon logout && ragon logout --all'
step E5 "device auth in both shapes completes" "$PRIMARY" "curl -fsS --cacert ~/.config/dreamference/ca.pem -X POST $PRIM_URL/oauth/device/code -d client_id=ragon"
step E6 "an API key works with the vendor's SDK: list, respond, stream, error" "$PRIMARY" 'python3 -I -c "import openai"'
evidence E7 "the desktop app's sign-in form"
evidence E8 "ragon web: login page and Account page"
step E9 "Forgejo sign-in through the accounts service" "$PRIMARY" 'ragon-admin node user list'
step E10 "a Night Shift task runs as the user" "$PRIMARY" 'ragon-admin night status'
evidence E11 "a phone paired through the Discord bridge"
step E12 "a request through the secondary reaches the engine" "$SECONDARY" 'ragon exec --skip-git-repo-check "say ok"'
step E13 "a revoked token is refused within ${REVOKE_REFUSAL_SECONDS_MAX} s on the secondary" "$SECONDARY" "ragon-admin node user key e2e --revoke e2e && sleep ${REVOKE_REFUSAL_SECONDS_MAX}"
step E14 "an unauthenticated request is refused and counted" "$PRIMARY" 'ragon-admin node user usage --days 1'

echo "passed $PASS, failed $FAIL, planned $PLAN"
if [ "$DRY" = 1 ]; then echo "dry run: not a gate pass"; exit 0; fi
[ "$FAIL" -eq 0 ] || { echo "RED: hold the release"; exit 1; }
echo "GREEN"
