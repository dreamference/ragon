#!/usr/bin/env bash
# The Mac node's acceptance gate, node side: G1-G20 of specs/DREAMFERENCE_MAC_NODE.md §8.
#
# Run on the gate box (MN5) after a fresh `install.sh --role node` and an unattended restart.
# The client-side parts (G2, G3, G7, G9-G14) come from scripts/mac_node_gate_client.sh, run on
# each client and handed in with --client-results; this script merges them into ONE report.
#
# Stages (spec §9): `--stage preview` is S3's staging, the default: the node ships as a 1.0
# preview behind `--role node` with a stand-in engine once G1-G3, G9-G14 and G16-G18 pass; the
# checks that need the Metal engine (G4-G8, G15, G19, G20) are skipped with a reason.
# `--stage full` is the 1.0 cut: all twenty are required and a skip is a failure.
#
# Evidence: checks whose procedure runs elsewhere or for hours (the engine's gate runs, the
# soak, the SWE-bench night, a power cut) read `<evidence dir>/G<n>.env`, KEY=VALUE lines,
# keys listed at each check. A missing file fails a required check.
#
# --dry-run changes nothing and runs anywhere (Linux included): the macOS probes and every
# command that changes the machine are listed, not run; the engine's /v1/models (the stand-in)
# is still asked, and evidence files present are still evaluated. A dry run is never a gate pass.
#
# Exit: 0 every required check passed (or was planned, in a dry run); 1 a required check
# failed; 2 usage, or a real run off macOS.
#
# Portable to macOS's /bin/bash 3.2: no associative arrays, mapfile, timeout, date -d, python, jq.
set -u

# --- Thresholds. Spec §8: "they may tighten, never silently loosen" (pinned by tests/test_mac_node_gate.py).
GATE_MEMORY_GB=128                    # MN5: the 128 GB Mac Studio is the gate box
M3_HOST_GAP_MS_MAX=1                  # Metal spec §7 M3: host gap <= 1 ms per step
M3_DECODE_BYTE_MODEL_FRACTION_MIN=0.90 # Metal spec §7 M3: one-stream decode >= 90% of §5's byte model
M4_PREFILL_PROMPT_TOKENS_MIN=35000    # Metal spec §7 M4: a 35K-token prefill, rate recorded (no pass line)
L3_TOP1_FLIPS_MAX=85                  # Metal spec §2: Fisher threshold over 2,490 positions, 54 prompts
L3_FLIP_GAP_NATS_MAX=1.75             # Metal spec §2: every flip's gap
L3_TOP5_AGREEMENT_MIN=1.0             # Metal spec §2: top-5 agreement 100%
L3_MEAN_KL_NATS_MAX=0.0008            # Metal spec §2: mean KL(CUDA || Metal)
L3_PPL_RATIO_MIN=0.999                # Metal spec §2: perplexity ratio against the CUDA engine
L3_PPL_RATIO_MAX=1.001
M5_MEMORY_GROWTH_GB_MAX=1             # Metal spec §7 M5: memory growth over a 1-hour run
M7_SOAK_HOURS=8                       # Metal spec §7 M7 / G19: no restart in 8 hours
M7_THERMAL_MINUTES=60                 # Metal spec §7 M7 / G19: drift and thermal state over 60 minutes
G20_MCNEMAR_ALPHA=0.05                # G20/M7 "no significant loss": the SWE-bench report's 95% convention
# Limits the spec has not set yet. Empty: a real run fails the check until the spec sets them.
G1_HEALTH_LIMIT_S=""                  # G1 "health ok within the limit"
G7_TURN_LIMIT_S=""                    # G7 "within the limit set after M3"
G16_RESTART_LIMIT_S=""                # G16 "restart within the limit"

ENGINE_CHECKS="G4 G5 G6 G7 G8 G15 G19 G20"
CLIENT_CHECKS="G2 G3 G7 G9 G10 G11 G12 G13 G14"
ALL_CHECKS="G1 G2 G3 G4 G5 G6 G7 G8 G9 G10 G11 G12 G13 G14 G15 G16 G17 G18 G19 G20"
DAEMON_LABEL="ai.dreamference.ragon-node"

