#!/bin/bash
# usage: run.sh <SIGKILL|SIGTERM> <GiB> ; samples once a second 8 s before and 40 s after the kill.
set -u
SIG=$1; GIB=$2; D=$(cd "$(dirname "$0")" && pwd)
[ -x /tmp/gpufill ] || swiftc -O "$D/fill.swift" -o /tmp/gpufill
sample() { # t free_pages(16K) compressed swapouts pressure_free_pct gpu_in_use_bytes
  local v; v=$(vm_stat)
  echo "$1 free=$(echo "$v"|awk '/Pages free/{gsub(/\./,"",$3);print $3}') comp=$(echo "$v"|awk '/occupied by compressor/{gsub(/\./,"",$5);print $5}') swapouts=$(echo "$v"|awk '/Swapouts/{gsub(/\./,"",$2);print $2}') sysfree=$(memory_pressure|awk '/free percentage/{print $5}') gpu=$(ioreg -r -d1 -c IOAccelerator|grep -o '"In use system memory"=[0-9]*'|head -1|cut -d= -f2)"
}
/tmp/gpufill "$GIB" > /tmp/gpufill.out 2>&1 & PID=$!
for i in $(seq 1 60); do grep -q ready /tmp/gpufill.out && break; sleep 0.5; done
cat /tmp/gpufill.out
for i in $(seq -8 -1); do sample $i; sleep 1; done
kill -$SIG $PID; T0=$(date +%s)
for i in $(seq 0 40); do
  alive=$(kill -0 $PID 2>/dev/null && echo alive || echo gone)
  sample "+$(( $(date +%s)-T0 )) $alive"; sleep 1
done
