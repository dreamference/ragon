#!/usr/bin/env bash
# The accounts end-to-end gate: specs/DREAMFERENCE_AUTH.md §17, serial tail row 6.
#
# A test primary and a test secondary, then every client: the first-run screen (sign in and
# create, empty password), login status, sessions, device auth in both shapes, an API key with
# the vendor's SDK, the desktop app's form, ragon web's login and Account page, Forgejo sign-in,
# a Night Shift task with the user, a phone paired through the Discord bridge, a request through
# the secondary, a revoked token refused within 60 s, unauthenticated requests refused and
# counted, logout and logout --all. A red run holds the release.
#
#   scripts/auth_e2e.sh --primary sixth-puffin --secondary fifth-puffin [--dry-run] [--any-time]
#                       [--evidence DIR] [--now "YYYY-MM-DD HH:MM"]
#
# The script creates its own test user (e2e-<random>, a random password that never leaves the
# nodes' private run folders or appears in argv or output) and removes it, and any account the
# run created, on exit, red runs and interrupts included.
#
# Person steps (a person at a screen or a phone): E7 (the desktop app's form) and E11 (a phone
# paired through the Discord bridge). They read DIR/E<n>.env, `RESULT=pass` plus free-form
# notes; a missing file fails the step. E1 (through a pty) and E8 (over HTTP) are driven by the
# script, which writes DIR/E1.env and DIR/E8.env itself, so they read the same contract.
# Node steps run `scripts/auth_e2e_node.py` (copied into a mode-700 run folder) or the CLIs
# over ssh. With --evidence, each step's output (credentials masked) and exit code go to
# DIR/<id>.log and the summary to DIR/summary.txt.
#
# --dry-run changes nothing and touches no machine: every step is listed, not run. It is never a
# gate pass. A real run refuses outside the GPU window 08:00-20:00 UTC, and stops with exit 3
# (blocked) when a node is unreachable or its install lacks the accounts commands. --any-time
# skips the window check on purpose (a run outside the window is not the owner's gate); the
# summary records that it was used. Steps that run `ragon whoami` get a pty (ssh -tt).
#
# Exit: 0 every step passed (or was planned, in a dry run); 1 a step failed; 2 usage;
#       3 blocked (window, install or reachability).
set -u

PRIMARY=""; SECONDARY=""; DRY=0; EVIDENCE=""; NOW=""; ANY_TIME=0
WINDOW_START_H=8; WINDOW_END_H=20      # the GPU window of nights and days (UTC)
REVOKE_REFUSAL_SECONDS_MAX=60          # a revoked token is refused within the gate's cache TTL
REFUSALS_MIN=2                         # E14: the refusal count rises by this much over both nodes
HERE=$(cd "$(dirname "$0")" && pwd)
HELPER_SRC="$HERE/auth_e2e_node.py"

usage() { echo "usage: $0 --primary HOST --secondary HOST [--dry-run] [--any-time] [--evidence DIR] [--now 'YYYY-MM-DD HH:MM']" >&2; exit 2; }
while [ $# -gt 0 ]; do
  case "$1" in
    --primary) PRIMARY="${2:-}"; shift 2 ;;
    --secondary) SECONDARY="${2:-}"; shift 2 ;;
    --evidence) EVIDENCE="${2:-}"; shift 2 ;;
    --now) NOW="${2:-}"; shift 2 ;;
    --dry-run) DRY=1; shift ;;
    --any-time) ANY_TIME=1; shift ;;
    *) usage ;;
  esac
done
[ -n "$PRIMARY" ] && [ -n "$SECONDARY" ] || usage
[ "$PRIMARY" != "$SECONDARY" ] || { echo "the primary and the secondary must differ" >&2; exit 2; }

