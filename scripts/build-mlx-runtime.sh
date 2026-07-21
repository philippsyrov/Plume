#!/bin/bash

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOCK="$PROJECT_ROOT/scripts/mlx-runtime-requirements.lock"
GENERATED="$PROJECT_ROOT/src-tauri/runtime/generated"
OUTPUT="$GENERATED/mlx-runtime"
BUILD_ROOT="$GENERATED/.mlx-runtime-build"
PYTHON_INSTALLS="$BUILD_ROOT/python"
EXPECTED_UV_VERSION="0.11.18"
EXPECTED_PYTHON_BUILD="20260510"

export UV_PYTHON_CPYTHON_BUILD="$EXPECTED_PYTHON_BUILD"
export PYTHONDONTWRITEBYTECODE=1

if [ "$(uname -s)" != "Darwin" ] || [ "$(uname -m)" != "arm64" ]; then
  echo "build-mlx-runtime.sh: the bundled MLX runtime requires Apple Silicon macOS" >&2
  exit 1
fi
if ! command -v uv >/dev/null 2>&1; then
  echo "build-mlx-runtime.sh: uv is required to build the pinned runtime" >&2
  exit 1
fi
UV_VERSION="$(uv --version | awk '{print $2}')"
if [ "$UV_VERSION" != "$EXPECTED_UV_VERSION" ]; then
  echo "build-mlx-runtime.sh: uv $EXPECTED_UV_VERSION is required, found $UV_VERSION" >&2
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
PYTHON_BUILD="$(tr -d '\r\n' < "$OUTPUT/BUILD")"
if [ "$PYTHON_BUILD" != "$EXPECTED_PYTHON_BUILD" ]; then
  echo "build-mlx-runtime.sh: CPython build $PYTHON_BUILD does not match $EXPECTED_PYTHON_BUILD" >&2
  exit 1
fi

# python-build-standalone supplies `python3` as a symlink, while the Rust
# release resolver deliberately refuses symlinked executables. Preserve the
# canonical python3.12 binary and stage a byte-identical regular file.
rm -f "$OUTPUT/bin/python3"
cp "$OUTPUT/bin/python3.12" "$OUTPUT/bin/python3"
chmod 755 "$OUTPUT/bin/python3"
if [ ! -f "$RUNTIME_PYTHON" ] || [ ! -x "$RUNTIME_PYTHON" ] || [ -L "$RUNTIME_PYTHON" ]; then
  echo "build-mlx-runtime.sh: staged bin/python3 must be a regular executable" >&2
  exit 1
fi

uv pip sync \
  --python "$RUNTIME_PYTHON" \
  --require-hashes \
  --break-system-packages \
  "$LOCK"

# MLX-VLM 0.5.0's continuous batcher stalls Qwen2-VL image requests. Stage
# Plume's narrow launcher beside the pinned packages so only the fixed vision
# catalog path uses upstream's existing direct stream_generate fallback.
WRAPPER_SOURCE="$PROJECT_ROOT/src-tauri/runtime/plume_mlx_vlm_server.py"
WRAPPER_OUTPUT="$OUTPUT/lib/python3.12/site-packages/plume_mlx_vlm_server.py"
POLICY_SOURCE="$PROJECT_ROOT/src-tauri/runtime/plume_mlx_vlm_policy.py"
POLICY_OUTPUT="$OUTPUT/lib/python3.12/site-packages/plume_mlx_vlm_policy.py"
cp "$WRAPPER_SOURCE" "$WRAPPER_OUTPUT"
cp "$POLICY_SOURCE" "$POLICY_OUTPUT"
chmod 644 "$WRAPPER_OUTPUT"
chmod 644 "$POLICY_OUTPUT"

# Plume launches `python3 -m mlx_lm`; generated console wrappers are unused and
# embed the build worktree in their shebangs. Keep only the two real Python
# executables in bin.
find "$OUTPUT/bin" -mindepth 1 -maxdepth 1 \
  ! -name python3 ! -name python3.12 -exec rm -rf {} +

