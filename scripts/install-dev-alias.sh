#!/bin/bash
#
# scripts/install-dev-alias.sh — drop a Desktop symlink to the
# repo's Plume.app dev bundle so subsequent launches are one click,
# no terminal required.
#
# What it does (and ONLY what it does):
#   1. Verifies the .app bundle exists at the expected path.
#      If not, points the user at `./scripts/smoke-app.sh` and exits.
#   2. Creates a symlink at `~/Desktop/Plume (dev).app` → that bundle.
#   3. Prints what it did and how to undo.
#
# What it deliberately does NOT do (D44 brief, in order):
#   * `pip install`, `brew install`, `cargo install`, or anything else
#     that puts software on disk.
#   * Move the app into `/Applications`. Tracking a debug bundle from
#     /Applications would silently break when the repo path changes,
#     and overwriting /Applications/Plume.app on someone's machine
#     without explicit consent is hostile.
#   * Register a URL scheme, system service, login item, file
#     handler, or anything that survives `rm -rf` of the repo.
#   * Modify your shell rc files or PATH.
#
# Removal:
#   rm "$HOME/Desktop/Plume (dev).app"
#
# macOS only. On Linux, the bundle path differs and there's no
# Desktop-app convention to mirror; use the raw debug binary
# directly instead.

set -euo pipefail

if [ "$(uname)" != "Darwin" ]; then
  echo "install-dev-alias.sh: macOS only. On Linux, launch the raw" >&2
  echo "  debug binary at src-tauri/target/debug/plume." >&2
  exit 1
fi

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUNDLE_REL="src-tauri/target/debug/bundle/macos/Plume.app"
BUNDLE_ABS="$PROJECT_ROOT/$BUNDLE_REL"

if [ ! -d "$BUNDLE_ABS" ]; then
  cat >&2 <<EOF
install-dev-alias.sh: no Plume.app bundle at
  $BUNDLE_ABS

Run the build first:
  ./scripts/smoke-app.sh

That script produces the bundle this script links against. After
it has been built once, you can re-run install-dev-alias.sh.
EOF
  exit 2
fi

ALIAS="$HOME/Desktop/Plume (dev).app"

if [ -L "$ALIAS" ]; then
  CURRENT_TARGET="$(readlink "$ALIAS")"
  if [ "$CURRENT_TARGET" = "$BUNDLE_ABS" ]; then
    echo "install-dev-alias.sh: alias already exists and points to the right place:"
    echo "  $ALIAS -> $BUNDLE_ABS"
    exit 0
  fi
  echo "install-dev-alias.sh: replacing existing alias:" >&2
  echo "  $ALIAS -> $CURRENT_TARGET" >&2
  rm "$ALIAS"
elif [ -e "$ALIAS" ]; then
  # Something with that name exists but isn't a symlink — refuse to
  # touch it. The user can rename their own thing first.
  cat >&2 <<EOF
install-dev-alias.sh: $ALIAS already exists and is NOT a symlink.

Refusing to clobber it. Move or rename that file/folder first, then
re-run install-dev-alias.sh.
EOF
  exit 3
fi

ln -s "$BUNDLE_ABS" "$ALIAS"

cat <<EOF
install-dev-alias.sh: created Desktop alias.

  $ALIAS
    -> $BUNDLE_ABS

Double-click "Plume (dev)" on your Desktop to launch the build.

To remove the alias:
  rm "$ALIAS"

The link is a plain symlink — it disappears if you rebuild and the
bundle path is unchanged (which is the common case; the symlink
keeps working because it points at the bundle directory, not at a
specific build output).
EOF
