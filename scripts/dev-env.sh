#!/bin/bash
#
# Run commands with Plume's dependency and model caches kept inside the project.
# This is not a security sandbox; it is a no-global-sprawl wrapper.

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

export NPM_CONFIG_CACHE="${NPM_CONFIG_CACHE:-$PROJECT_ROOT/.cache/npm}"
export npm_config_cache="${npm_config_cache:-$NPM_CONFIG_CACHE}"
export NPM_CONFIG_PREFIX="${NPM_CONFIG_PREFIX:-$PROJECT_ROOT/.local/npm-global}"
export npm_config_prefix="${npm_config_prefix:-$NPM_CONFIG_PREFIX}"

export COREPACK_HOME="${COREPACK_HOME:-$PROJECT_ROOT/.cache/corepack}"
export PNPM_HOME="${PNPM_HOME:-$PROJECT_ROOT/.local/pnpm}"
export YARN_CACHE_FOLDER="${YARN_CACHE_FOLDER:-$PROJECT_ROOT/.cache/yarn}"

export CARGO_HOME="${CARGO_HOME:-$PROJECT_ROOT/.cargo-home}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PROJECT_ROOT/src-tauri/target}"

export PIP_CACHE_DIR="${PIP_CACHE_DIR:-$PROJECT_ROOT/.cache/pip}"
export PIP_REQUIRE_VIRTUALENV="${PIP_REQUIRE_VIRTUALENV:-true}"

export HF_HOME="${HF_HOME:-$PROJECT_ROOT/.cache/huggingface}"
export XDG_CACHE_HOME="${XDG_CACHE_HOME:-$PROJECT_ROOT/.cache/xdg}"
export PLUME_MODEL_DIR="${PLUME_MODEL_DIR:-$PROJECT_ROOT/plume-models}"

mkdir -p \
  "$NPM_CONFIG_CACHE" \
  "$NPM_CONFIG_PREFIX" \
  "$COREPACK_HOME" \
  "$PNPM_HOME" \
  "$YARN_CACHE_FOLDER" \
  "$CARGO_HOME" \
  "$CARGO_TARGET_DIR" \
  "$PIP_CACHE_DIR" \
  "$HF_HOME" \
  "$XDG_CACHE_HOME" \
  "$PLUME_MODEL_DIR"

if [ -d "$PROJECT_ROOT/.venv/bin" ]; then
  export VIRTUAL_ENV="${VIRTUAL_ENV:-$PROJECT_ROOT/.venv}"
  export PATH="$VIRTUAL_ENV/bin:$PATH"
fi

if [ "$#" -eq 0 ]; then
  exec "${SHELL:-/bin/bash}"
fi

exec "$@"