# Every external command is a variable, so a test can stand one in.
RAGON="${RAGON:-ragon}"
CURL="${CURL:-curl}"
UNAME="${UNAME:-uname}"
ENGINE_PID_CMD="${ENGINE_PID_CMD:-pgrep -x ragon-serve}"

here="$(cd "$(dirname "$0")" && pwd)"
CLIENT_TWIN="${CLIENT_TWIN:-$here/mac_node_gate_client.sh}"

stage="preview"
dry_run=0
engine_url="http://127.0.0.1:8000"
evidence_dir=""
candidate=""
report=""
client_results=""

usage() {
  cat >&2 <<'EOF'
usage: mac_node_gate.sh [--stage preview|full] [--dry-run] [--engine-url URL]
                        [--evidence DIR] [--candidate SHA] [--client-results FILE]...
                        [--report FILE]
EOF
  exit 2
}

while [ $# -gt 0 ]; do
  case "$1" in
    --stage) stage="${2:-}"; shift 2 || usage ;;
    --dry-run) dry_run=1; shift ;;
    --engine-url) engine_url="${2:-}"; shift 2 || usage ;;
    --evidence) evidence_dir="${2:-}"; shift 2 || usage ;;
    --candidate) candidate="${2:-}"; shift 2 || usage ;;
    --client-results) client_results="$client_results ${2:-}"; shift 2 || usage ;;
    --report) report="${2:-}"; shift 2 || usage ;;
    -h|--help) usage ;;
    *) echo "mac_node_gate: unknown argument: $1" >&2; usage ;;
  esac
done
case "$stage" in preview|full) ;; *) echo "mac_node_gate: --stage is preview or full" >&2; exit 2 ;; esac

os="$($UNAME -s 2>/dev/null || echo unknown)"
if [ "$dry_run" -eq 0 ] && [ "$os" != "Darwin" ]; then
  echo "mac_node_gate: the gate runs on the macOS gate box; use --dry-run elsewhere" >&2
  exit 2
fi
stamp="$(TZ=UTC date +%Y%m%dT%H%M%SZ)"
[ -n "$report" ] || report="mac-node-gate-$stamp.md"

work="$(mktemp -d "${TMPDIR:-/tmp}/mac-node-gate.XXXXXX")" || exit 2
trap 'rm -rf "$work"' EXIT
rows="$work/rows.tsv"
plan="$work/plan.txt"
: > "$rows"
: > "$plan"

# --- Helpers ---------------------------------------------------------------------------
clean() { printf '%s' "$*" | tr '\t\n' '  '; }
record() { printf '%s\t%s\t%s\n' "$1" "$2" "$(clean "$3")" >> "$rows"; }
planned() { printf '%s: %s\n' "$1" "$(clean "$2")" >> "$plan"; }
in_list() { case " $2 " in *" $1 "*) return 0 ;; esac; return 1; }
num_le() { awk -v a="$1" -v b="$2" 'BEGIN { exit !(a + 0 <= b + 0) }'; }
num_ge() { awk -v a="$1" -v b="$2" 'BEGIN { exit !(a + 0 >= b + 0) }'; }
is_num() { printf '%s' "$1" | grep -Eq '^-?[0-9]+(\.[0-9]+)?$'; }

# run CHECK CMD...: runs a command, or lists it in a dry run.
run() {
  local check="$1"; shift
  if [ "$dry_run" -eq 1 ]; then planned "$check" "$*"; return 0; fi
  "$@"
}

# ev CHECK KEY: a value from the check's evidence file ("" when absent).
ev() {
  [ -n "$evidence_dir" ] && [ -f "$evidence_dir/$1.env" ] || return 0
  awk -v k="$2" 'index($0, k "=") == 1 { print substr($0, length(k) + 2); exit }' "$evidence_dir/$1.env"
}

