#!/bin/bash
#
# D129A: one-command MLX benchmark smoke matrix — mechanics validation
# for the real mlx_lm.server adapter on the current machine. Runs a
# tiny warm+cold short-chat matrix against a locally present MLX
# checkpoint with verified identity (real model-dir digest, probed
# mlx-lm version) and prints the summarizer's tables.
#
# NOT a benchmark result: single dev machine, tiny repetition counts,
# records land in gitignored benchmark-artifacts/ and are never
# committed. No downloads, no installs; missing prerequisites refuse
# with a diagnostic (PLUME_MLX_PYTHON / PLUME_MODEL_DIR override
# discovery).
#
# Usage:
#   scripts/benchmark-mlx-smoke.sh

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_ROOT"

exec npx --no-install vite-node scripts/benchmark/mlx-smoke-cli.ts -- "$@"