TAG=$(od -An -tx1 -N6 /dev/urandom | tr -d ' \n')
TEST_USER="e2e-$TAG"
RUN="/tmp/ragon-auth-e2e.$TAG"           # the private run folder on each node
H="python3 -I $RUN/auth_e2e_node.py"     # the node helper
# The SDK step runs under AUTH_E2E_SDK_PYTHON on the primary: an interpreter the operator prepared with the
# vendor's SDK and httpx (the node's own venv has neither). Default: the system python3.
SDK="${AUTH_E2E_SDK_PYTHON:-python3} -I $RUN/auth_e2e_node.py sdk $RUN"
SESSION="env CODEX_HOME=$RUN/home RAGON_HOME=$RUN/home"

PASS=0; FAIL=0; PLAN=0
RESULTS=""
record() { # id status text
  RESULTS="$RESULTS[$2] $1: $3
"
  case "$2" in pass) PASS=$((PASS+1));; plan) PLAN=$((PLAN+1));; *) FAIL=$((FAIL+1));; esac
  echo "[$2] $1: $3"
}
# on HOST CMD: ssh. ON_TTY=1 (set for one step) gives the command a pty: `ragon whoami` refuses
# to run without a terminal ("stdin is not a terminal").
on() { ssh ${ON_TTY:+-tt} -o BatchMode=yes -o ConnectTimeout=10 "$1" "bash -lc '$2'"; }
# mask: replaces anything that looks like a bearer token, an API key or a JWT.
mask() {
  sed -E -e 's/([Bb]earer[[:space:]]+)[^[:space:]"]+/\1***/g' \
         -e 's/sk-ragon-[A-Za-z0-9_-]+/sk-ragon-***/g' \
         -e 's/eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+(\.[A-Za-z0-9_-]*)?/eyJ***/g' \
         -e 's/("?(access_token|refresh_token|id_token|code_verifier|password)"?[[:space:]]*[:=][[:space:]]*"?)[^",}[:space:]]+/\1***/g'
}
# log ID HOST CMD OUTPUT RC: the step's evidence log, masked before it is written.
log() {
  [ -n "$EVIDENCE" ] || return 0
  printf 'host: %s\ncommand: %s\n--- output\n%s\n--- exit: %s\n' "$2" "$3" "$4" "$5" | mask > "$EVIDENCE/$1.log"
}
OUT=""; RC=0
# run ID HOST CMD: runs a node command, keeps its output in OUT and RC, writes its log.
run() { OUT=$(on "$2" "$3" 2>&1 < /dev/null); RC=$?; log "$1" "$2" "$3" "$OUT" "$RC"; }

