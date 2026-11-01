#!/usr/bin/env bash
# Installs Ragon on a GB10 from a checkout of this repository: the Python package (which provides
# `ragon-admin`), the workspace config, the host settings a model load needs, and the `ragon`
# terminal agent built from the pinned Codex fork. Everything is done by `ragon-admin`; this
# script only runs it in order. To install from a release without a checkout, use ../install.sh.
#
#   ./scripts/install_gb10.sh [MODEL]     MODEL defaults to the registry's default model
set -euo pipefail

cd "$(dirname "$0")/.."
git submodule update --init codex

echo "🔧 Installing Ragon's admin package (ragon-admin)..."
python3 -m pip install -e .

echo "⚙️  Writing the workspace configuration..."
if [ -n "${1:-}" ]; then
    ragon-admin init --model "$1"
else
    ragon-admin init
fi

echo "🛡️  Checking the host settings a model load needs (swap, sysctls, earlyoom, sysstat)..."
# Applies what `server start` would otherwise refuse over; sudo asks before anything changes.
ragon-admin host setup || echo "⚠️  Fix the above before 'ragon-admin server start'."

echo "🔨 Building ragon (the terminal agent) from the codex submodule..."
ragon-admin codex build

echo "🎉 Done. Start the model server with 'ragon-admin server start', then run 'ragon'."
