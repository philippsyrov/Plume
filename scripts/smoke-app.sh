#!/bin/bash
#
# scripts/smoke-app.sh — build and launch a real Plume.app bundle so
# accessibility tools and computer-use agents can target it.
#
# Why this script exists: a raw `tauri dev` produces
# `src-tauri/target/debug/plume`, which macOS LaunchServices does not
# register as an installed app. Visual agents (Apple Accessibility API,
# computer-use MCP, screen-sharing automation, etc.) cannot allowlist
# or address it. The contract for this script is: produce a real
# `Plume.app` bundle in a path the agent can `open` and re-target.
#
# See docs/AGENT_OPERABILITY.md for the rule this enforces: Plume must
# be operable through the same surface humans drive.
#
# Status: SKELETON. Producing a real bundle here requires either
# flipping `bundle.active` in tauri.conf.json (currently false) or
# wiring a separate dev-bundle profile, plus the icon assets Tauri
# expects in the documented sizes. None of those are done yet. This
# file marks the contract and the planned location; the real
# implementation lands in a follow-up slice and updates this comment
# when it ships.

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_ROOT"

cat <<'MSG' >&2
smoke-app.sh is a documented skeleton, not a working build script.

Building an addressable Plume.app currently needs:
  - tauri.conf.json bundle config (bundle.active is false today)
  - icon assets in the sizes Tauri expects
  - a debug-vs-release profile decision (slow release vs fast debug)

Until that lands, smoke testing means `npm run tauri dev` and a
human driving the window. See docs/AGENT_OPERABILITY.md.
MSG

exit 1
