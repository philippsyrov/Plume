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

### No-project chat (D49)

A pre-project path. Lets the user chat against Ollama or an
already-running Plume-managed MLX server before committing to
a project.

1. From the empty / open-project screen, click **Chat without
   a project** (the secondary button below the Open form).
2. The window flips to a two-zone shell: Providers + Local
   models on the left, Chat on the right. No file navigator,
   no inspector, no Memory panel.
3. Pick an Ollama model. Send a short prompt. Tokens stream
   in. No `instructionsIncluded` badge, no Memory chip — both
   are honest "n/a in no-project mode".
4. Inspect a `mlx-folder` / `transformer-folder` row in the
   Local models panel. The Start button renders as disabled
   with tooltip "Open and trust a project to start
   Plume-managed runtimes." — the D40 trust gate stays intact.
5. If you previously started an MLX server in a trusted
   project this session, its row in no-project chat still
   shows `port N · Stop` and is selectable. Chat against it
   works; Stop also works (cleanup verb is not gated). The
   `useMlxServers` bus is App-scoped (D49 Codex MEDIUM fix),
   so jumping between trusted-project view and no-project
   chat preserves running handles in both directions.
6. Click **Open a project** in the top strip to return to the
   open form. The selection state drops, but running MLX
   servers KEEP running — only closing the window / quitting
   Plume fires the `useMlxServers` cleanup that stops them.
   If you want to surface them again, jump back into
   no-project chat (or open a project) and they reappear in
   the Local models panel.

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

### Agent autonomy settings (D84)

The Agent card sits in the left-column toggle strip (peer of Memory),
in a trusted project. It drives the `session.*` config the backend
already holds — no tool runs from here; it only declares intent.

1. Open the **Agent** panel. It loads the current config (default:
   mode `Chat`, approval `Ask each`, empty allowlists, no cap).
2. Change **Mode** to `Propose diff` — it applies immediately (one
   `session.setMode` round-trip) and the select sticks.
3. Change **Mode** to `Agent loop` with empty allowlists. It is
   **refused**: the select snaps back to the prior mode and a red list
   appears — "agent-loop requires a non-empty fileAllowlist", "…
   commandAllowlist", "… an iterationCap". This is the fail-closed rule;
   you cannot enter autonomy without gates.
4. Fill **File allowlist** (e.g. `src/`), **Command allowlist** (e.g.
   `cargo test` on its own line), and **Iteration cap** (e.g. `5`), click
   **Apply gates**. Now switching to `Agent loop` is accepted.
5. A non-numeric cap (e.g. `abc`) disables **Apply gates** with an inline
   "cap must be a number"; a blank cap means "no cap" and applies fine.
6. The config is per-project session state — it resets to the
   least-privilege default when you close and reopen the project.

### Chat sessions — persisted sidebar (D63) {#chat-sessions}

Chats are durable now: the sidebar's **Chats** section (local) and the
rows under a project are real sessions in SQLite — local ones under the
app-data directory, project ones under `<project>/.plume/sessions/` —
driven by the D63A `sessions.*` IPC. Quick tour:

1. Launch Plume without a project. Click **New chat** — a "New chat"
   row appears. Send one turn against a running local model (or let it
   error without one; errors persist too). Quit and relaunch: the most
   recently updated chat is selected and its transcript restored.
2. Open and trust a project. Create a project chat with the **+** on
   the project row. The new row appears under the project — never
   under **Chats** — and local rows never appear under the project.
   (D65) A chat still titled "New chat" titles itself from the first
   accepted user message: whitespace collapsed, capped at 60
   characters with a word-boundary `…` cut. Derived locally — no
   model, no network. Rejected/empty sends never title a chat, and a
   chat you renamed yourself is never overwritten — in this launch or
   any later one. To keep that promise across relaunches, the literal
   title "New chat" is reserved for untitled chats: the rename dialog
   refuses it with a visible message.
3. Row menu (**…**): **Rename** opens a Plume-styled dialog (no
   browser prompt; title trimmed, max 120 characters). **Archive**
   removes the row from the list and an **Archived chats** action
   appears at the bottom of the section — Unarchive restores the row
   at its historical position. **Delete** requires the explicit
   *Delete permanently* click; after relaunch the transcript is gone.
4. Start a streaming reply, then click another chat row or **New
   chat** mid-stream: the switch is refused with a visible notice and
   the stream keeps going — nothing is cancelled silently. After
   Stop (or completion) switching works again.