# step ID TEXT HOST COMMAND: a command run over ssh; the dry run lists it.
step() {
  if [ "$DRY" = 1 ]; then record "$1" plan "$2 (on $3: $4)"; return; fi
  run "$1" "$3" "$4"
  if [ "$RC" = 0 ]; then record "$1" pass "$2"; else record "$1" FAIL "$2 (exit $RC)"; fi
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
# driven ID TEXT HOST COMMAND: a screen step the script drives; a pass writes DIR/ID.env, which
# `evidence` then reads, so the contract of the person steps holds for it too.
driven() {
  if [ "$DRY" = 1 ]; then record "$1" plan "$2 (on $3: $4; writes ${EVIDENCE:-DIR}/$1.env)"; return; fi
  run "$1" "$3" "$4"
  [ "$RC" = 0 ] && printf 'RESULT=pass\nDRIVEN_BY=scripts/auth_e2e.sh\nAT=%s\n' "$(date -u +%FT%TZ)" > "$EVIDENCE/$1.env"
  evidence "$1" "$2"
}
# put HOST FILE: stdin into a mode-600 file in the run folder (secrets never go in argv).
put() { on "$1" "umask 077 && cat > $RUN/$2"; }

# --- The clean-up: the test user, the account E1 creates, the run folders. Runs on every exit.
CREATED=0; STAGED=""
cleanup() {
  [ "$DRY" = 0 ] || return 0
  if [ "$CREATED" = 1 ]; then
    on "$PRIMARY" "ragon-admin node user remove $TEST_USER; test -s $RUN/created && ragon-admin node user remove \$(cat $RUN/created); true" >/dev/null 2>&1 < /dev/null \
      || echo "warning: remove the test user $TEST_USER on $PRIMARY by hand" >&2
    # Prove the removes took: neither test account may still be listed.
    left=$(on "$PRIMARY" "ragon-admin node user list" 2>&1 < /dev/null) \
      || echo "WARNING: could not list users on $PRIMARY; check that $TEST_USER and $TEST_USER-np are gone" >&2
    if printf '%s\n' "$left" | grep -qE "(^|[[:space:]])$TEST_USER(-np)?([[:space:]]|$)"; then
      echo "WARNING: test accounts remain on $PRIMARY: remove $TEST_USER and $TEST_USER-np by hand" >&2
    fi
  fi
  for h in $STAGED; do on "$h" "rm -rf $RUN" >/dev/null 2>&1 < /dev/null; done
}
trap 'rc=$?; cleanup; exit $rc' EXIT
trap 'exit 130' INT TERM

# --- Preflight: the window, reachability, the install.
VERSIONS=""
WINDOW="${WINDOW_START_H}:00-${WINDOW_END_H}:00 UTC"
if [ "$DRY" = 0 ]; then
  if [ "$ANY_TIME" = 1 ]; then
    WINDOW="any time (--any-time: the GPU window check is skipped)"
    echo "window check skipped (--any-time)" >&2
  else
    hour=$(date -u ${NOW:+-d "$NOW"} +%H); hour=$((10#$hour))
    if [ "$hour" -lt "$WINDOW_START_H" ] || [ "$hour" -ge "$WINDOW_END_H" ]; then
      echo "blocked: outside the GPU window ${WINDOW_START_H}:00-${WINDOW_END_H}:00 UTC" >&2; exit 3
    fi
  fi
  for h in "$PRIMARY" "$SECONDARY"; do
    on "$h" 'true' >/dev/null 2>&1 < /dev/null || { echo "blocked: $h unreachable over ssh" >&2; exit 3; }
    on "$h" 'ragon-admin node migrate --help' >/dev/null 2>&1 < /dev/null \
      || { echo "blocked: $h has no accounts commands (install the build first)" >&2; exit 3; }
    VERSIONS="$VERSIONS$h: $(on "$h" 'ragon-admin --version' 2>&1 < /dev/null | head -n 1)
"
  done
  if [ -z "$EVIDENCE" ]; then EVIDENCE=$(mktemp -d); echo "evidence: $EVIDENCE (no --evidence given)"; fi
  mkdir -p "$EVIDENCE"
  # A stale pass must not carry over: the driven steps' files and every log are written afresh.
  rm -f "$EVIDENCE/E1.env" "$EVIDENCE/E8.env" "$EVIDENCE/summary.txt" "$EVIDENCE"/*.log
  # Stage the run folder and the secrets on both nodes; the password never leaves this shell.
  PASSWORD=$(od -An -tx1 -N18 /dev/urandom | tr -d ' \n')
  for h in "$PRIMARY" "$SECONDARY"; do
    on "$h" "test ! -e $RUN && mkdir -m 700 $RUN" < /dev/null || { echo "blocked: cannot stage $RUN on $h" >&2; exit 3; }
    STAGED="$STAGED $h"
    put "$h" auth_e2e_node.py < "$HELPER_SRC" && printf '%s\n' "$TEST_USER" | put "$h" user \
      && printf '%s\n' "$PASSWORD" | put "$h" pass || { echo "blocked: cannot stage $RUN on $h" >&2; exit 3; }
  done
  unset PASSWORD
  CREATED=1
fi

# --- The run: the primary.
step E0a "install: the build is on both nodes" "$PRIMARY" 'ragon-admin node user list'
step E0b "migrate: the primary question answered, the old tokens migrated" "$PRIMARY" 'ragon-admin node primary --self && ragon-admin node migrate'
step E0c "the secondary points at the primary" "$SECONDARY" "ragon-admin node primary $PRIMARY"
ON_TTY=1 step E0d "the test user is created and signs in (ragon login --password-stdin)" "$PRIMARY" \
  "{ cat $RUN/pass; cat $RUN/pass; } | ragon-admin node user add $TEST_USER && $H ca $RUN && $H signin $RUN"
driven E1 "first-run screen through a pty: sign in, and create an account with an empty password" "$PRIMARY" "$H pty $RUN"
ON_TTY=1 step E2 "login status shows the user and the issuer" "$PRIMARY" "$SESSION ragon whoami | grep -qF $TEST_USER"
step E3 "sessions lists this device" "$PRIMARY" "$SESSION ragon sessions"
step E5 "device auth in both shapes (user code; verification_uri_complete) completes; the tokens name the user" "$PRIMARY" "$H device $RUN"
step E6 "an API key works with the vendor's SDK: list, respond, stream, error; then revoked" "$PRIMARY" \
  "$H key $RUN && { $SDK; }; rc=\$?; $H revoke-key $RUN && exit \$rc"
evidence E7 "the desktop app's sign-in form"
driven E8 "ragon web over HTTP (WEAK: page script not executed): login page, login API, Account page and its API" "$PRIMARY" "$H web $RUN"
step E9 "Forgejo sign-in through the accounts service; Forgejo's API names the user" "$PRIMARY" "$H forgejo $RUN"
step E10 "a Night Shift task is queued as the user; its record names the owner (WEAK: queued in the run's queue, not run)" "$PRIMARY" "$H night $RUN"
evidence E11 "a phone paired through the Discord bridge"

# --- E14: a bad credential refused on both nodes (and no credential, per auth_enforce); the gates'
# refusal counts (WEAK: read from the gate logs, there is no refusal metric) rise by REFUSALS_MIN.
if [ "$DRY" = 1 ]; then
  record E14 plan "unauthenticated requests are refused (401) and counted on both nodes (on $PRIMARY and $SECONDARY: $H ca $RUN && $H unauth $RUN; the refusal counts rise by >= $REFUSALS_MIN)"
else
  rise=0; e14=pass
  for h in "$PRIMARY" "$SECONDARY"; do
    run "E14-$h" "$h" "$H ca $RUN && $H unauth $RUN"
    [ "$RC" = 0 ] || e14=FAIL
    n=$(printf '%s\n' "$OUT" | sed -n 's/^refusals_rise=\([0-9][0-9]*\)$/\1/p' | tail -n 1)
    rise=$((rise + ${n:-0}))
  done
  [ "$rise" -ge "$REFUSALS_MIN" ] || e14=FAIL
  record E14 "$e14" "unauthenticated requests are refused (401) and counted on both nodes (refusals rose by $rise, at least $REFUSALS_MIN)"
fi

# --- The secondary.
ON_TTY=1 step E12 "a request through the secondary reaches the engine" "$SECONDARY" \
  "$H ca $RUN && $H signin $RUN && $SESSION ragon exec --skip-git-repo-check \"say ok\""
step E13 "a revoked token is refused within ${REVOKE_REFUSAL_SECONDS_MAX} s on the secondary (WEAK: revoke and wait, the refusal is not checked)" "$SECONDARY" "ragon-admin node user key $TEST_USER --revoke e2e-sdk && sleep ${REVOKE_REFUSAL_SECONDS_MAX}"

# --- Last on the primary: logout ends this session, then logout --all ends every session.
ON_TTY=1 step E4 "logout ends this session; logout --all ends every session (the run session and a second password-grant session both get invalid_grant on refresh)" "$PRIMARY" \
  "$SESSION ragon logout && $H signin $RUN && $H logout $RUN"

SUMMARY="passed $PASS, failed $FAIL, planned $PLAN"
echo "$SUMMARY"
if [ "$DRY" = 0 ]; then
  printf 'at: %s\nwindow: %s\nprimary: %s\nsecondary: %s\n%s%s%s\n' "$(date -u +%FT%TZ)" "$WINDOW" "$PRIMARY" "$SECONDARY" \
    "$VERSIONS" "$RESULTS" "$SUMMARY" | mask > "$EVIDENCE/summary.txt"
fi
if [ "$DRY" = 1 ]; then echo "dry run: not a gate pass"; exit 0; fi
[ "$FAIL" -eq 0 ] || { echo "RED: hold the release"; exit 1; }
echo "GREEN"