# need_evidence CHECK: 0 when the file is there; otherwise records dry or fail and returns 1.
need_evidence() {
  if [ -n "$evidence_dir" ] && [ -f "$evidence_dir/$1.env" ]; then return 0; fi
  if [ "$dry_run" -eq 1 ]; then
    record "$1" dry "would read ${evidence_dir:-<evidence dir>}/$1.env"
  else
    record "$1" fail "no evidence file ${evidence_dir:-<--evidence not given>}/$1.env"
  fi
  return 1
}

# expect CHECK KEY OP LIMIT: one evidence value against a threshold; appends to $bad on a miss.
expect() {
  local v; v="$(ev "$1" "$2")"
  if ! is_num "$v"; then bad="$bad $2=missing"; return; fi
  case "$3" in
    le) num_le "$v" "$4" || bad="$bad $2=$v>$4" ;;
    ge) num_ge "$v" "$4" || bad="$bad $2=$v<$4" ;;
    eq) [ "$v" = "$4" ] || bad="$bad $2=$v!=$4" ;;
  esac
  good="$good $2=$v"
}

# conclude CHECK: pass or fail from $bad, after a series of expect calls.
conclude() {
  if [ -n "$bad" ]; then record "$1" fail "out of bounds:$bad"; else record "$1" pass "within bounds:$good"; fi
}

models_ok() { "$CURL" -fsS --max-time 5 "$engine_url/v1/models" 2>/dev/null | grep -q '"id"'; }

# limit_unset CHECK VALUE: in a real run, an empty limit records a failure and returns 0.
limit_unset() {
  if [ "$dry_run" -eq 1 ] || [ -n "$2" ]; then return 1; fi
  record "$1" fail "limit not set in the spec; the check cannot pass until it is"
}
limit_note() { if [ -n "$1" ]; then printf 'limit %ss' "$1"; else printf 'limit not set in the spec'; fi; }

# --- Node-side checks -------------------------------------------------------------------
check_G1() {
  # Fresh install, restart with nobody logged in: daemon up, health within the limit, FileVault recorded.
  run G1 sysctl -n hw.optional.arm64
  run G1 sysctl -n hw.memsize
  run G1 launchctl print "system/$DAEMON_LABEL"
  run G1 fdesetup status
  run G1 stat -f %Su /dev/console
  if [ "$dry_run" -eq 1 ]; then
    if models_ok; then
      record G1 dry "the engine (stand-in) answers /v1/models at $engine_url; macOS probes listed, not run; health $(limit_note "$G1_HEALTH_LIMIT_S")"
    else
      record G1 fail "no answer from $engine_url/v1/models"
    fi
    return
  fi
  local bad="" mem arm console fv boot now
  arm="$(sysctl -n hw.optional.arm64 2>/dev/null)"
  [ "$arm" = "1" ] || bad="$bad not-apple-silicon"
  mem="$(sysctl -n hw.memsize 2>/dev/null)"
  num_ge "${mem:-0}" "$((GATE_MEMORY_GB * 1000 * 1000 * 1000))" || bad="$bad memory=${mem:-?}<${GATE_MEMORY_GB}GB"
  launchctl print "system/$DAEMON_LABEL" >/dev/null 2>&1 || bad="$bad daemon-not-loaded"
  console="$(stat -f %Su /dev/console 2>/dev/null)"
  [ "$console" = "root" ] || bad="$bad someone-logged-in"
  fv="$(fdesetup status 2>/dev/null | head -n 1)"
  limit_unset G1 "$G1_HEALTH_LIMIT_S" && return
  boot="$(sysctl -n kern.boottime | sed 's/.*sec = \([0-9]*\).*/\1/')"
  while ! models_ok; do
    now="$(date +%s)"
    if [ $((now - boot)) -gt "$G1_HEALTH_LIMIT_S" ]; then bad="$bad no-health-within-${G1_HEALTH_LIMIT_S}s"; break; fi
    sleep 5
  done
  if [ -n "$bad" ]; then record G1 fail "$bad; FileVault: $fv"; else record G1 pass "healthy $(( $(date +%s) - boot ))s after boot; FileVault: $fv"; fi
}

