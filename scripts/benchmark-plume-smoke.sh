#!/bin/bash
#
# D129C: one-command PAIRED benchmark smoke — the same verified local
# checkpoint measured directly (rawRuntime) and through Plume's real
# orchestration modules (plumeOrchestration via the plume_bench
# sidecar), with shared pairIds so the summarizer derives Plume's
# orchestration overhead per pair.
#
# NOT a benchmark result: single dev machine, tiny repetition counts,
# records land in gitignored benchmark-artifacts/ and are never
# committed. No downloads, no installs; missing prerequisites refuse
# with a diagnostic (PLUME_MLX_PYTHON / PLUME_MODEL_DIR /
# PLUME_BENCH_BIN override discovery).
#
# Usage:
#   scripts/benchmark-plume-smoke.sh

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_ROOT"

echo "building plume_bench sidecar…" >&2
./scripts/dev-env.sh cargo build --manifest-path src-tauri/Cargo.toml --bin plume_bench

exec npx --no-install vite-node scripts/benchmark/plume-smoke-cli.ts -- "$@"
