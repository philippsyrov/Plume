#!/bin/bash

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOCK="$PROJECT_ROOT/scripts/mlx-runtime-requirements.lock"
GENERATED="$PROJECT_ROOT/src-tauri/runtime/generated"
OUTPUT="$GENERATED/mlx-runtime"
BUILD_ROOT="$GENERATED/.mlx-runtime-build"
PYTHON_INSTALLS="$BUILD_ROOT/python"

if [ "$(uname -s)" != "Darwin" ] || [ "$(uname -m)" != "arm64" ]; then
  echo "build-mlx-runtime.sh: the bundled MLX runtime requires Apple Silicon macOS" >&2
  exit 1
fi
if ! command -v uv >/dev/null 2>&1; then
  echo "build-mlx-runtime.sh: uv is required to build the pinned runtime" >&2
  exit 1
fi
if [ ! -f "$LOCK" ]; then
  echo "build-mlx-runtime.sh: missing hashed lock: $LOCK" >&2
  exit 1
fi

rm -rf "$BUILD_ROOT" "$OUTPUT"
mkdir -p "$PYTHON_INSTALLS" "$OUTPUT"

# `--install-dir` is mandatory: uv's default managed-Python home is global.
uv python install 3.12.13 --install-dir "$PYTHON_INSTALLS" --no-bin
PYTHON="$(find "$PYTHON_INSTALLS" -path '*/bin/python3.12' -type f -perm +111 -print -quit)"
if [ -z "$PYTHON" ]; then
  echo "build-mlx-runtime.sh: uv did not produce a standalone Python 3.12" >&2
  exit 1
fi
PYTHON_ROOT="$(cd "$(dirname "$PYTHON")/.." && pwd)"
cp -R "$PYTHON_ROOT/." "$OUTPUT/"

RUNTIME_PYTHON="$OUTPUT/bin/python3"
if [ ! -x "$RUNTIME_PYTHON" ]; then
  echo "build-mlx-runtime.sh: staged runtime has no executable bin/python3" >&2
  exit 1
fi

uv pip sync \
  --python "$RUNTIME_PYTHON" \
  --require-hashes \
  --break-system-packages \
  "$LOCK"

# Tests and bytecode are not runtime inputs and only inflate the application.
find "$OUTPUT" -type d \( -name __pycache__ -o -name test -o -name tests \) \
  -prune -exec rm -rf {} +
find "$OUTPUT" -type f \( -name '*.pyc' -o -name '*.pyo' \) -delete

LOCK_SHA256="$(shasum -a 256 "$LOCK" | awk '{print $1}')"
export PLUME_RUNTIME_LOCK_SHA256="$LOCK_SHA256"
"$RUNTIME_PYTHON" - "$OUTPUT/runtime-identity.json" <<'PY'
import importlib.metadata
import json
import os
import platform
import sys

identity_path = sys.argv[1]
identity = {
    "schemaVersion": 1,
    "pythonVersion": platform.python_version(),
    "packages": {
        "mlx": importlib.metadata.version("mlx"),
        "mlx-lm": importlib.metadata.version("mlx-lm"),
        "mlx-metal": importlib.metadata.version("mlx-metal"),
    },
    "requirementsLockSha256": os.environ["PLUME_RUNTIME_LOCK_SHA256"],
}
with open(identity_path, "w", encoding="utf-8") as output:
    json.dump(identity, output, indent=2, sort_keys=True)
    output.write("\n")
PY

"$RUNTIME_PYTHON" -c 'import importlib.metadata, mlx, mlx_lm; print(importlib.metadata.version("mlx"), importlib.metadata.version("mlx-lm"))'
rm -rf "$BUILD_ROOT"
echo "build-mlx-runtime.sh: staged $OUTPUT"
