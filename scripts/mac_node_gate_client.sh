#!/usr/bin/env bash
# The Mac node's acceptance gate, client side: the twin of scripts/mac_node_gate.sh
# (specs/DREAMFERENCE_MAC_NODE.md §8). Run it on each client (Linux, macOS; on Windows from
# Git Bash) and on the GB10 that pairs; hand its output to the node script with
# `mac_node_gate.sh --client-results FILE`, which merges every client into one report.
#
# Output: one line per check, `G<n> TAB pass|fail|skip|dry TAB reason TAB os`.
# Checks: G2 G3 G7 G9 G10 G11 G12 G13 G14. Procedures that span machines or need a person
# (a disconnect mid-job, a phone) read `<evidence dir>/G<n>.env`, KEY=VALUE lines, keys at
# each check; a missing file is a failure. --dry-run runs nothing and reports `dry`.
#
# Exit: 0 no check failed; 1 a check failed; 2 usage.
set -u

G7_TURN_LIMIT_S=""   # G7 "within the limit set after M3": not set yet; the node script fails G7 until it is

RAGON="${RAGON:-ragon}"
RAGON_ADMIN="${RAGON_ADMIN:-ragon-admin}"
RAGON_SEARCH="${RAGON_SEARCH:-ragon-search}"
SSH="${SSH:-ssh}"
SSH_KEYGEN="${SSH_KEYGEN:-ssh-keygen}"
UNAME="${UNAME:-uname}"

node=""
dry_run=0
evidence_dir=""
job_user=""
checks="G2 G3 G7 G9 G10 G11 G12 G13 G14"

usage() {
  echo "usage: mac_node_gate_client.sh --node NAME [--dry-run] [--evidence DIR] [--job-user USER] [--checks 'G2 G3 ...']" >&2
  exit 2
}

while [ $# -gt 0 ]; do
  case "$1" in
    --node) node="${2:-}"; shift 2 || usage ;;
    --dry-run) dry_run=1; shift ;;
    --evidence) evidence_dir="${2:-}"; shift 2 || usage ;;
    --job-user) job_user="${2:-}"; shift 2 || usage ;;
    --checks) checks="${2:-}"; shift 2 || usage ;;
    -h|--help) usage ;;
    *) echo "mac_node_gate_client: unknown argument: $1" >&2; usage ;;
  esac
done
[ -n "$node" ] || usage

case "$($UNAME -s 2>/dev/null)" in
  Darwin) os=macos ;;
  Linux) os=linux ;;
  MINGW*|MSYS*|CYGWIN*) os=windows ;;
  *) os=unknown ;;
esac

work="$(mktemp -d "${TMPDIR:-/tmp}/mac-node-gate-client.XXXXXX")" || exit 2
trap 'rm -rf "$work"' EXIT
failed=0

line() {
  [ "$2" = fail ] && failed=1
  printf '%s\t%s\t%s\t%s\n' "$1" "$2" "$(printf '%s' "$3" | tr '\t\n' '  ')" "$os"
}

ev() {
  [ -n "$evidence_dir" ] && [ -f "$evidence_dir/$1.env" ] || return 0
  awk -v k="$2" 'index($0, k "=") == 1 { print substr($0, length(k) + 2); exit }' "$evidence_dir/$1.env"
}

# flags CHECK KEY=WANT...: every evidence key must equal its wanted value.
flags() {
  local check="$1" bad="" good="" kv k want v; shift
  if [ -z "$evidence_dir" ] || [ ! -f "$evidence_dir/$check.env" ]; then
    line "$check" fail "no evidence file ${evidence_dir:-<--evidence not given>}/$check.env"; return
  fi
  for kv in "$@"; do
    k="${kv%%=*}"; want="${kv#*=}"; v="$(ev "$check" "$k")"
    if [ "$v" = "$want" ]; then good="$good $k=$v"; else bad="$bad $k=${v:-missing}!=$want"; fi
  done
  if [ -n "$bad" ]; then line "$check" fail "$bad"; else line "$check" pass "$good"; fi
}

check_G2() {
  # No configuration: the client's own browse finds the node over mDNS.
  if [ "$dry_run" -eq 1 ]; then line G2 dry "would run: $RAGON node list --json, expect $node"; return; fi
  if "$RAGON" node list --json 2>/dev/null | grep -q "\"$node\""; then
    line G2 pass "found $node over mDNS"
  else
    line G2 fail "$node not in ragon node list --json"
  fi
}