check_G4() {
  # Keys: host_gap_ms, decode_byte_model_fraction, spec_equals_plain (1/0), repeat_identical (1/0).
  need_evidence G4 || return
  local bad="" good=""
  expect G4 host_gap_ms le "$M3_HOST_GAP_MS_MAX"
  expect G4 decode_byte_model_fraction ge "$M3_DECODE_BYTE_MODEL_FRACTION_MIN"
  expect G4 spec_equals_plain eq 1
  expect G4 repeat_identical eq 1
  conclude G4
}

check_G5() {
  # Keys: prefill_prompt_tokens, prefill_tokens_per_s, gb10_prefill_tokens_per_s (both recorded, no pass line).
  need_evidence G5 || return
  local bad="" good=""
  expect G5 prefill_prompt_tokens ge "$M4_PREFILL_PROMPT_TOKENS_MIN"
  expect G5 prefill_tokens_per_s ge 0
  expect G5 gb10_prefill_tokens_per_s ge 0
  conclude G5
}

check_G6() {
  # The engine's L3 gate on the release candidate (spec §5: the node gate checks it was run).
  # Keys: candidate, top1_flips, max_flip_gap_nats, top5_agreement, mean_kl_nats,
  # kl_grows_with_position (1/0), ppl_ratio, acceptance_within_spread (1/0).
  need_evidence G6 || return
  local bad="" good="" c
  c="$(ev G6 candidate)"
  if [ -z "$candidate" ] || [ "$c" != "$candidate" ]; then bad="$bad candidate=${c:-missing}!=${candidate:-<--candidate not given>}"; fi
  expect G6 top1_flips le "$L3_TOP1_FLIPS_MAX"
  expect G6 max_flip_gap_nats le "$L3_FLIP_GAP_NATS_MAX"
  expect G6 top5_agreement ge "$L3_TOP5_AGREEMENT_MIN"
  expect G6 mean_kl_nats le "$L3_MEAN_KL_NATS_MAX"
  expect G6 kl_grows_with_position eq 0
  expect G6 ppl_ratio ge "$L3_PPL_RATIO_MIN"
  expect G6 ppl_ratio le "$L3_PPL_RATIO_MAX"
  expect G6 acceptance_within_spread eq 1
  conclude G6
}

check_G8() {
  # Keys: server_tests_pass (1/0): the engine's tests/server, vision and structured output, --batch-check at two.
  need_evidence G8 || return
  local bad="" good=""
  expect G8 server_tests_pass eq 1
  conclude G8
}

check_G15() {
  # Keys: engine_killed, desktop_responsive, others_killed (0), memory_returned, restart_ok, memory_growth_gb.
  need_evidence G15 || return
  local bad="" good=""
  expect G15 engine_killed eq 1
  expect G15 desktop_responsive eq 1
  expect G15 others_killed eq 0
  expect G15 memory_returned eq 1
  expect G15 restart_ok eq 1
  expect G15 memory_growth_gb le "$M5_MEMORY_GROWTH_GB_MAX"
  conclude G15
}

check_G16() {
  # Kill the engine at work (run here); the power cut is evidence. G16.env keys: power_cut_back (1/0), filevault.
  local pid
  run G16 "kill -9 \$($ENGINE_PID_CMD)  # then wait for /v1/models"
  if [ "$dry_run" -eq 1 ]; then
    record G16 dry "kill and restart listed, restart $(limit_note "$G16_RESTART_LIMIT_S"); power cut from ${evidence_dir:-<evidence dir>}/G16.env"
    return
  fi
  limit_unset G16 "$G16_RESTART_LIMIT_S" && return
  need_evidence G16 || return
  pid="$($ENGINE_PID_CMD | head -n 1)"
  if [ -z "$pid" ]; then record G16 fail "no engine pid from: $ENGINE_PID_CMD"; return; fi
  kill -9 "$pid"
  local start; start="$(date +%s)"
  sleep 2
  while ! models_ok; do
    if [ $(( $(date +%s) - start )) -gt "$G16_RESTART_LIMIT_S" ]; then record G16 fail "engine not back within ${G16_RESTART_LIMIT_S}s"; return; fi
    sleep 2
  done
  local bad="" good=""
  expect G16 power_cut_back eq 1
  good="$good restart=$(( $(date +%s) - start ))s filevault=$(ev G16 filevault)"
  conclude G16
}

