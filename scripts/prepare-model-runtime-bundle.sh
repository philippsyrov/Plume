#!/bin/bash

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GENERATED="$PROJECT_ROOT/src-tauri/runtime/generated"
LOCK="$PROJECT_ROOT/scripts/mlx-runtime-requirements.lock"
PYTHON="$GENERATED/mlx-runtime/bin/python3"
IDENTITY="$GENERATED/mlx-runtime/runtime-identity.json"
HELPER="$GENERATED/apple-model/plume-apple-model"

"$PROJECT_ROOT/scripts/build-mlx-runtime.sh"
"$PROJECT_ROOT/scripts/build-apple-model-helper.sh"

if [ ! -x "$PYTHON" ] || [ ! -f "$IDENTITY" ]; then
  echo "prepare-model-runtime-bundle.sh: MLX runtime identity is incomplete" >&2
  exit 1
fi
LOCK_SHA256="$(shasum -a 256 "$LOCK" | awk '{print $1}')"
"$PYTHON" - "$IDENTITY" "$LOCK_SHA256" <<'PY'
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

echo "prepare-model-runtime-bundle.sh: runtime and helper resources are ready"