5. A local chat inside a project window stays a SIMPLE chat: no attach
   affordance, no AGENTS.md / memory badges, no project tool drawer —
   the same boundary as the no-project chat surface.
6. Persistence happens only at turn boundaries (the accepted user
   turn, then the terminal reply / stop / error) — never per token.
   If a save fails, a "Chat history could not be saved" banner
   appears and the next completed turn retries automatically.

### Agent event dry-run (D93)

Below the Agent settings card is an **Event stream dry-run** card. Click
**Run dry-run** — the transcript fills with a scripted sequence of typed
agent events (message → tool proposed → approval → started → finished →
failed → paused → done). This is a plumbing proof that the typed D85
event protocol drives the `AgentEventLog` surface; **nothing real runs**
(no model, no shell, no patch, no file writes).

> Note: the **tool catalog** (D86/D92, `tools.list` / `tools.search`) is
> a read-only IPC with no panel yet, and the dry-run above runs **no real
> tools**. Mutating tool *execution* (apply, run-command) is unimplemented
> and will land only behind an explicit approval / allowlist gate — see
> `docs/IPC_ROADMAP.md § Tools`. The local-first proof path is MLX / Qwen
> (the two smoke scripts below); Ollama is supported for compatibility but
> is not the happy path.

### Single-step agent (D96) — the first executing step

Above the dry-run card is a **Run one step** card. This one is real: it
drives the selected, running local MLX model for a single step.

Preconditions: open and trust a project, then in **Local models** start a
Qwen (MLX) server and select it (the same setup the chat smoke uses). In
the **Agent** card, set **Mode** to **Propose diff** (or higher) — the
mode axis gates what the model may do, so `chat` mode refuses a step. The
**Run step** button stays disabled with a one-line reason until all four
hold (Agent mode ≥ propose-diff · MLX model selected · server running ·
instruction typed); the backend rejects a chat-mode step with
`BadArgument` even if the button is bypassed.

1. Type a small, self-contained instruction, e.g.
   *"Change greet in greet.py to return an f-string: f\"Hello, {name}!\""*
   (include enough context — this step does not read files into the
   prompt yet).
2. Click **Run step**. The transcript fills with the **real** event
   stream: the model's reply, the read-only `patch.validate` Plume ran on
   the diff, and — if the diff is valid — the **apply** step held behind
   **needs approval** and a **paused** terminal.
3. **Nothing is written.** Applying a diff is a write, which always
   prompts under every approval policy, and single-step never auto-applies
   regardless. Validation is read-only.
4. If the model replies with the `TOOL_REQUEST: <tool>` sentinel instead
   of a diff, you'll see a **blocked** `toolFailed` event — only
   propose-diff is wired in this slice. A model/transport failure shows as
   a `ProviderDown` IPC error.

This is the seam where the catalog / approval / event scaffolds first
carry a real model turn. It is Apple-Silicon-only (it needs a running MLX
server); there is no Ollama path for `agent.singleStep`.

**Verified in-app (D97, 2026-06-27).** The full round-trip was confirmed
on Apple Silicon against a Plume-managed `Qwen2.5-Coder-3B-Instruct-4bit`
server. A *modify* instruction ("change the first heading in README.md")
produced a diff that rendered the whole happy-path stream in **Run one
step**: `messageChunk` → `patch.validate` (proposed / started / finished,
"diff is valid — 1 file, 1 hunk") → apply **proposed** →
**approvalRequired** ("applying writes files") → **paused**. A
*create-a-new-file* instruction exercised the blocked path — the model
replied `TOOL_REQUEST: create-file`, which surfaced as a blocked
`toolFailed`. Disk stayed untouched in both runs (no apply, no
checkpoint). The mode gate (`chat` refuses with a disabled button + reason,
`propose-diff` allows) was confirmed live.

**File context (D99).** The **Run one step** card has the same attach
control as the chat panel. Select a UTF-8 file (or a line range) in the
inspector, click **Attach current file** / **Attach selection**, and the
chip shows the pending attachment. Running the step folds that file
(redacted, optionally sliced to the range) into the propose-diff prompt so
the model edits real code, then clears the chip (one-shot). Attaching a
secret-named file (`.env`, `*.pem`, …) or one over the 256 KiB cap is
refused with an IPC error, exactly as `chat.send` refuses it — nothing is
read past the gate. No trusted project ⇒ the step is `NeedsApproval`
before any read.

