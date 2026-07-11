#!/bin/bash
#
# D129: coordinate deterministic fixture cases across warm/cold
# populations and repetitions, producing one sanitized JSONL
# collection via single-invocation benchmark-model runs. Reserved by
# docs/MODEL_BENCHMARKS.md § "Reserved D129 command shapes"; the plan
# format is documented in docs/BENCHMARK_HARNESS.md.
#
# Usage:
#   scripts/benchmark-suite.sh <plan.json>

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_ROOT"

exec npx --no-install vite-node scripts/benchmark/run-suite-cli.ts -- "$@"
