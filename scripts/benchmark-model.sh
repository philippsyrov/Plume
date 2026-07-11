#!/bin/bash
#
# D129: run ONE benchmark invocation for one exact model / runtime /
# configuration and append one bounded attempt record to a JSONL file.
# Reserved by docs/MODEL_BENCHMARKS.md § "Reserved D129 command
# shapes"; flags and record shape are documented in
# docs/BENCHMARK_HARNESS.md.
#
# This script downloads nothing, installs nothing, and talks to no
# network: the runtime command comes from the sanitized config file
# and the D129 harness ships only the local scripted fake runtime.
#
# Usage:
#   scripts/benchmark-model.sh --config <config.json> \
#     --fixture <fixture-dir> --out <records.jsonl> \
#     [--population warm|cold] [--repetition N] [--planned N] \
#     [--run-id ID] [--group-id ID] [--pair-id ID] [--timestamp RFC3339]

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_ROOT"

exec npx --no-install vite-node scripts/benchmark/run-model-cli.ts -- "$@"
