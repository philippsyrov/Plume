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
not** auto-run. Skip this if you don't already have it. Today
the supervisor only accepts `mlx-folder` and `transformer-folder`
inventory entries — single-file `.gguf` / `.safetensors` weights
aren't loadable by `mlx_lm.server`.

1. With a folder model in the inventory, click **Start** on its
   row in the Local models panel (D46). The row flips through
   `starting…` → `port N · Stop`; the model is auto-selected as
   the chat target.
2. The handle is also reachable directly: the IPC verb is
   `providers.startServer({providerId: 'mlx-lm', modelId: <id>})`,
   returning `{id, port, pid}`.
3. Click **Stop** (or call `providers.stopServer({handleId})`).
   The child PID exits within ~3 seconds (SIGINT → SIGKILL across
   the whole process group; Codex D40 fix).

### Gemma via Plume-managed MLX, end-to-end (D40 + D45 + D46) {#gemma-smoke}

This is the canonical happy-path smoke for a Plume-managed local
chat. It exercises the D40 process supervisor, D39 SSE parser,
D45 chat-routing, and D46 Start/Stop UI together. No Ollama,
no auto-install, no downloads from Plume — you bring the weights.

**Prereqs.** All three are operator responsibilities; Plume will
not perform any of them for you.

1. `mlx-lm` importable from the `python` interpreter on the
   `PATH` that Plume's launch shell sees. The supervisor runs
   `python -m mlx_lm server …`, so what matters is that
   `python -c "import mlx_lm"` exits cleanly in the same shell
   you launch Plume from. If `python` resolves but the import
   fails the Start button surfaces `spawn failed (is python
   installed? mlx_lm package installed?)`.

   **Important:** `pipx install mlx-lm` and `uv tool install
   mlx-lm` are *not* sufficient on their own. Both install
   `mlx-lm` into an isolated env and only expose CLI shims on
   PATH; `python` from your normal shell still cannot
   `import mlx_lm` because that env is not on
   `sys.path`. Pick one of the working setups instead:

   - **Project-local venv (recommended).** From a shell rooted
     in your Plume working dir:

     ```bash
     python -m venv .venv
     .venv/bin/pip install mlx-lm
     source .venv/bin/activate          # exports the venv's python to PATH
     ./scripts/dev-env.sh npm run tauri dev   # or scripts/smoke-app.sh
     ```

     Plume's child inherits the activated shell's PATH, so
     `python -m mlx_lm` resolves the venv's interpreter.

   - **uv-managed venv.** Same idea via `uv`:

     ```bash
     uv venv
     uv pip install mlx-lm
     source .venv/bin/activate
     # launch Plume from this shell
     ```

   - **Dedicated `mlx` venv anywhere.** Create the venv
     somewhere of your choice (`~/mlx-env`, a conda env, …) and
     either activate it before launching Plume, or set
     `PATH="$HOME/mlx-env/bin:$PATH"` so `python` resolves to
     that env.

   What does NOT work today: launching Plume from Finder /
   Spotlight when your venv is only active in the terminal —
   the GUI app inherits LaunchServices' bare PATH, not your
   shell's. Always launch from the activated shell for this
   path; the D44 dev-alias symlink works as long as you
   `open` the alias from the activated shell, not from Finder.
2. A Gemma MLX folder on disk that's already been quantized for
   MLX consumption. Plume does not download or quantize models.
   Public sources include the `mlx-community/*` repositories on
   HuggingFace (e.g. `mlx-community/gemma-3-4b-it-4bit`); use
   `huggingface-cli download` or `git lfs clone` to fetch one to
   your local model directory. Verify the folder contains a
   `config.json`, a `tokenizer.json` (or `tokenizer.model`), and
   at least one `*.safetensors` weight shard before continuing.
3. A trusted project open in Plume. The D40 supervisor gates
   `providers.startServer` on trust because spawning a Python
   subprocess is shell-command execution; without a trusted
   project the Start button surfaces "Trust the project to
   start a Plume-managed server."

**Where to put weights.** Plume scans `$PLUME_MODEL_DIR` if set,
otherwise `./plume-models` relative to the project root. Suggested
local layout:

```text
$PLUME_MODEL_DIR/
  mlx-community/
    gemma-3-4b-it-4bit/
      config.json
      tokenizer.json
      model.safetensors            # or model-00001-of-00002.safetensors, …
      model.safetensors.index.json # if sharded
```

The folder name appears in the inventory as the model id (the
`PathBuf::file_name` of the bottom-most directory; nested folders
keep the relative path). No symlinks: the scanner refuses them by
design, and following one would bypass the path-safety check.

**Smoke steps.**

1. **Open + trust the project.** With your model dir populated,
   open any project (the model dir does NOT need to be inside the
   project). Trust the project so the supervisor gate clears.
2. **Inventory check.** Open the Local models panel from the
   left-column chip strip. The Gemma row should appear with kind
   `MLX folder` and a Start button on the right. If it doesn't
   appear, check:
   - `PLUME_MODEL_DIR` env var is set OR the working directory
     contains a `plume-models/` link to the right path.
   - The folder has at least one `*.safetensors` weight shard.
     Empty or partial downloads are filtered out by the kind
     classifier.
3. **Start.** Click Start on the Gemma row. The row flips to
   `starting…`. Within ~15 seconds (load time depends on weight
   size; a 4-bit 4B model on Apple Silicon is typically 5-10 s)
   the row flips to `port N · Stop` and the model is now the
   chat target (a `selected` badge appears on the row).
4. **Verify the supervisor's child.** In a separate terminal,
   `lsof -iTCP -sTCP:LISTEN -n -P | grep 127.0.0.1:<port>`
   should show a `python` process bound to that port. Plume's
   SIGINT-then-SIGKILL on Stop will reap it.
5. **Send a chat.** Type a short prompt in the chat panel and
   Send. Tokens stream in via the D39 SSE parser. The footer
   shows `<n> tokens · <prompt> prompt-tokens` after `chat.done`;
   the tok/s field is `null` (MLX's OpenAI usage chunk doesn't
   carry per-phase durations — D45 deliberately doesn't
   fabricate a wall-clock fallback).
6. **Cancel mid-stream.** Send a longer prompt, then click Stop
   on the chat panel. The transcript keeps the partial reply with
   a `(stopped)` marker. The server stays running; subsequent
   sends still route to it.
7. **Stop the server.** Click Stop on the Local models row. The
   row flips through `stopping…` to idle within ~3 s. A follow-on
   chat send rejects with "no live MLX server with handleId
   …; call providers.startServer and pass the returned id." That
   typed `NotFound` is the right shape — the UI uses it to drive
   the "start the server again" flow.

**Troubleshooting.**

| Symptom                                       | Likely cause                                                                                                                                    |
| --------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| Start button never appears on the row         | The folder isn't classified as `mlx-folder` / `transformer-folder`. Check that it has a `config.json` AND at least one weight shard.            |
| Start → error: `spawn failed (is python…?)`  | `python` isn't on `PATH`, or `mlx_lm` isn't installed in the env Plume's `python` resolves to.                                                   |
| Start → error: `did not become ready in time` | Either weights are too big for the 30s health budget (large 70B models can take more), or another process won the port. The supervisor already retries once on port-race; if it fires twice, the model load is the bottleneck. |
| Chat → error: `model 'foo' not found at mlx-lm` | The request's `model` field doesn't match what `mlx_lm.server` has loaded. D45 echoes the supervisor's recorded `model_label` (the `--model` path passed at spawn) back on the wire so the loaded-vs-requested check passes. If this fires, the supervisor's registered `model_label` has drifted from what the server actually loaded — restart the server (`Stop` → `Start`) to re-sync. |
| Cancel doesn't stop tokens immediately        | Cooperative cancel polls the flag between SSE line reads (~200 ms). One more frame can still arrive after Stop — same contract as Ollama.        |
| Stop hangs in `stopping…`                     | The child process is unresponsive to SIGINT. The supervisor escalates to SIGKILL across the whole process group after a 3s grace; if it still hangs, `ps -ef | grep mlx_lm` and `kill -9` manually, then click Start to re-spawn. |

**Cleanup.** Stopping the server frees its port; Plume's process
registry drops the handle. If Plume crashes or is force-killed
while a child is running, the child is reparented to PID 1 and
keeps holding its port — `pkill -f mlx_lm.server` clears it. The
supervisor doesn't write PID files; nothing on disk needs cleanup.

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