check_G3() {
  # Pair, then a key the node never authorised must be refused.
  local target="${job_user:+$job_user@}$node"
  if [ "$dry_run" -eq 1 ]; then
    line G3 dry "would run: $RAGON_ADMIN node add $node; ssh with a fresh unauthorised key to $target must be refused"
    return
  fi
  if ! "$RAGON_ADMIN" node add "$node" > "$work/G3.log" 2>&1; then
    line G3 fail "node add failed: $(tail -n 1 "$work/G3.log")"; return
  fi
  "$SSH_KEYGEN" -q -t ed25519 -N "" -f "$work/wrong_key" >/dev/null 2>&1 || { line G3 fail "ssh-keygen failed"; return; }
  if "$SSH" -o BatchMode=yes -o IdentitiesOnly=yes -o StrictHostKeyChecking=accept-new \
      -i "$work/wrong_key" "$target" true >/dev/null 2>&1; then
    line G3 fail "a wrong key was accepted by $target"
  else
    line G3 pass "paired; a wrong key was refused"
  fi
}

check_G7() {
  # A Work session from this client, a long turn with tools. Keys: completed (1/0), turn_seconds.
  if [ "$dry_run" -eq 1 ]; then line G7 dry "would read ${evidence_dir:-<evidence dir>}/G7.env (completed, turn_seconds)"; return; fi
  local t; t="$(ev G7 turn_seconds)"
  if [ "$(ev G7 completed)" != 1 ]; then line G7 fail "the turn did not complete"; return; fi
  if [ -n "$G7_TURN_LIMIT_S" ] && ! awk -v a="${t:-x}" -v b="$G7_TURN_LIMIT_S" 'BEGIN { exit !(a + 0 <= b + 0 && a != "x") }'; then
    line G7 fail "turn took ${t:-?}s, limit ${G7_TURN_LIMIT_S}s"; return
  fi
  line G7 pass "completed in ${t:-?}s"
}

check_G9() {
  # Job over the forced command: run, status, fetch, cancel, a disconnect mid-run.
  if [ "$dry_run" -eq 1 ]; then
    line G9 dry "would run: $RAGON_ADMIN node run|jobs|fetch|cancel against $node; read G9.env"
    return
  fi
  flags G9 survived_disconnect=1 memory_cap_tripped=1 time_limit_tripped=1 shell_refused=1 gpu_denied=1
}

check_G10() {
  if [ "$dry_run" -eq 1 ]; then line G10 dry "would run: $RAGON_ADMIN node sync-model to $node from a GB10 and from a Mac; read G10.env"; return; fi
  flags G10 hash_equal_from_gb10=1 hash_equal_from_mac=1 served_after_restart=1 over_cable=1
}

check_G11() {
  if [ "$dry_run" -eq 1 ]; then line G11 dry "would read G11.env after /night add --on $node (three attempts, chooser, report)"; return; fi
  flags G11 commit_in_worktree=1 attempts=3 report=1 merged=0 pushed=0
}

check_G12() {
  if [ "$dry_run" -eq 1 ]; then line G12 dry "would run: $RAGON_SEARCH -n 1 ragon; voice and image search from G12.env"; return; fi
  if ! "$RAGON_SEARCH" -n 1 ragon > "$work/G12.log" 2>&1 || [ ! -s "$work/G12.log" ]; then
    line G12 fail "ragon-search gave no answer"; return
  fi
  flags G12 voice_ok=1 images_ok=1
}

check_G13() {
  if [ "$dry_run" -eq 1 ]; then line G13 dry "would read G13.env (push over SSH, /forge/ through ragon web, _ragon-forge._tcp seen, backup)"; return; fi
  flags G13 push_ok=1 forge_web_ok=1 forge_mdns_seen=1 backup_ok=1
}

check_G14() {
  if [ "$dry_run" -eq 1 ]; then line G14 dry "would read G14.env (ragon web --lan from a phone and the desktop app, policy conformance)"; return; fi
  flags G14 phone_conformance=1 app_conformance=1
}

for c in $checks; do
  case "$c" in
    G2|G3|G7|G9|G10|G11|G12|G13|G14) "check_$c" ;;
    *) echo "mac_node_gate_client: $c is not a client check" >&2; exit 2 ;;
  esac
done
exit "$failed"