check_G17() {
  run G17 "$RAGON" audit egress
  if [ "$dry_run" -eq 1 ]; then record G17 dry "ragon audit egress listed (exit 0 required; /airgapped on blocks mDNSResponder and host sockets)"; return; fi
  if "$RAGON" audit egress > "$work/G17.log" 2>&1; then
    record G17 pass "audit egress exit 0"
  else
    record G17 fail "audit egress exit $?: $(tail -n 1 "$work/G17.log")"
  fi
}

check_G18() {
  # `ragon update` over the running node, then the forced failure. G18.env keys: rolled_back (1/0).
  run G18 "$RAGON" update
  if [ "$dry_run" -eq 1 ]; then record G18 dry "ragon update listed; rollback from ${evidence_dir:-<evidence dir>}/G18.env"; return; fi
  if ! "$RAGON" update > "$work/G18.log" 2>&1; then record G18 fail "ragon update exit $?: $(tail -n 1 "$work/G18.log")"; return; fi
  models_ok || { record G18 fail "no /v1/models after the update"; return; }
  need_evidence G18 || return
  local bad="" good=""
  expect G18 rolled_back eq 1
  conclude G18
}

check_G19() {
  # Keys: soak_hours, restarts (0), thermal_minutes, step_drift_reported (1/0), thermal_state.
  need_evidence G19 || return
  local bad="" good=""
  expect G19 soak_hours ge "$M7_SOAK_HOURS"
  expect G19 restarts eq 0
  expect G19 thermal_minutes ge "$M7_THERMAL_MINUTES"
  expect G19 step_drift_reported eq 1
  conclude G19
}

check_G20() {
  # A SWE-bench night served by the Mac and run by a GB10. Keys: mcnemar_p, mac_resolved, gb10_resolved.
  need_evidence G20 || return
  local p mac gb10
  p="$(ev G20 mcnemar_p)"; mac="$(ev G20 mac_resolved)"; gb10="$(ev G20 gb10_resolved)"
  if ! is_num "$p" || ! is_num "$mac" || ! is_num "$gb10"; then record G20 fail "mcnemar_p, mac_resolved or gb10_resolved missing"; return; fi
  if num_ge "$mac" "$gb10" || num_ge "$p" "$G20_MCNEMAR_ALPHA"; then
    record G20 pass "mac $mac, gb10 $gb10, McNemar p=$p (alpha $G20_MCNEMAR_ALPHA)"
  else
    record G20 fail "significant loss: mac $mac < gb10 $gb10, McNemar p=$p < $G20_MCNEMAR_ALPHA"
  fi
}

# --- Client-side checks: merged from the twin's lines (G<n> TAB status TAB reason TAB os) ---
merge_client() {
  local check="$1" lines statuses oses
  lines="$(awk -F'\t' -v c="$check" '$1 == c' "$work/client.tsv")"
  if [ -z "$lines" ]; then record "$check" fail "no result from the client twin"; return; fi
  statuses="$(printf '%s\n' "$lines" | awk -F'\t' '{ print $2 }' | sort -u | tr '\n' ' ')"
  oses="$(printf '%s\n' "$lines" | awk -F'\t' '{ print $4 }' | sort -u | tr '\n' ' ')"
  local detail; detail="$(printf '%s\n' "$lines" | awk -F'\t' '{ printf "%s[%s] %s", sep, $4, $3; sep = "; " }')"
  if [ "$check" = "G7" ] && [ "$dry_run" -eq 0 ] && [ -z "$G7_TURN_LIMIT_S" ]; then
    record G7 fail "limit not set in the spec (G7, set after M3); $detail"; return
  fi
  case " $statuses" in
    *" fail "*) record "$check" fail "$detail"; return ;;
    *" skip "*) record "$check" fail "a client skipped: $detail"; return ;;
  esac
  if [ "$check" = "G2" ]; then
    local missing="" want
    for want in linux windows macos; do in_list "$want" "$oses" || missing="$missing $want"; done
    if [ -n "$missing" ]; then
      if [ "$dry_run" -eq 1 ]; then record G2 dry "clients missing:$missing; $detail"; else record G2 fail "clients missing:$missing; $detail"; fi
      return
    fi
  fi
  case " $statuses" in
    *" dry "*) record "$check" dry "$detail" ;;
    *) record "$check" pass "$detail" ;;
  esac
}

