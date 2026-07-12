#!/bin/bash
#
# D131: select-and-run benchmark presets from the model catalog
# (benchmarks/catalog/). With no argument, lists the presets. With a
# preset id, builds the plume_bench sidecar (presets that measure the
# Plume path or use its generation posture need its verified
# handshake), binds the preset to this machine — the catalog's pinned
# artifact digest and quantization are re-verified against the live
# checkpoint — and runs the matrix.
#
# Records land in gitignored benchmark-artifacts/presets/<id>/ and are
# never a performance claim by themselves. No downloads, no installs;
# missing prerequisites refuse with a diagnostic (PLUME_MLX_PYTHON /
# PLUME_MODEL_DIR / PLUME_BENCH_BIN override discovery).
#
# Usage:
#   scripts/benchmark-preset.sh                 # list presets
#   scripts/benchmark-preset.sh <preset-id>     # run one

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_ROOT"

if [[ $# -gt 0 ]]; then
  echo "building plume_bench sidecar…" >&2
  ./scripts/dev-env.sh cargo build --manifest-path src-tauri/Cargo.toml --bin plume_bench
fi

exec npx --no-install vite-node scripts/benchmark/preset-cli.ts -- "$@"