# python-build-standalone's sysconfig data captures its installation prefix.
# Replace that build-only prefix with a marker, then resolve the marker from
# the final resource location whenever sysconfig is imported.
SYSCONFIG_DATA="$OUTPUT/lib/python3.12/_sysconfigdata__darwin_darwin.py"
export PLUME_RUNTIME_BUILD_PREFIX="$PYTHON_ROOT"
"$RUNTIME_PYTHON" - "$SYSCONFIG_DATA" <<'PY'
import os
from pathlib import Path
import sys

path = Path(sys.argv[1])
source = path.read_text(encoding="utf-8")
build_prefix = os.environ["PLUME_RUNTIME_BUILD_PREFIX"]
if build_prefix not in source:
    raise SystemExit("sysconfig data did not contain the expected build prefix")
source = source.replace(build_prefix, "@PLUME_RUNTIME_PREFIX@")
source += """

# Plume packaging: resolve the relocatable runtime prefix at import time.
import os as _plume_os
_plume_runtime_prefix = _plume_os.path.realpath(
    _plume_os.path.join(_plume_os.path.dirname(__file__), "..", "..")
)
build_time_vars = {
    key: value.replace("@PLUME_RUNTIME_PREFIX@", _plume_runtime_prefix)
    if isinstance(value, str) else value
    for key, value in build_time_vars.items()
}
"""
path.write_text(source, encoding="utf-8")
PY

# The standalone libpython install name also captures the temporary prefix.
# Give it a loader-relative identity and restore a valid ad-hoc signature after
# editing the Mach-O load command.
LIBPYTHON="$OUTPUT/lib/libpython3.12.dylib"
install_name_tool -id "@rpath/libpython3.12.dylib" "$LIBPYTHON"
codesign --force --sign - "$LIBPYTHON"

# Tests and bytecode are not runtime inputs and only inflate the application.
find "$OUTPUT" -type d \( -name __pycache__ -o -name test -o -name tests \) \
  -prune -exec rm -rf {} +
find "$OUTPUT" -type f \( -name '*.pyc' -o -name '*.pyo' \) -delete

LOCK_SHA256="$(shasum -a 256 "$LOCK" | awk '{print $1}')"
PYTHON_SHA256="$(shasum -a 256 "$RUNTIME_PYTHON" | awk '{print $1}')"
export PLUME_RUNTIME_LOCK_SHA256="$LOCK_SHA256"
export PLUME_RUNTIME_PYTHON_SHA256="$PYTHON_SHA256"
export PLUME_RUNTIME_PYTHON_BUILD="$PYTHON_BUILD"
export PLUME_RUNTIME_UV_VERSION="$UV_VERSION"
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
    "pythonBuild": os.environ["PLUME_RUNTIME_PYTHON_BUILD"],
    "uvVersion": os.environ["PLUME_RUNTIME_UV_VERSION"],
    "pythonExecutableSha256": os.environ["PLUME_RUNTIME_PYTHON_SHA256"],
    "packages": {
        "mlx": importlib.metadata.version("mlx"),
        "mlx-lm": importlib.metadata.version("mlx-lm"),
        "mlx-vlm": importlib.metadata.version("mlx-vlm"),
        "mlx-metal": importlib.metadata.version("mlx-metal"),
    },
    "requirementsLockSha256": os.environ["PLUME_RUNTIME_LOCK_SHA256"],
}
with open(identity_path, "w", encoding="utf-8") as output:
    json.dump(identity, output, indent=2, sort_keys=True)
    output.write("\n")
PY

"$RUNTIME_PYTHON" -c 'import importlib.metadata, mlx, mlx_lm, mlx_vlm.server; print(importlib.metadata.version("mlx"), importlib.metadata.version("mlx-lm"), importlib.metadata.version("mlx-vlm"))'

# Identity/import probes must leave the payload bytecode-free.
find "$OUTPUT" -type d -name __pycache__ -prune -exec rm -rf {} +
find "$OUTPUT" -type f \( -name '*.pyc' -o -name '*.pyo' \) -delete
if grep -R -a -F -l "$PROJECT_ROOT" "$OUTPUT" >/dev/null 2>&1; then
  echo "build-mlx-runtime.sh: generated runtime contains its build worktree path" >&2
  exit 1
fi
rm -rf "$BUILD_ROOT"
echo "build-mlx-runtime.sh: staged $OUTPUT"
