# Manual Testing

How to drive a built Plume binary by hand and what to try once it's
open. The automated test surface (`./scripts/verify.sh`, `cargo
test`, `npm run build`) covers what code does; this doc covers what
you do as a human poking the app.

The existing harnesses are still the entry points — this doc points
at them and adds the convenience pieces that don't live in code:

- `./scripts/smoke-app.sh` — builds a real `Plume.app` bundle macOS
  LaunchServices addresses, launches it. See `docs/SMOKE_TESTING.md`
  for what it asserts.
- `./scripts/dev-env.sh npm run tauri dev` — raw dev binary with
  hot reload. Faster cycle, NOT addressable by LaunchServices /
  accessibility tools.
- `./scripts/install-dev-alias.sh` (D44, this doc's main addition)
  — drops a symlink on your Desktop so subsequent launches are
  one-click, no terminal required.

## TL;DR — first run on a Mac

```bash
# One-time:
./scripts/dev-env.sh npm install     # frontend deps; sets up dev-env
./scripts/smoke-app.sh               # builds Plume.app + launches it

# Optional:
./scripts/install-dev-alias.sh       # adds ~/Desktop/Plume (dev).app
```

After the first build the cached cargo + node artifacts persist
locally; subsequent `smoke-app.sh` runs are an incremental rebuild
(seconds to a minute on a warm cache).

The alias script (D44) is **opt-in**. It creates a symlink at
`~/Desktop/Plume (dev).app` that points back at the repo's debug
bundle. Removing the alias is one `rm` away. It does NOT:

- Move the app into `/Applications`.
- Install a launcher in `~/Library`.
- Register a URL scheme, system extension, login item, or service.
- Add anything to your shell rc files.

## Manual smoke checklist

Once the window is up, this is the checklist that exercises the
shipped surface end-to-end without any automated harness. Each
bullet is a thing you should expect to see; if any of them
deviates, that's a regression to file.

### Trust gate

1. Click **Open project** in the empty state.
2. Pick a folder. The pre-trust view shows project metadata
   (package managers, git branch) and a Trust button.
3. Click **Trust**. The header collapses to the compact status
   strip and the three-zone workspace renders.

A trusted project sticks across windows / restarts — the trust
file is in OS app-data (`~/Library/Application Support/dev.plume.app/`).

### File browser + inspector

1. Click a file in the left column. The right column renders it
   read-only in CodeMirror.
2. `.env` and other secret-pattern filenames refuse to open with
   an explicit message.
3. Files over the prompt-read cap render normally (the cap
   applies to attachment reads, not display reads).

### Chat

1. Pick a model in the Providers panel (Ollama must be running for
   the demo path).
2. Send a short prompt. Tokens stream in.
3. Click **Stop** mid-stream. The partial reply stays with a
   "(stopped)" marker.
4. Click **Attach current file** with a small text file selected
   in the inspector. Send again. The chip shows on the user turn.
5. Toggle `Propose diff` mode and ask for a small change. The
   reply renders as a per-line-coloured diff preview.

### Patch validate / apply / revert

1. After a propose-diff reply, click **Validate**. The pill flips
   to `Valid` or surfaces a specific reject reason.
2. Click **Apply** on a valid diff. The pill says `Applied · N
   files`; a **Revert** button appears.
3. Click **Revert**. The pill says `Reverted · N files` and the
   files return to their pre-apply state.

### Memory

1. Open the Memory panel from the left-column toggle strip.
2. Type a short note and click **Remember**.
3. Watch the entry land. Forget it. Confirm it's gone from the
   `.plume/memory/entries.jsonl` file under the project root.

### Memory search (D43)

1. With a few entries stored, type a substring of one of them in
   the search field. Results appear as you type (debounced).
2. Empty / whitespace queries collapse back to the full list.
3. Forget still works on a hit row.

### Memory in chat context (D42)

1. With at least one entry in memory, send any chat prompt.
2. The chat header shows a `✱ Memory · N entries · K B` chip.
3. The chip flips to `included` after the response lands.

### Local model details (D41)

1. Drop a model folder (or a `.gguf` file) into the configured
   model directory (default: `./plume-models/`).
2. The Local models panel lists it. Click the disclosure caret.
3. Architecture / max-context / quantization / weight counts
   render below.

### MLX server (D40, if you have `mlx-lm` installed)

This path is gated on `pip install mlx-lm`, which Plume **does
not** auto-run. Skip this if you don't already have it.

1. With an MLX folder in the inventory, you can drive the
   supervisor through a console (or future UI button) via
   `providers.startServer({providerId: 'mlx-lm', modelId: <id>})`.
2. The handle response carries `{id, port, pid}`.
3. `providers.stopServer({handleId})` returns `{ok: true}`; the
   child PID exits within ~3 seconds (SIGINT → SIGKILL fallback).

## Logs

GUI logs land in the macOS console under subsystem
`dev.plume.app`. To get logs in your terminal:

```bash
PLUME_LOG=info \
  ./src-tauri/target/debug/bundle/macos/Plume.app/Contents/MacOS/plume
```

This runs the inner binary directly. Same window, same code, but
bypasses LaunchServices' app-process model — use only when you
need stdout in-shell.

## Quitting

- Cmd-Q on the window.
- Or `pkill -f Plume.app` to nuke any stuck instance.

`smoke-app.sh` already detects a previous instance of the same
bundle and refuses to launch a second one on top of a stale build;
read its output if a launch seems to do nothing.

## Linux / Windows

`smoke-app.sh` is macOS-only because LaunchServices addressing is
macOS-specific. On Linux:

```bash
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo build'
./src-tauri/target/debug/plume
```

Windows is not currently a supported test platform.
