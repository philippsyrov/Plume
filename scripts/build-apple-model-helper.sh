#!/bin/bash

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PACKAGE="$PROJECT_ROOT/src-tauri/apple-model"
OUTPUT="$PROJECT_ROOT/src-tauri/runtime/generated/apple-model"

if [ "$(uname -s)" != "Darwin" ] || [ "$(uname -m)" != "arm64" ]; then
  echo "build-apple-model-helper.sh: the helper bundle requires Apple Silicon macOS" >&2
  exit 1
fi
if ! command -v swift >/dev/null 2>&1; then
  echo "build-apple-model-helper.sh: Swift is required" >&2
  exit 1
fi

swift build \
  --package-path "$PACKAGE" \
  --configuration release \
  --product plume-apple-model \
  --arch arm64
BIN_DIR="$(swift build \
  --package-path "$PACKAGE" \
  --configuration release \
  --show-bin-path \
  --arch arm64)"
BUILT_HELPER="$BIN_DIR/plume-apple-model"
if [ ! -x "$BUILT_HELPER" ]; then
  echo "build-apple-model-helper.sh: release helper was not produced" >&2
  exit 1
fi
if ! file "$BUILT_HELPER" | grep -Eq 'Mach-O 64-bit executable arm64'; then
  echo "build-apple-model-helper.sh: release helper is not a thin arm64 executable" >&2
  file "$BUILT_HELPER" >&2
  exit 1
fi

rm -rf "$OUTPUT"
mkdir -p "$OUTPUT"
cp "$BUILT_HELPER" "$OUTPUT/plume-apple-model"
chmod 755 "$OUTPUT/plume-apple-model"
echo "build-apple-model-helper.sh: staged $OUTPUT/plume-apple-model"
