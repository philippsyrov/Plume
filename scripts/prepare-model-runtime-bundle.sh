#!/bin/bash

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GENERATED="$PROJECT_ROOT/src-tauri/runtime/generated"
LOCK="$PROJECT_ROOT/scripts/mlx-runtime-requirements.lock"
PYTHON="$GENERATED/mlx-runtime/bin/python3"
IDENTITY="$GENERATED/mlx-runtime/runtime-identity.json"
HELPER="$GENERATED/apple-model/plume-apple-model"
EXPECTED_UV_VERSION="0.11.18"
EXPECTED_PYTHON_BUILD="20260510"

export PYTHONDONTWRITEBYTECODE=1

"$PROJECT_ROOT/scripts/build-mlx-runtime.sh"
"$PROJECT_ROOT/scripts/build-apple-model-helper.sh"

if [ ! -f "$PYTHON" ] || [ ! -x "$PYTHON" ] || [ -L "$PYTHON" ] || [ ! -f "$IDENTITY" ]; then
  echo "prepare-model-runtime-bundle.sh: MLX runtime identity is incomplete" >&2
  exit 1
fi
PYTHON_BUILD="$(tr -d '\r\n' < "$GENERATED/mlx-runtime/BUILD")"
if [ "$PYTHON_BUILD" != "$EXPECTED_PYTHON_BUILD" ]; then
  echo "prepare-model-runtime-bundle.sh: CPython build stamp mismatch" >&2
  exit 1
fi
UV_VERSION="$(uv --version | awk '{print $2}')"
if [ "$UV_VERSION" != "$EXPECTED_UV_VERSION" ]; then
  echo "prepare-model-runtime-bundle.sh: uv version mismatch" >&2
  exit 1
fi
LOCK_SHA256="$(shasum -a 256 "$LOCK" | awk '{print $1}')"
PYTHON_SHA256="$(shasum -a 256 "$PYTHON" | awk '{print $1}')"
"$PYTHON" - "$IDENTITY" "$LOCK_SHA256" "$PYTHON_BUILD" "$UV_VERSION" "$PYTHON_SHA256" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as source:
    identity = json.load(source)
expected = {
    "mlx": "0.32.0",
    "mlx-lm": "0.31.3",
    "mlx-metal": "0.32.0",
}
if identity.get("packages") != expected:
    raise SystemExit(f"runtime package identity mismatch: {identity.get('packages')!r}")
if identity.get("requirementsLockSha256") != sys.argv[2]:
    raise SystemExit("runtime requirements lock digest mismatch")
if identity.get("pythonBuild") != sys.argv[3]:
    raise SystemExit("runtime Python build stamp mismatch")
if identity.get("uvVersion") != sys.argv[4]:
    raise SystemExit("runtime uv version mismatch")
if identity.get("pythonExecutableSha256") != sys.argv[5]:
    raise SystemExit("runtime Python executable digest mismatch")
PY

if ! file "$HELPER" | grep -Eq 'Mach-O 64-bit executable arm64'; then
  echo "prepare-model-runtime-bundle.sh: Apple helper identity check failed" >&2
  exit 1
fi

# Model checkpoints are user-triggered Application Support downloads. They
# must never drift into the generated application resources.
if find "$GENERATED" -type f \( \
  -iname '*.safetensors' -o \
  -iname '*.gguf' -o \
  -iname '*.ggml' -o \
  -iname 'pytorch_model*.bin' -o \
  -iname 'model*.bin' \
\) -print -quit | grep -q .; then
  echo "prepare-model-runtime-bundle.sh: model weights found in application resources" >&2
  exit 1
fi
if grep -R -a -F -l "$PROJECT_ROOT" "$GENERATED/mlx-runtime" >/dev/null 2>&1; then
  echo "prepare-model-runtime-bundle.sh: MLX runtime contains the build worktree path" >&2
  exit 1
fi
if find "$GENERATED" -type d -name __pycache__ -print -quit | grep -q . || \
  find "$GENERATED" -type f \( -name '*.pyc' -o -name '*.pyo' \) -print -quit | grep -q .; then
  echo "prepare-model-runtime-bundle.sh: generated resources contain Python bytecode caches" >&2
  exit 1
fi

echo "prepare-model-runtime-bundle.sh: runtime and helper resources are ready"