**Apply / revert (D100) — patch-only mutation.** When the model's diff
validates, an **Apply diff** button appears below the event log (the run
itself wrote nothing — it only validated and paused). Click it: Plume runs
the diff through the existing `patch.apply` (re-validates server-side, takes
a checkpoint, writes atomically), the log gains an `applied — N file(s) ·
checkpoint <id>` frame, and the button flips to **Revert** (→ `patch.revert`,
which drift-detects and restores). An invalid diff offers no Apply; an apply
failure (e.g. pre-image drift) shows `apply failed (<reason>)` in the log and
leaves Apply available to retry. There is **no automatic apply** — only the
click writes — and **no shell execution**: this is patch-only mutation.

### Single-step patch flow — quick checklist (D124) {#single-step-patch-flow}

The condensed pass over the patch-only local-agent loop as it stands
after D123 (head status line, no-diff copy, revert-failed copy,
past-run banner). The D96/D99/D100 prose above explains each surface
in depth; run this checklist after touching the flow to confirm the
user-visible pieces still hold together. Use a **scratch project** —
step 6 writes a real file (and reverts it).

1. **Launch Plume.** `./scripts/smoke-app.sh` (or the D44 Desktop
   alias). Launch from a shell where `python -c "import mlx_lm"`
   exits cleanly, or set `PLUME_MLX_PYTHON` — see the
   [Gemma walkthrough's prereqs](#gemma-smoke).
2. **Open and trust a project.** Without trust every agent verb is
   `NeedsApproval` and nothing below is reachable.
3. **Start a Qwen MLX server.** In **Local models**, click **Start**
   on a Qwen coder folder (the same weights the
   [Qwen chat smoke](#qwen-mlx-smoke) uses) and wait for
   `port N · Stop`. Start auto-selects the model. In the **Agent**
   card set **Mode** to **Propose diff** — `chat` mode refuses a
   step.
4. **Attach a small file — or don't.** Open a small UTF-8 file in
   the inspector and click **Attach current file** (the chip is
   one-shot; it clears after the run). Running unattached also
   works — the model just edits blind, so put enough context in
   the instruction.
5. **Run one step.** Type a small self-contained instruction and
   click **Run step**. While the step is in flight the head shows
   `running…` in the status line and the button reads **Running…**.
6. **Confirm the six surfaces:**
   - **Event log** — real frames land: the model's reply chunk(s),
     `patch.validate` proposed → started → finished, then the apply
     step held at **needs approval** and a **paused** terminal.
     Nothing on disk has changed yet.
   - **Proposed change** — the card under the log shows the diff
     body plus a changed-files summary, and the head status line
     reads `diff ready`. The note next to Apply says a checkpoint
     is saved first so Revert can undo it.
   - **Apply** — click **Apply diff**. The log gains
     `applied — N file(s) · checkpoint <id>`, the note flips to
     "Applied — a checkpoint was saved first, so Revert can undo
     this.", the status line reads `applied`, and the file really
     changed on disk (check it in the inspector).
   - **Revert** — click **Revert**. The note flips to "Reverted —
     your files are back to the pre-apply state.", the status line
     reads `reverted`, and the file matches its pre-apply content
     again. (To see the failure copy instead, hand-edit the applied
     file before clicking Revert — drift detection rejects and the
     note says "Revert failed — the applied files were left as they
     are…" with Revert still clickable.)
   - **No-diff copy** — run another step whose reply can't yield a
     valid diff (ask a question, or request a new file so the model
     emits `TOOL_REQUEST`). The panel says "This run produced no
     applicable diff — there is nothing to apply." instead of
     rendering nothing, and the status line reads `no diff`.
   - **Past-run read-only boundary** — with two runs done, the
     **Recent runs** switcher appears. Select the older run: a
     "Viewing a past run (read-only)" banner with **Back to live
     run** renders above the log, the diff card is badged
     `read-only · past run`, and no Apply/Revert exists anywhere in
     the view. **Back to live run** returns to the interactive run.

Every write in this flow is an explicit click through the existing
`patch.apply` / `patch.revert` verbs — no shell execution, no
auto-apply, no new tool execution.

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

### MLX runtime smoke script (D53) {#mlx-runtime-smoke}

**Use this BEFORE the full Gemma walkthrough below.** When a Gemma
start in Plume fails, the most useful diagnostic is "does the model
folder + `mlx-lm` actually work outside Plume at all?" — and that's
what `scripts/smoke-mlx-runtime.sh` answers in ~30 seconds, without
touching the Plume UI:

```bash
./scripts/smoke-mlx-runtime.sh <absolute-path-to-model-folder>
```

The script:

1. Checks `python -c "import mlx_lm"` actually works in the current
   shell (the most common Gemma-debug confusion is a `pipx` / `uv
   tool` install that creates a CLI shim but no python-import path).
   If `mlx_lm` is missing, it prints the recommended venv playbook
   and exits non-zero — it never installs anything.
2. Verifies the model folder shape (`config.json` + a `tokenizer*`
   file + at least one `.safetensors` / `.gguf` / `.npz` weight)
   matches Plume's scanner classification floor.
3. Allocates an ephemeral port, spawns `python -m mlx_lm server
   --model <folder> --host 127.0.0.1 --port <port>`, polls
   `GET /health` until 200 (30 s budget by default; override with
   `STARTUP_TIMEOUT=...`).
4. Sends one tiny `POST /v1/chat/completions` and prints the first
   ~1 KiB of the response so the operator can confirm the model
   actually generated something.
5. SIGINTs the server (3 s grace, then SIGKILL across the process
   group — same shutdown posture as Plume's supervisor).

Examples (do not hardcode — paths depend on your install):

```bash
# Plume's own model dir
./scripts/smoke-mlx-runtime.sh "$PLUME_MODEL_DIR/gemma-2b-it"

# LM Studio's models tree
./scripts/smoke-mlx-runtime.sh ~/.lmstudio/models/lmstudio-community/qwen2.5-coder-7b-instruct

# Locally AI's sandboxed HF cache (snapshot folder is the actual model)
./scripts/smoke-mlx-runtime.sh \
  "~/Library/Containers/app.locallyai.Locally/Data/Library/app.locallyai.Locally/huggingface/models/models--mlx-community--gemma-2b-it/snapshots/<sha>"
```

Decision tree if the in-app Gemma walkthrough fails:

| Smoke script exits | Plume's Start fails with     | What it tells you                           |
| ------------------ | ----------------------------- | ------------------------------------------- |
| `import mlx_lm` ✗  | `spawn failed`                | `mlx-lm` missing from the python on PATH    |
| folder shape ✗     | `local model not in inventory`| Plume's scanner won't classify it; missing `config.json` / tokenizer / weight |
| `/health` ✗ (30 s) | `health timeout`              | Weights are too big for memory, or model needs an mlx-lm version Plume's launch shell doesn't see |
| chat OK ✓          | still fails in Plume          | Plume-specific wiring problem; look at D52's "Logs & diagnostics" disclosure on the row |

The script never modifies the model folder, never downloads anything,
and never installs packages. Re-run as often as you like.

### Qwen MLX chat smoke — the local-first happy path (D90) {#qwen-mlx-smoke}

This is the **"does my local Qwen actually answer?"** one-command proof.
It is the no-arguments, auto-discovering wrapper over the D53 runtime
smoke above — same supervisor runtime path, but it resolves the Python
interpreter and finds the Qwen checkpoint for you. No UI, no
computer-use, no Ollama, no downloads.

```bash
# Point it at your mlx-lm venv (the same var Plume's supervisor uses):
export PLUME_MLX_PYTHON=$HOME/.venvs/mlx-env/bin/python
./scripts/smoke-qwen-mlx.sh
```

The script:

1. Resolves the interpreter the same way the MLX supervisor does —
   `PLUME_MLX_PYTHON` first (D58), then `~/.venvs/mlx-env/bin/python`,
   then `python3` / `python` — and only accepts one that can actually
   `import mlx_lm`.
2. Auto-discovers a Qwen checkpoint under `$PLUME_MODEL_DIR` (else
   `<repo>/plume-models`), preferring a **Qwen2.5-Coder 3B 4-bit**
   folder and falling back to any classifiable Qwen folder.
3. Hands off to `scripts/smoke-mlx-runtime.sh` (spawn → `/health` →
   one tiny `/v1/chat/completions` → validate → shut down).
4. Prints a single **`SMOKE: PASS`** / **`SMOKE: FAIL`** banner. On
   FAIL the diagnostic names the missing precondition (no interpreter,
   `mlx_lm` not importable, no Qwen model, server never healthy).

Overrides: `PROMPT_TEXT` (default "Reply with the single word: pong"),
`STARTUP_TIMEOUT` (default 90 s — a cold 3B/4-bit load is slower than
Gemma-2b), `PLUME_MODEL_DIR` (where to look for the checkpoint).

> Requires Apple Silicon: `mlx-lm` only runs on a Mac, so this smoke
> only reaches **PASS** there. On Linux / CI it exits **FAIL** at step 1
> with the venv playbook — that is the expected, honest result, not a
> regression. It never downloads a model (bring your own via the Local
> models panel) and never installs packages.

### Qwen propose-diff smoke — can the local model edit code? (D91) {#qwen-propose-diff-smoke}

The model-quality question: can a local 3B/4-bit Qwen produce a unified
diff that survives Plume's **own** validate → apply → revert path? UI-free,
no Ollama, no downloads, no writes to real source files (only a throwaway
temp fixture + Plume's pre-apply checkpoint inside it).

```bash
export PLUME_MLX_PYTHON=$HOME/.venvs/mlx-env/bin/python
./scripts/smoke-qwen-propose-diff.sh
```

The harness resolves the interpreter + Qwen checkpoint (same discovery as
the chat smoke), seeds a temp `greet.py`, starts mlx-lm, asks the model
for **only** a unified diff editing it, then hands the captured diff +
fixture to Plume's real patch code via the `#[ignore]`d Rust smoke test
`patch::propose_diff_smoke_tests::qwen_propose_diff_smoke` (which runs
`validate_patch` → `apply_patch` → `revert_patch`). It prints
`PROPOSE-DIFF: PASS` only when the diff validated, applied, and reverted
cleanly.

A malformed or non-applying diff is a **model-quality FAIL**, reported
with the captured diff and the Rust outcome (`Invalid` / `ApplyFailed`),
not a script bug — and because apply only runs after validation and is
all-or-nothing with rollback, the machine state stays clean. This is
smoke, not a guarantee: small local models often need a few tries.

The Rust cycle itself (minus the model) is covered in the normal suite by
three non-ignored tests in the same file (valid diff applies + reverts;
invalid diff reported, disk untouched; pre-image mismatch fails + rolls
back), so `cargo test` exercises the patch path even on Linux/CI. As with
the chat smoke, a real model PASS requires Apple Silicon.

### Gemma via Plume-managed MLX, end-to-end (D40 + D45 + D46) {#gemma-smoke}

This is the canonical happy-path smoke for a Plume-managed local
chat. It exercises the D40 process supervisor, D39 SSE parser,
D45 chat-routing, and D46 Start/Stop UI together. No Ollama,
no auto-install, no downloads from Plume — you bring the weights.

If you've hit a Start failure here, run the D53 smoke script
([above](#mlx-runtime-smoke)) FIRST. It isolates "is the model file
healthy with mlx-lm at all" from "is Plume's supervisor wiring
healthy" — the two failure modes look identical in the panel.

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
     somewhere of your choice (`~/.venvs/mlx-env`, a conda env, …)
     and either activate it before launching Plume, **or** set
     the D58 `PLUME_MLX_PYTHON` env override (see below) so Plume
     spawns the venv's interpreter directly.

   - **D58: `PLUME_MLX_PYTHON` override (recommended for GUI
     launches).** Set the env var to the absolute path of the
     interpreter you want Plume's supervisor to spawn:

     ```bash
     export PLUME_MLX_PYTHON="$HOME/.venvs/mlx-env/bin/python"
     ```

     With this set, Plume's MLX supervisor invokes
     `~/.venvs/mlx-env/bin/python -m mlx_lm server …` directly —
     **no shell activation required**. The venv's interpreter
     binary finds `mlx_lm` through its own site-packages, so the
     LaunchServices-bare-PATH gotcha (below) goes away. Empty /
     whitespace values fall back to bare `"python"` cleanly.
     Documented in `docs/MLX_RUNTIME.md § PLUME_MLX_PYTHON`.

   What does NOT work today: launching Plume from Finder /
   Spotlight when your venv is only active in the terminal AND
   `PLUME_MLX_PYTHON` is unset — the GUI app inherits
   LaunchServices' bare PATH, not your shell's. Either set
   `PLUME_MLX_PYTHON` (D58, above) or always launch from the
   activated shell. The D44 dev-alias symlink works either way
   as long as one of those two conditions holds.
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
   shows `<n> tokens · <prompt> prompt-tokens` after `chat/done`;
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
