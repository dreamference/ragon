#!/usr/bin/env bash
# The release check for the AMD Strix Halo Linux node, run by the release operator on each RC. A
# hosted runner has no AMD GPU and the box is a shared dual-boot machine, so there is no
# self-hosted runner: this script goes to the box over ssh and prints ONE line.
#
#   scripts/amd_linux_release_check.sh --host SSH_HOST --workdir DIR [options]
#
# On the box it, in order:
#   1. takes the GPU lock (flock; fails if a lane or night holds it),
#   2. pulls the pinned ragon-serve-hip image (the QWEN38_RAGON_HIP_IMAGE digest in the registry),
#   3. runs the engine's ctest inside it, with the AMD device set,
#   4. runs the 64-token BF16 smoke (p32k prompt, --max-context 66560, --sessions 1, sparse head,
#      --spec, --ids-out) and compares its ids with scripts/amd_linux_release_check.ids,
#   5. runs the quick node checks that need no model: /dev/kfd, the video and render groups, the
#      gfx1151 KFD node, the GPU memory the installer's session fit reads, Docker.
# The output is `AMD_LINUX_RELEASE_CHECK PASS|FAIL ...` and the exit status is 0 only on PASS.
#
# Options:
#   --host H           ssh destination (required; never written into the repository)
#   --workdir DIR      on the box: DIR/models/{qwen3.8-27b-nvfp4,dflash2-scalesearch-2026-11-01,
#                      qwen3.8-27b-sparse-head} and DIR/data/p32k.jsonl
#   --image REF        image to check (default: the registry's pinned ragon-serve-hip digest)
#   --test-dir PATH    ctest directory inside the image (default /opt/ragon/build)
#   --ctest-exclude R  ctest -E regex, named in the output line (for a known, decided failure)
#   --bare DIR         before the image exists: use the engine build in DIR on the box instead of
#                      the image (ctest --test-dir DIR, DIR/ragon-run), with --rocm DIR/lib on
#                      LD_LIBRARY_PATH. The output line says bare=1.
#   --rocm DIR         ROCm runtime lib directory for --bare
#   --gpu-lock FILE    on the box (default $HOME/.ling-gpu.lock)
#   --expect FILE      expected ids file (default scripts/amd_linux_release_check.ids)
# Environment: AMD_LINUX_SSH names the ssh command (default ssh), AMD_LINUX_FS a root prefix for /dev
# and /sys and AMD_LINUX_GROUPS the group list, so the tests can stand in for the box.
set -uo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(dirname "$here")"

mode=local
host="" workdir="" image="" testdir=/opt/ragon/build exclude="" bare="" rocm="" lock='$HOME/.ling-gpu.lock'
expect="$here/amd_linux_release_check.ids"
while [ $# -gt 0 ]; do
  case "$1" in
    --remote) mode=remote ;;
    --host) host="$2"; shift ;;
    --workdir) workdir="$2"; shift ;;
    --image) image="$2"; shift ;;
    --test-dir) testdir="$2"; shift ;;
    --ctest-exclude) exclude="$2"; shift ;;
    --bare) bare="$2"; shift ;;
    --rocm) rocm="$2"; shift ;;
    --gpu-lock) lock="$2"; shift ;;
    --expect) expect="$2"; shift ;;
    --expect-ids) expect_ids="$2"; shift ;;
    *) echo "AMD_LINUX_RELEASE_CHECK FAIL unknown argument $1"; exit 2 ;;
  esac
  shift
done

if [ "$mode" = local ]; then
  [ -n "$host" ] && [ -n "$workdir" ] || { echo "AMD_LINUX_RELEASE_CHECK FAIL --host and --workdir are required"; exit 2; }
  if [ -z "$image" ] && [ -z "$bare" ]; then
    image="$(grep -hoE 'ghcr\.io/dreamference/ragon-serve-hip@sha256:[0-9a-f]{64}' \
      "$repo/dreamference/hardware/model_matrix_registry.py" | head -1)"
  fi
  if [ -z "$bare" ]; then
    case "$image" in
      "") echo "AMD_LINUX_RELEASE_CHECK FAIL no ragon-serve-hip image pinned in the registry"; exit 1 ;;
      *@sha256:0000000000000000000000000000000000000000000000000000000000000000)
        echo "AMD_LINUX_RELEASE_CHECK FAIL the registry still holds the placeholder digest (image not built)"; exit 1 ;;
    esac
  fi
  [ -r "$expect" ] || { echo "AMD_LINUX_RELEASE_CHECK FAIL no expected ids at $expect"; exit 2; }
  ids="$(tr -d '\r\n ' < "$expect")"
  ssh_cmd="${AMD_LINUX_SSH:-ssh}"
  q() { printf '%q ' "$@"; }
  args="$(q --remote --workdir "$workdir" --image "$image" --test-dir "$testdir" --ctest-exclude "$exclude" \
    --bare "$bare" --rocm "$rocm" --gpu-lock "$lock" --expect-ids "$ids")"
  # The script itself is the remote program: no file is copied to the box.
  $ssh_cmd "$host" "bash -s -- $args" < "${BASH_SOURCE[0]}"
  exit $?
fi

# ---- on the box ----
eval "lock=$lock"
fails=()
res_pull=skip res_ctest=skip res_smoke=skip res_quick=skip

exec 9>"$lock" || { echo "AMD_LINUX_RELEASE_CHECK FAIL cannot open the GPU lock"; exit 1; }
if ! flock -w "${AMD_LINUX_LOCK_WAIT:-60}" 9; then
  echo "AMD_LINUX_RELEASE_CHECK FAIL the GPU lock is held (a lane or a night has the box)"; exit 1
fi

amd_args=(--device /dev/kfd --device /dev/dri --group-add video --group-add render
  --security-opt seccomp=unconfined --ipc=host)
