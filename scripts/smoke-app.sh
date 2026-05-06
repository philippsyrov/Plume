#!/bin/bash
#
# scripts/smoke-app.sh — build and launch a real Plume.app bundle so
# accessibility tools and computer-use agents can target it.
#
# Why this script exists: a raw `tauri dev` produces
# `src-tauri/target/debug/plume`, which macOS LaunchServices does not
# register as an installed app. Visual agents (Apple Accessibility API,
# computer-use MCP, screen-sharing automation, etc.) cannot allowlist
# or address it. This script produces a real `Plume.app` bundle and
# launches it via `open` so macOS treats it like any installed app.
#
# Profile: debug. Release builds also produce an addressable .app but
# take far longer. The smoke harness is for "does the UI come up and
# behave," not for distribution.
#
# See docs/AGENT_OPERABILITY.md for the rule this enforces: Plume must
# be operable through the same surface humans drive.

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_ROOT"

if [ "$(uname)" != "Darwin" ]; then
  echo "smoke-app.sh: macOS only — Linux/Windows .app bundling is a separate path." >&2
  exit 1
fi

# Make rustup-installed Cargo discoverable even when this is invoked
# from a shell that didn't source ~/.cargo/env (matches verify.sh).
if [ -d "$HOME/.cargo/bin" ]; then
  case ":$PATH:" in
    *":$HOME/.cargo/bin:"*) ;;
    *) PATH="$HOME/.cargo/bin:$PATH" ;;
  esac
fi

if ! command -v cargo >/dev/null 2>&1; then
  echo "smoke-app.sh: cargo not found. Install Rust via https://rustup.rs" >&2
  exit 1
fi

if [ ! -d "node_modules" ]; then
  echo "smoke-app.sh: node_modules missing. Run './scripts/dev-env.sh npm install' first." >&2
  exit 1
fi

REQUIRED_ICONS=(
  "src-tauri/icons/icon.icns"
  "src-tauri/icons/32x32.png"
  "src-tauri/icons/128x128.png"
  "src-tauri/icons/128x128@2x.png"
)
for f in "${REQUIRED_ICONS[@]}"; do
  if [ ! -f "$f" ]; then
    echo "smoke-app.sh: missing required icon: $f" >&2
    echo "  Regenerate from icons/icon.png with:" >&2
    echo "    sips -z 1024 1024 src-tauri/icons/icon.png --out /tmp/source-1024.png" >&2
    echo "    ./scripts/dev-env.sh npx tauri icon /tmp/source-1024.png" >&2
    echo "  then prune iOS/Android/Square*/StoreLogo files." >&2
    exit 1
  fi
done

echo "smoke-app.sh: building Plume.app (debug profile, .app bundle only, offline)..."
# CARGO_NET_OFFLINE=true keeps the build from reaching the network.
# The smoke harness promises "no network/model downloads"
# (docs/AGENT_OPERABILITY.md § Smoke Harness); a cold project-local
# Cargo cache would otherwise silently fetch crates here. If cargo
# trips over a missing crate, it exits non-zero and the caller is told
# how to populate the cache.
export CARGO_NET_OFFLINE=true
if ! ./scripts/dev-env.sh bash -lc 'source "$HOME/.cargo/env" 2>/dev/null; npm run tauri -- build --debug --bundles app'; then
  cat >&2 <<EOF

smoke-app.sh: tauri build failed.

If the failure mentions "no matching package" or "registry index not
found" with --offline, the project-local Cargo cache is incomplete.
Populate it once (this is the only step that talks to the network) and
then re-run smoke-app.sh:

  ./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo fetch'
  ./scripts/smoke-app.sh

Any other failure is a real build error — read the cargo output above.
EOF
  exit 3
fi

APP="src-tauri/target/debug/bundle/macos/Plume.app"
if [ ! -d "$APP" ]; then
  echo "smoke-app.sh: build did not produce .app bundle at $APP" >&2
  exit 2
fi

ABS_APP="$PROJECT_ROOT/$APP"

# A previously-launched instance of *this exact bundle* would
# otherwise be re-activated by `open` instead of replaced with the
# freshly built one, and the user would silently test stale UI. Match
# against the absolute executable path so we don't reach into other
# Plume bundles a developer might have running (e.g. a release build,
# a different worktree's debug build).
EXEC="$ABS_APP/Contents/MacOS/plume"
if pgrep -f "$EXEC" >/dev/null 2>&1; then
  echo "smoke-app.sh: quitting previous instance of $EXEC..."
  pkill -f "$EXEC" || true
  for _ in 1 2 3 4 5 6 7 8; do
    pgrep -f "$EXEC" >/dev/null 2>&1 || break
    sleep 0.25
  done
  if pgrep -f "$EXEC" >/dev/null 2>&1; then
    cat >&2 <<EOF
smoke-app.sh: previous instance of
  $EXEC
did not exit within 2 s. Refusing to launch a second copy on top of
the stale one.

Quit it manually (Cmd-Q the window, or:
  pkill -9 -f "$EXEC"
) and rerun ./scripts/smoke-app.sh.
EOF
    exit 4
  fi
fi

echo "smoke-app.sh: launching $ABS_APP"
open "$ABS_APP"

cat <<EOF

Bundle:  $ABS_APP
Bundle id: dev.plume.app

Logs:
  GUI: open Console.app, search subsystem 'dev.plume.app'.
  CLI: PLUME_LOG=info "$ABS_APP/Contents/MacOS/plume"
       (runs the inner binary directly with stdout/stderr in your
       terminal; same code, same window, but bypasses LaunchServices'
       app-process model — use only when you need stdout in-shell.)

Stop:
  Cmd-Q the window, or: pkill -f "$EXEC"
EOF