# --- Run ---------------------------------------------------------------------------------
: > "$work/client.tsv"
if [ -n "$client_results" ]; then
  for f in $client_results; do
    if [ -f "$f" ]; then grep -E '^G[0-9]+	' "$f" >> "$work/client.tsv"; else echo "mac_node_gate: no client results file $f" >&2; fi
  done
elif [ "$dry_run" -eq 1 ]; then
  # No clients handed in: run the twin here, dry, so the report still has every row.
  bash "$CLIENT_TWIN" --dry-run --node "${RAGON_GATE_NODE:-<node>}" >> "$work/client.tsv" 2>/dev/null \
    || echo "mac_node_gate: the client twin failed in its dry run" >&2
fi

for check in $ALL_CHECKS; do
  if [ "$stage" = "preview" ] && in_list "$check" "$ENGINE_CHECKS"; then
    record "$check" skip "S3 preview staging (spec §9): needs the Metal engine; the 1.0 cut (--stage full) waits for it"
    continue
  fi
  if in_list "$check" "$CLIENT_CHECKS"; then merge_client "$check"; continue; fi
  "check_$check"
done

# --- Report ------------------------------------------------------------------------------
failed=0
{
  echo "# Mac node gate: $stamp"
  echo
  if [ "$dry_run" -eq 1 ]; then echo "**DRY RUN: nothing was changed; this is not a gate result.**"; echo; fi
  echo "- stage: $stage$( [ "$stage" = preview ] && echo ' (S3: stand-in engine; G4-G8, G15, G19, G20 wait for the Metal engine)')"
  echo "- host: $os; engine: $engine_url; candidate: ${candidate:-not given}"
  echo "- evidence: ${evidence_dir:-none}; client results: ${client_results:-<twin run here, dry>}"
  echo
  echo "| # | Result | Detail |"
  echo "|---|---|---|"
  while IFS='	' read -r g status reason; do
    echo "| $g | $status | $reason |"
  done < "$rows"
  echo
  echo "## Thresholds"
  echo
  grep -E '^(GATE|M[0-9]|L3|G[0-9]+)_[A-Z0-9_]+=' "$0" | sed 's/^\([^ #]*\).*/- \1/'
  if [ -s "$plan" ]; then
    echo
    echo "## Planned commands (not run)"
    echo
    sed 's/^/- /' "$plan"
  fi
} > "$report"
while IFS='	' read -r g status reason; do
  required=1
  if [ "$stage" = "preview" ] && in_list "$g" "$ENGINE_CHECKS"; then required=0; fi
  if [ "$required" -eq 1 ]; then
    case "$status" in
      pass) ;;
      dry) [ "$dry_run" -eq 1 ] || failed=1 ;;
      *) failed=1 ;;
    esac
  fi
done < "$rows"
summary="$(awk -F'\t' '{ n[$2]++ } END { for (s in n) printf "%s %d  ", s, n[s] }' "$rows")"
if [ "$failed" -eq 0 ]; then verdict="PASS"; else verdict="FAIL"; fi
[ "$dry_run" -eq 1 ] && verdict="$verdict (dry run)"
printf '\n**%s**: %s\n' "$verdict" "$summary" >> "$report"
echo "mac_node_gate: $verdict: $summary"
echo "mac_node_gate: report $report"
exit "$failed"