engine_env=(-e RAGON_NOLOG=1 -e RAGON_PROBE_SKIP=1)
# Inside the image the two directories are mounted at /models and /data; bare, they are read in place.
if [ -z "$bare" ]; then mp=/models dp=/data; else mp="$workdir/models" dp="$workdir/data"; fi
smoke_args=(--model "$mp/qwen3.8-27b-nvfp4" --max-context 66560 --sessions 1
  --draft "$mp/dflash2-scalesearch-2026-11-01" --sparse-head-dir "$mp/qwen3.8-27b-sparse-head"
  --spec --ids-out --prompts-file "$dp/p32k.jsonl" --max-tokens 64)

log="$(mktemp -d)"
trap 'rm -rf "$log"' EXIT

if [ -z "$bare" ]; then
  if docker pull -q "$image" > "$log/pull" 2>&1; then res_pull=ok; else res_pull=FAIL; fails+=(pull); fi
else
  res_pull=bare
fi

if [ "$res_pull" != FAIL ]; then
  ctest_x=()
  [ -n "$exclude" ] && ctest_x=(-E "$exclude")
  if [ -z "$bare" ]; then
    docker run --rm "${amd_args[@]}" "$image" ctest --test-dir "$testdir" --output-on-failure "${ctest_x[@]}" \
      > "$log/ctest" 2>&1
  else
    LD_LIBRARY_PATH="$rocm" ctest --test-dir "$bare" --output-on-failure "${ctest_x[@]}" > "$log/ctest" 2>&1
  fi
  ctest_rc=$?
  sumline="$(grep -E 'tests passed' "$log/ctest" | tail -1 | sed 's/^[0-9]*% *//; s/tests passed, */passed /; s/ tests failed out of /\/ /')"
  [ -n "$sumline" ] || sumline="no summary"
  if [ $ctest_rc -eq 0 ]; then res_ctest="ok($sumline${exclude:+, excluding $exclude})"; else res_ctest="FAIL($sumline)"; fails+=(ctest); fi

  if [ -z "$bare" ]; then
    docker run --rm "${amd_args[@]}" "${engine_env[@]}" -v "$workdir/models:/models:ro" -v "$workdir/data:/data:ro" \
      "$image" ragon-run "${smoke_args[@]}" > "$log/smoke.out" 2> "$log/smoke.err"
  else
    LD_LIBRARY_PATH="$rocm" RAGON_NOLOG=1 RAGON_PROBE_SKIP=1 "$bare/ragon-run" \
      "${smoke_args[@]}" > "$log/smoke.out" 2> "$log/smoke.err"
  fi
  smoke_rc=$?
  got="$(tr -d '\r\n ' < "$log/smoke.out")"
  if [ $smoke_rc -ne 0 ]; then
    res_smoke="FAIL(rc $smoke_rc)"; fails+=(smoke)
  elif [ "$got" != "$expect_ids" ]; then
    first=$(printf '%s\n' "$got" | tr ',' '\n' | paste -d' ' - <(printf '%s\n' "$expect_ids" | tr ',' '\n') | awk '$1!=$2{print NR; exit}')
    res_smoke="FAIL(ids differ at ${first:-length})"; fails+=(smoke)
  elif ! grep -q 'drafter head: sparse' "$log/smoke.err"; then
    res_smoke="FAIL(no sparse head)"; fails+=(smoke)
  else
    res_smoke="ok(64 ids)"
  fi
fi

# Quick node checks: no model, no container.
quick_bad=()
fs="${AMD_LINUX_FS:-}"
groups_now="${AMD_LINUX_GROUPS:-$(id -nG)}"
[ -e "$fs/dev/kfd" ] || quick_bad+=(kfd)
[ -e "$fs/dev/dri" ] || quick_bad+=(dri)
for g in video render; do printf '%s\n' "$groups_now" | tr ' ' '\n' | grep -qx "$g" || quick_bad+=("group-$g"); done
grep -qs 'gfx_target_version 110501' "$fs"/sys/class/kfd/kfd/topology/nodes/*/properties || quick_bad+=(gfx1151)
vram="$(cat "$fs"/sys/class/drm/card*/device/mem_info_vram_total 2>/dev/null | sort -n | tail -1)"
gtt="$(cat "$fs"/sys/class/drm/card*/device/mem_info_gtt_total 2>/dev/null | sort -n | tail -1)"
# One session at 196,608 needs about 42.4 GB usable (VRAM, plus GTT where the kernel line grants it).
usable=$(( ${vram:-0} + ${gtt:-0} ))
[ "${vram:-0}" -ge 42400000000 ] || [ "$usable" -ge 42400000000 ] || quick_bad+=(gpu-memory)
docker info > /dev/null 2>&1 || quick_bad+=(docker)
if [ ${#quick_bad[@]} -eq 0 ]; then res_quick="ok(vram $((${vram:-0} / 1000000000)) GB, gtt $((${gtt:-0} / 1000000000)) GB)"
else res_quick="FAIL(${quick_bad[*]})"; fails+=(quick); fi

verdict=PASS; [ ${#fails[@]} -gt 0 ] && verdict=FAIL
echo "AMD_LINUX_RELEASE_CHECK $verdict pull=$res_pull ctest=$res_ctest smoke=$res_smoke quick=$res_quick image=${image:-none} bare=$([ -n "$bare" ] && echo 1 || echo 0) kernel=$(uname -r)"
if [ "$verdict" = FAIL ]; then
  for f in ctest smoke; do [ -s "$log/$f" ] && tail -n 5 "$log/$f" >&2; done
  [ -s "$log/smoke.err" ] && tail -n 3 "$log/smoke.err" >&2
  exit 1
fi
exit 0
