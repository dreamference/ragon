#!/usr/bin/env bash
# Packages the installed ragon binaries as release assets for one target:
#
#   scripts/package_ragon.sh <target> <bin dir> <out dir>
#
# Writes <name>-<target>.gz for ragon, codex-code-mode-host, ragon-search, ragon-fetch and
# ragon-code, and for ragon-docs, ragon-signal and ragon-delegate where they were built (Linux only), and
# ragon-<target>.sha256sums
# covering them: the names and the checksum file that
# install.sh and `ragon update` (ragon-rs/src/update.rs) look for. Portable between Linux and
# macOS (sha256sum or shasum).
set -euo pipefail

target="${1:?target triple}"
bin="${2:?bin directory}"
out="${3:?output directory}"

mkdir -p "$out"
names=(ragon codex-code-mode-host ragon-search ragon-fetch ragon-code)
files=()
for name in "${names[@]}"; do
    [ -x "$bin/$name" ] || { echo "missing $bin/$name" >&2; exit 1; }
    gzip -9 -c "$bin/$name" > "$out/$name-$target.gz"
    files+=("$name-$target.gz")
done
# The local file index, the Signal bridge and the delegation plugin's server are built on Linux only; elsewhere the release
# simply does not carry them.
for name in ragon-docs ragon-signal ragon-delegate; do
    if [ -x "$bin/$name" ]; then
        gzip -9 -c "$bin/$name" > "$out/$name-$target.gz"
        files+=("$name-$target.gz")
    fi
done

if command -v sha256sum >/dev/null 2>&1; then
    (cd "$out" && sha256sum "${files[@]}" > "ragon-$target.sha256sums")
else
    (cd "$out" && shasum -a 256 "${files[@]}" > "ragon-$target.sha256sums")
fi
ls -l "$out"
