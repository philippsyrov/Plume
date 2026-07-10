# Plume — Agent Instructions

Plume is an experimental open-source local AI coding editor. The product brief
is `docs/PLUME_PROJECT_SPEC.md`. Treat it as the source of truth for product
direction.

## Stack

Tauri 2 (Rust) shell + TypeScript / React 19 frontend with CodeMirror 6 as the
editor surface. Local model runtimes (MLX-LM, Ollama, LM Studio, llama.cpp)
reach the app through a `Provider` trait in `src-tauri/src/providers/`. No
Electron. No default cloud calls.

## Local-first positioning (read this before touching the model path)

- **MLX / Qwen is the current local-first proof path.** Plume-managed MLX
  on Apple Silicon is the happy path we build and verify against; the
  scripted `scripts/smoke-qwen-mlx.sh` (chat) and
  `scripts/smoke-qwen-propose-diff.sh` (code edit) are its proof. See
  `docs/MODEL_PROVIDERS.md` ("MLX-first, Ollama-compatible") and
  `docs/LOCAL_AGENT_NORTH_STAR.md`.
- **Ollama is compatibility, not the happy path.** It works and stays
  supported, but it is not the experience Plume is designed around — see
  `docs/LOCAL_AGENT_NORTH_STAR.md § "Ollama is compatibility, not the
  center"`. Don't frame Ollama as the default in docs or UI copy.
- **The tool catalog is still a scaffold; the agent loop can now complete
  one safe *mutating edit*, patch-only.** `tools.*` (D92) and
  `agent.dryRun` (D93) are read-only / dev-only proofs of shape.
  `agent.singleStep` (D96) drives the MLX model for ONE turn; D99 lets it
  fold a read-only file as context; **D100** closes the first mutating
  loop: the model proposes a diff, Plume validates it (read-only), and the
  user can **explicitly apply** it through the existing `patch.apply`
  (checkpoint + atomic write) and **revert** it via `patch.revert`. The
  command itself still writes nothing — only a user click applies — and the
  boundary stays hard: **patch-only mutation, no shell command execution
  and no arbitrary `tools.invoke`.** Anything beyond applying a validated
  diff waits for a real executor behind an explicit approval / allowlist
  gate (`docs/SAFETY.md`, `docs/IPC_ROADMAP.md § Tools`).

## Status

Early foundation. Slice A landed the IPC and safety contracts.
Slice B added project open + persisted trust + `ProjectMeta`.
Slice C added trusted display reads, a read-only file browser,
CodeMirror viewing, blocked secret-file reads, and a packaged-app
smoke harness. Slice D0 documented the provider track vs engine
track split. Slice D1 added the provider registry plus
reachability UI — `providers.list`, `providers.health`, and a
small panel showing each runtime's category and current state.
Slice D1.5 reshaped the trusted-project view into a three-zone
workspace shell — left navigation (file tree + provider strip),
center agent placeholder, right file inspector — without
committing to a chat backend yet. Slice D2 layered the first
adapter-specific HTTP probe on top: when Ollama answers TCP, Plume
also fetches `/api/tags` and shows the installed model count plus
names in the provider panel. Slice D3 turned "Ollama has models" into
"which ones fit this machine" — a lazy per-model `POST /api/show`
reads family / parameter count / quantization / context length, a
cautious fit estimator weighs them against `sysctl hw.memsize`, and
the panel expands each model in place with a green / amber / red
verdict plus a `GGUF / Metal (Ollama)` runtime-path label. Slice D4
extended the model-list probes to LM Studio and llama.cpp via their
OpenAI-style `/v1/models` endpoints; those expose
runtime-reported / server-visible model ids only, not full
downloaded catalogs (LM Studio's richer `/api/v1/models` is
roadmap). llama.cpp moved off the "not configured" list onto the
TCP probe set at the documented 8080 default, with the shared
parser in `src-tauri/src/providers/openai_compat.rs`. Slice D5
added a lightweight host-machine status strip: `system.snapshot`
shells out to `sysctl` / `vm_stat` / `uname` / `sw_vers` and feeds
memory pressure + RAM-used + swap chips polled at ~7s, with CPU /
GPU live usage explicitly on the roadmap. Slice D6 added a
window-local model picker shell: each model row in the provider
panel grew a Select button (disabled when reachability is not
`available`), the agent workspace gained a "Selected model"
banner above the mode cards, and selection state lives in React
in `TrustedView` via `useSelectedModel`. Slice D7 added the first
real read-only chat against Ollama. Slice D7.1 turned that into a
streaming surface: `chat.send` now returns a `ChatStreamId`
immediately and the assistant reply arrives as `chat.token`
events; a new `chat.cancel(streamId)` verb flips a cooperative
cancel flag the streaming loop polls between NDJSON line reads.
The `ChatPanel` renders the streaming reply in place with a
blinking cursor and shows a Stop button while a stream is in
flight; cancellation keeps the partial reply in the transcript
with a "stopped by you" marker. Slice D8 added read-only file
context for chat: an "Attach current file" control on the chat
panel hands a project-relative path to the backend, which uses a
Rust-private prompt-read path (`prompts::assemble` +
`prompts::read::read_for_prompt` + `prompts::redact`) to fold the
file content into the last user message. Raw bytes never cross
IPC; the secret redactor (`AKIA…`, `ghp_…`, `sk-…`, JWTs,
`Bearer …`) is the only producer of `RedactedContent`, and a 256
KiB cap plus binary / secret-filename / `.git/`-whitelist blocks
sit in front of it. The chip on the chat panel is the source of
truth for what got attached; clearing it removes the context
from the next send. Slice D9 layered provider-neutral generation
telemetry onto `chat.done`: the terminal event now carries a
`stats` object with `outputTokens`, `evalMs`, `tokensPerSecond`,
`promptTokens`, and `promptMs` parsed from Ollama's final-frame
`eval_count` / `eval_duration` / `prompt_eval_count` /
`prompt_eval_duration`. The chat panel renders a tiny footer
under completed assistant messages — `<n> tokens · <r> tok/s` —
with the prompt-eval breakdown carried in the hover title;
cancelled / truncated / errored streams suppress the footer
because they have no authoritative metrics. Slice D10 narrowed
the D8 attachment to an optional line range: the read-only
inspector's editor tracks the user's text selection and reports
1-based line numbers; the chat panel's attach button flips to
"Attach selection" when a non-empty selection exists and emits
`startLine` / `endLine` on the `chat.send` payload. The backend
slices the redacted content to that range AFTER redaction (so
secrets outside the range never appear and the range can't be
used to dodge redaction inside the slice). Slice D11 layered
project-instructions auto-context onto chat: when the trusted
project has a root `AGENTS.md`, the backend reads it through the
same Rust-private `prompts::read::read_for_prompt` path (size
cap, binary detection, hardlink check, redactor) and prepends it
as a `system` message on every send. A broken `AGENTS.md`
(oversize / binary / hardlink) skips silently; a new
`instructionsIncluded: boolean` on the synchronous
`chat.send` response confirms what landed. The chat header
renders a small `¶ AGENTS.md` badge when the project has one.
Re-read on every send picks up edits to `AGENTS.md` mid-session.
Slice D12 added a read-only `chat.context` IPC alongside
`chat.send`: same trust gate, same prompt-read pipeline, but no
model call — it reports back what AGENTS.md and the optional
attachment would contribute (`originalBytes`, `redactionCount`,
line range when applicable). Attachment rejections that the real
send would surface as a typed `IpcError` come back here as
`attachment.status === 'blocked'` with a stable `reason` code,
so the chat panel can render a small "Context preview" area
area listing both AGENTS.md and the attachment without a blocked
attachment hiding the AGENTS.md side. The two paths share
`prompts::preview_context` so the preview's numbers always match
what the actual send would log.
Slice D13 is CSS/layout polish only — no IPC, no Rust changes.
Once a project is trusted, the global `Plume` hero is hidden so
the compact status strip is the top-of-window identity (the open
form still keeps the hero). The whole window is a fixed canvas
now: `overflow: hidden` on `html`, `body`, `#root`, and
`.plume-shell` prevents page-level scrolling — only the file
listing, the chat transcript, and the inspector editor scroll
internally. The CodeMirror gutter paints `var(--paper)` and
holds a `min-width: 40px` so horizontally-scrolled file content
no longer slides under the line numbers. A new `--radius-small`
token (4px) replaces the hardcoded `3px` corners on small chips
and rows; `--radius-soft` (6px) stays for full panels. UI_STYLE
documents the workspace shell rule for future slices.
Slice D14 is chat UX hardening — frontend-only, no backend
capability changes. The chat panel pre-flights the selected
provider via a new `useProviderReachability` hook so the user
sees "Ollama not reachable — start the daemon and click Recheck
to send." before typing, with a Recheck button that re-probes
without remounting the project. Textarea stays enabled in that
state so the user can compose while starting the daemon; Send
disables until reachability returns to `available`. `useChat`'s
`send` now returns a `SendOutcome` (`'accepted' | 'rejected' |
'busy' | 'empty'`); ChatPanel restores the attachment chip on a
synchronous `'rejected'` so the user doesn't re-attach after a
transport failure. Completed assistant turns gain a subtle Copy
button anchored top-right of the entry; it uses
`navigator.clipboard.writeText` and flips to `Copied!` for ~2 s.
Streaming and cancelled turns deliberately don't expose Copy to
avoid misleading partial-reply captures. SMOKE_TESTING gains
steps 31-35 covering the new affordances.
Slice D15 adds the propose-diff preview path. An optional
`mode: 'chat' | 'proposeDiff'` field on `chat.send` (defaults to
`'chat'`, so D7.1 wire compatibility is byte-identical) tells
the backend which system message to prepend; the new
`prompts::mode::propose_diff_system_message` pins the model to
respond with a unified diff inside a single fenced ```diff
block. The chat panel exposes a segmented `Chat | Propose diff`
toggle in the header; user turns sent in propose-diff carry a
`¶ propose diff` badge in the transcript so history is honest
about which reply is a diff. The assistant renderer parses the
fenced ```diff (or ```patch) block and shows a coloured diff
panel (additions paired with `--good`, deletions with `--bad`,
hunk headers in pencil). A disabled Apply button sits below the
diff with a tooltip naming the boundary — **Plume does not apply
patches in D15** and no IPC verb writes to disk on behalf of a
diff. The existing D14 Copy button covers "grab the diff and
apply by hand." A prose-only reply in propose-diff mode shows a
warn-coloured "No diff fence detected" hint instead of a fake
preview. SMOKE_TESTING gains steps 36-39 for mode toggle, diff
render, prose fallback, and mode-on-next-send semantics.
Slice D16 layers a read-only validator on top of D15. A new
`patch.validate(payload: { diff })` IPC parses the assistant's
reply (extracting the fenced ```diff/```patch block when present,
or treating the payload as a bare unified diff), walks `--- /
+++ ` header pairs, counts `@@` hunks per file, and enforces
project-root path safety on every diff-side path — lexical
reject for absolute paths / `..` components / NUL bytes, plus
ancestor canonicalize-via-`ensure_inside` (walk up from the
joined path to the deepest existing on-disk path and check it
stays inside root) so symlinked-out targets AND symlinked-out
parents on create-diffs (`link/new.rs` where
`<root>/link -> /tmp/outside`) both reject. Create-diffs
against genuinely-missing paths whose ancestors stay inside the
project are permitted. The verb returns
`{ ok: true; touches; hunks }` with per-file change-type
classification (modify / create / delete / rename) or
`{ ok: false; errors[] }` with typed kinds (`noDiffBlock`,
`noHunks`, `malformed`, `devNullBoth`, `pathEscape`,
`absolutePath`). Structured validation errors come back IN-BAND;
the Promise only rejects for `Version` (envelope mismatch) or
`NeedsApproval` (no trusted project). The chat panel renders the
verdict as a small pill between the diff body and the Apply row
(`valid diff · N file(s) · M hunk(s)` in `--good`, `invalid
diff: <reason>` in `--bad`, `validation unavailable: <message>`
in pencil for IPC failures). The Apply button stays disabled
even when validation passes — **D16 still writes nothing to
disk; `patch.apply` / `patch.checkpoint` / `patch.revert` are
roadmap.** SMOKE_TESTING gains steps 40-42 covering the valid
pill, the invalid-via-devtools probe, and the IPC-failure
fallback.
Slice D17 is a docs-only roadmap slice. It scopes Plume's
eventual computer-use track — the EMITTING surface that lets the
model drive a target environment on the user's behalf via a
`computer.*` tool family — without shipping any of it. The
roadmap splits the track into two phases (Phase A is a bundled
in-app webview sandbox with strict CSP / no host access; Phase B
is host desktop via macOS accessibility APIs, off by default,
per-session opt-in, per-target allowlist), reserves the verb
shapes (`computer.session.start/end`, `computer.capture`,
`.click`, `.type`, `.scroll`, `.drag`, `.observe`, `.trace`),
defines the per-session approval contract (no persistent ledger,
no "remember this" toggle, mandatory target allowlist, visible
trace area with Pause/Stop), and explicitly distinguishes the
EMITTING role from the RECEIVING role described in
`docs/AGENT_OPERABILITY.md`. The doc shape leaves room for a
future `trycua`/`cua-driver` integration without committing to
one. Touched docs: `PLUME_PROJECT_SPEC.md § 13.5`,
`AGENT_OPERABILITY.md § Plume as a computer-use HOST`,
`IPC_ROADMAP.md § Tools / Computer use`,
`SAFETY.md § Computer-use sandbox`, `UI_STYLE.md § Computer-use
trace area`. D17 adds no tests; the status test count was
corrected from 281 to the D16 value of 286 (a doc-only
correction, not a new run of tests).
The non-streaming `send_chat` adapter is retained
`#[cfg(test)]`-only as a reference implementation. No multi-file
attachments, no `README.md` auto-context, no per-directory
overlays, no patching, no command running, no `ollama serve`
auto-start, no tool calls. The Rust backend compiles with 286
cargo tests passing, the TS frontend typechecks, and
`./scripts/verify.sh` passes with clippy clean. The agent loop,
file writes, the patch flow, and the computer-use track are not
implemented yet. See `docs/DEVELOPMENT.md` for working with the
current slice and `docs/IPC_ROADMAP.md` for what's reserved.
Slice D21 is a docs + soft-guardrail slice — no feature code.
It adds `docs/DECOMPOSITION.md` (file-size rule plus a concrete
refactor map for the current oversized files: `commands/chat.rs`
at 1,860 lines, `ChatPanel.tsx` at 1,523, `prompts/assemble.rs`
at 1,323, `chat/ollama.rs` at 1,317, and the long-tail yellow
zone) and a warn-only `scripts/check-file-sizes.sh` wired into
`scripts/verify.sh § File sizes`. The check never fails CI
today; existing oversized files are grandfathered. Future
decomposition slices are explicit and unbundled from feature
work — see DECOMPOSITION.md § Cadence rule.
Slices D22–D26 are decomposition refactors against the red-zone
files mapped in `DECOMPOSITION.md`: D22 split
`src/features/chat/ChatPanel.tsx` (1,523 → 364-line orchestrator
plus sibling files); D23 split
`src-tauri/src/commands/chat.rs` (1,860 → 299); D24 split
`src-tauri/src/prompts/assemble.rs` (1,323 → 592 production +
731 tests in a sibling file via `#[path]`); D25 split
`src-tauri/src/chat/ollama.rs` (1,317 → 117); D26 split
`src/styles/layout.css` (1,650 → 22) into 17 sibling files.
Each slice held behaviour and the cargo suite steady — the
count stayed at 286 across the whole run.
Slice D27 added the read-only `providers.localModels` IPC verb
plus a Local models section in the provider panel. The scanner
walks `PLUME_MODEL_DIR` (default `<project>/plume-models`),
skips symlinks, and reports `.gguf` files, `.safetensors`
files, and HuggingFace-style transformer folders (`config.json`
+ a `tokenizer*` file + a `.safetensors` / `.gguf` / `.npz`
weight file). The folder kind is deliberately
`transformer-folder`, not `mlx-folder`: the same on-disk layout
is produced by any `huggingface-cli download`, and the runtime
honesty rule (the one that keeps Ollama labelled `GGUF / Metal`
on Mac) forbids claiming MLX without verifying it. No
downloads, no import, no launch, no selection — local weight
inventory only. Cargo suite is at 295 tests (286 + 9 new in
`providers/local_models.rs`).
Slice D29 is local-model hardening — no new feature surface. The
scanner now enforces a defensive nesting cap with walkdir-style
semantics: the model directory is depth 0, its children depth 1,
and entries strictly past depth 8 are invisible (files, plain
folders, and transformer folders alike). Dot-prefixed entries
(`.git`, `.DS_Store`, `.cache`, dotfile configs) are also skipped.
The provider panel fails soft on a local-model scan rejection: the
registry and reachability snapshot still render, the Local models
section shows the failure inline instead of taking down the whole
panel. Cargo suite is at 299 (295 + 4 new tests covering the cap
boundary on plain files AND transformer folders, the dotfile skip,
and the in-cap nested case).
Slice D30 added resizable workspace columns plus show/hide
toggles for the trusted-project shell — frontend only, no IPC or
Rust changes. The left navigation and right inspector columns
are now drag-resizable through 8 px handles that double as the
visible gutters; max widths are container-aware (derived from
`window.innerWidth` minus shell padding, a 280 px center
reservation, and handle widths) so neither side can starve the
agent center on any sane window size. Chevron buttons in the
status strip collapse either side, and `Cmd+Shift+[` /
`Cmd+Shift+]` toggle the same state from the keyboard
(`event.code` bindings so non-US layouts work). Widths and
visibility persist to
`localStorage['plume:workspace-layout-v1']`. When the viewport
shrinks or a hidden panel un-hides, a slack-based rebalance
redistributes the excess in proportion to each side's
`currentWidth - staticMin` so the center stays above its
minimum without flattening the user's preferred widths. The
chat transcript also dropped its old `max-height: 50vh` so the
input and Send button no longer slip below the visible window
when Plume is sized short. Cargo suite is unchanged at 299 —
D30 is pure frontend layout, no Rust changes.
Slice D31 added `patch.apply` — Plume's first writing verb. It
takes a previously-validated unified diff, re-runs the validator
server-side (the frontend's cached result is treated as a UI
hint, not a security artifact), verifies every hunk's pre-image
against disk, takes a filesystem-backed checkpoint at
`.plume/checkpoints/<id>/` BEFORE the first write, then writes
each touched file via sibling-tempfile + atomic rename. Apply is
all-or-nothing: any pre-image mismatch rejects without writing;
a mid-apply write failure rolls back every previous write via
the checkpoint and surfaces `reason: 'writeFailed'`. Supported
change types are modify, create, and delete. Rename is
classified by the validator but rejected by the applier with
`reason: 'scopeUnsupported'` — rename apply is reserved for a
follow-up slice.
The chat panel's Apply button is now wired: enabled when
validation is green, disables while in flight, flips terminal to
`Applied` with the 8-char checkpoint prefix in the pill. The
parser also grew `ParsedHunk { old_start, old_count, new_start,
new_count, lines }` so the applier can re-verify pre-image
against disk; the `\ No newline at end of file` marker is
intentionally dropped (a later slice may handle the flip-newline
case).
Manifest format under `.plume/checkpoints/<id>/` is JSON, not
TOML — the design doc loosely mentions TOML; JSON keeps us off
a new crate dependency. Cargo suite is at 326 (299 + 27 new
tests across the parser, the applier, checkpoint round-trip,
and path-safety / rollback). D33 lifted the rename and revert
deferrals.
Slice D32 added toggleable inner panels on top of D30's outer
columns. Inside each visible side column the user can now
show/hide individual panels independently: left column carries
Files, Providers, and Local models; right column carries
Inspector (with Diff / Preview slots reserved for later slices).
A small chip strip at the top of each column renders one pill
per panel — filled = visible, outlined = hidden, click toggles —
and stays rendered when the column is up so a user who hid every
panel inside the column still has the recovery affordance. The
state lives in `useInnerPanels`, persisted to
`localStorage['plume:inner-panels-v1']` (independent from the
outer D30 layout key so the two don't share migrations). All
panels default to visible on first run — the toggle is opt-out.
ProviderPanel was split into `ProvidersPanel` + `LocalModelsPanel`
backed by a shared `useProviderInventory` hook called once at the
trusted-view level, so the split did not double the IPC load.
When every panel in a column is hidden the column renders an
`EmptyColumn` placeholder explaining how to bring one back. No
backend changes; cargo suite stays at 326.
Slice D33 finished the apply safety loop: `patch.revert({
checkpoint })` and rename apply both landed. Revert reads the
checkpoint's manifest, drift-detects every touched file against
the stored post-apply image, and rejects in-band on any
disagreement (`reason: 'drift'`); on agreement it applies the
inverse of each manifest entry all-or-nothing. A pre-revert
in-memory snapshot covers rollback on any mid-revert write
failure — a durable redo checkpoint is a follow-up. The
checkpoint manifest grew a `version` field (current = 2) and
a new `post/` subtree mirroring `files/`; D31-vintage
checkpoints (no version, no post/) reject with
`unsupportedCheckpoint` since drift detection has nothing to
compare against. Rename apply uses the existing path-safety
defenses (the validator already canonicalizes both old and new
paths), refuses to clobber an existing destination, and
supports rename-with-edits in a single atomic operation
(rename then sibling-tempfile body write). The parser also
relaxed its "every file must touch a hunk" rule for renames
so pure rename-no-edits diffs parse cleanly. UI: a Revert
button appears next to the now-disabled Apply button on a
successfully applied turn; the validation pill shadows with
`reverting…` / `reverted · N files` / `revert failed (...)`
in turn. Cargo suite is now at 343 (326 + 17 new tests across
rename apply, revert happy paths, drift detection, idempotency,
unknown / malformed checkpoint ids, symlink defenses on the
checkpoint dir, and the version-gate rejection for D31
vintage). The IPC handler count grew to include `patch.revert`.
Follow-up hardening brought the cargo suite to 348 by rejecting
tampered checkpoint image symlinks / hardlinks and tightening the
rename rollback cleanup path. Slice D34 is docs-only: it adds
`docs/LOCAL_AGENT_NORTH_STAR.md` and pins the product direction as
MLX-first local model ownership plus Hermes-class agent memory,
skills, toolsets, and Sass-style distillation. Ollama remains a
compatibility provider, not the required happy path. No code or IPC
changed in D34.

Slice D35 is a pure decomposition refactor across the three patch
amber files. `parse.rs` dropped from 862 to 492 lines by moving its
inline test module to a sibling `parse_tests.rs` via `#[path]`.
`apply.rs` dropped from 1045 to 772 lines by extracting
`apply_hunks_to` + `create_from_hunks` into `apply_hunks.rs` and
`rollback_apply` into `apply_rollback.rs`. `revert.rs` dropped from
807 to 445 lines by extracting per-entry planning (`RevertPlan`,
`plan_revert_entry`, `validate_manifest_path`, `drift_check`,
`load_pre_image`, `change_type_to_wire`) into `revert_planning.rs`.
No behavior change; the cargo suite is unchanged at 348. The whole
patch module is now green or yellow.

Slice D36 added the first floor of verified MLX detection. The
local-model inventory now upgrades a `transformer-folder` to
`mlx-folder` only when actual MLX evidence is present: either a
`weights.npz` shard at the folder root (legacy MLX format) or a
`config.json` whose top-level `quantization` object carries both
`group_size` and `bits` integer keys (the MLX-LM quantization
shape). HuggingFace's `quantization_config` key — used by
bitsandbytes-quantized models — is intentionally NOT sufficient.
Unquantized MLX safetensors uploads stay classified as
`transformer-folder` because their on-disk shape is identical to
vanilla HF. `config.json` reads are bounded at 256 KiB. The wire
adds the `'mlx-folder'` kebab variant to `LocalModel.kind`; the
panel renders it as `MLX folder`. Cargo suite is at 356 (348 + 8
new tests: two positive paths, two negative-key paths, partial
shape, malformed JSON, oversize file, kebab-case serialization).

Slice D37 added a local-memory MVP. Three IPC verbs (`memory.index`,
`memory.remember`, `memory.forget`) back a flat JSONL store under
`<project>/.plume/memory/entries.jsonl`. Every remembered text passes
through the same secret redactor (`prompts::redact`) the prompt-read
pipeline uses — the pre-redaction string never reaches disk; `sk-…`
and `ghp_…` show up as `[REDACTED:<kind>]` with a per-entry
`redactionCount`. Caps are hard: 100 entries, 1 KiB per entry, 64
KiB total. Reaching either entry-count or total-byte cap returns
`capacityReached`. `memory.forget` is idempotent and validates the
opaque `m_[0-9a-fA-F]{32}` id shape before any read. `.plume/`
symlinks are refused (same guard as the checkpoint dir). UI: a new
Memory panel in the left column toggle strip with an input, a small
"N of 100 entries · K KiB used" hint, and a per-entry Forget button.
No embeddings, no session log replay, no distillation — those are
reserved for follow-ups. Cargo suite at 367 (348 + 15 memory store +
4 command-payload tests).

Slice D38 is a docs-only research spike that pins the
implementation contract for the upcoming Plume-managed MLX runtime.
`docs/MLX_RUNTIME.md` documents the verified `mlx_lm.server` surface
(CLI flags, default 127.0.0.1:8080, GET /health, GET /v1/models,
POST /v1/chat/completions with SSE `data: ...\n\n`, SIGINT-graceful
shutdown) and the Plume integration plan (port allocation that
avoids llama-server's 8080, local-folder-only model paths for the
MVP, SSE chat routing reusing the OpenAI-compat shape, no
auto-install of `mlx-lm`). Source ground-truth: `mlx_lm/server.py`
on the ml-explore/mlx-lm main branch. No code or IPC changed in
D38.

Slice D40 lands the Plume-managed MLX-LM process supervisor
skeleton. New module `providers/mlx_lm/process.rs` owns the
five-piece lifecycle: ephemeral port allocation
(bind 127.0.0.1:0 → read port → drop listener), spawn of the
non-deprecated `python -m mlx_lm server --model PATH --host
127.0.0.1 --port N --log-level INFO`, background stdout+stderr
drain into a 16 KiB ring buffer, GET /health probe with
50/200/500 ms backoff inside a 30-second budget, and SIGINT-then-
SIGKILL shutdown (3 s grace). Process-wide registry keyed by
opaque handle ids; the child runs in its own session via setsid()
so a SIGINT to Plume doesn't double-signal the child. New IPC:
`providers.startServer({providerId, modelId})` → `ServerHandle`,
`providers.stopServer({handleId})` → `{ok}`. Only
`providerId: "mlx-lm"` is supervised today; only `mlx-folder` and
`transformer-folder` LocalModel kinds are accepted. No chat
routing yet (that consumes D39's SSE parser in a follow-up
slice); no model download; no auto-install of `mlx-lm`. Cargo
suite at 395 (377 + 18 D40 tests: port allocator, default-command
shape, build-command-args, ring buffer including 0-cap and
overflow, health probe success/503/timeout, start_server input
validation, missing-binary path, health-timeout cleanup,
stop-server unknown-handle, and a kill-and-reap exercise). Raw
FFI bindings for `kill(2)` and `setsid(2)` rather than adding a
`libc` dependency.

Codex D40 review-round fixes: (1) `providers.startServer` now
requires a trusted open project before spawn — running a Python
subprocess is shell command execution and the trust gate is the
right boundary. `stopServer` stays gate-free so a revoked trust
can't orphan a running child. (2) `start_server` now retries
once on `HealthTimeout` to cover the OS port-race window
between `allocate_port`'s drop and the child's bind; the inner
helper `try_start_once` is the single-attempt unit. (3) The
SIGINT→SIGKILL fallback now sends SIGKILL to the whole process
group (negative pid) instead of just the direct child, so any
grandchildren mlx-lm may have spawned can't survive the kill.
Cargo suite at 397 (added two regression tests: retry-elapsed
relative to a single attempt, and short-circuit on
non-HealthTimeout errors).

Slice D41 added on-disk details for local-model rows. A new
`providers.localModelDetails` IPC verb resolves an inventory id
back to its folder/file under the model directory and surfaces
architecture, model_type, max-context, MLX quantization bits +
group_size, tokenizer presence, weight-file count, and total
weight bytes — every field independently optional so a vanilla HF
folder reports what it has and stays silent about what it doesn't.
The reader sits in `providers/local_model_details.rs` (sibling of
`providers/local_models.rs`) and reuses the scanner's symlink
defenses + the same 256 KiB `config.json` cap; HuggingFace's
`quantization_config` key is intentionally NOT surfaced as MLX
quantization. The Local-models panel expands each row in place
with a disclosure caret and lazy-fetches details on first open;
fetch errors stay inline. Cargo suite at 399 (377 + 22 D41 tests
covering path safety, single-file vs folder, all config edge
cases, symlinked config refusal, and the camelCase wire shape).
No launch, no selection change, no download.

Codex D41 review-round fix: `providers.localModelDetails` now
runs `scan_model_dir` first and requires the id to match an
inventory entry before reading details. Without the gate any
path-safe regular file under the model dir (`README.md`, a stray
`.txt`) would have resolved through and come back as a 1-weight
"model" — the resolver only enforced path safety, not inventory
membership. The fix moves the source-of-truth check to the handler
where it belongs; the reader stays the same. Cargo suite at 402
(+3 commands::providers regression tests).

Slice D42 wires the D37 memory store into chat context. Every
`chat.send` and `chat.context` now folds the project's memory
entries into a bounded system message via `prompts::assemble`. A
new `memory::read_for_prompt(root, byte_cap)` returns newest-first
entries within a 4 KiB cap (`prompts::MEMORY_CONTEXT_BYTE_CAP`);
older entries are dropped first. The injected system message is
tagged "read-only notes the user remembered earlier" so the model
doesn't elevate them to instructions. Both IPC responses gain a
`memory: ChatMemoryUsage | null` field — `{entryCount, bytes,
byteCap, truncated}` — and the chat header gets a sibling
`MemoryBadge` next to the AGENTS.md badge that flips between
"available" (preview) and "included" (post-send) the same way
`InstructionsBadge` does. Memory store failures (planted `.plume`
symlink, etc.) skip silently — same posture as a broken AGENTS.md;
chat continues, the badge stays hidden, the panel surfaces the
error from `memory.index`. Cargo suite at 392 (367 + 15 D42 tests:
5 in `memory::tests`, 10 in `prompts::assemble::tests`).

Codex D42 review-round fix: the chat header's `MemoryBadge`
preview was going stale after a remember/forget because
`useChatContextPreview` only re-fired on attachment / AGENTS
changes. New `src/features/memory/memoryRevision.ts` exports a
tiny revision bus via `useSyncExternalStore`: MemoryPanel calls
`bumpMemoryRevision()` after every successful remember/forget,
and `useChatContextPreview` adds the revision to its effect deps
so it refetches `chat.context` with the new memory state. The
actual `chat.send` path was always honest (assemble re-reads on
every send); this only fixes the forward-looking preview.

Slice D43 added a read-only search verb on top of D37's memory
store. `memory.search({query, limit})` returns a ranked list of
hits — case-insensitive substring match across entry text, ranked
shorter-entry-first then newest-first, capped at 50 results per
call and 256-byte queries. Same symlink-safe path resolver and
process-wide mutex as `memory.index`; the verb never mutates the
store (a regression test asserts byte-equality of `entries.jsonl`
before and after). New `MemorySearchHit { entry, matchCount,
firstMatchIndex }` shape so the UI can render "5 matches" hints
and highlight first occurrence. The Memory panel grew a tiny
search field above the list that debounces at 200 ms; the entry
list collapses into the result view while a query is active.
Cargo suite at 388 (377 + 11 D43 tests: empty/oversize/zero/
oversize-limit query rejections, empty-store, case-insensitive,
ranking, truncation, match-count + first-index, symlink refusal,
and a no-mutation regression).

Slice D44 is the manual-testing convenience layer. Adds
`docs/MANUAL_TESTING.md` — a checklist that walks through trust,
file browser, chat (token streaming + stop + attach +
propose-diff), patch validate/apply/revert, memory CRUD, memory
search, memory-in-chat-context, local-model details expansion,
and the MLX supervisor handle — and a small new
`scripts/install-dev-alias.sh` that drops a `~/Desktop/Plume
(dev).app` symlink to the smoke-app build so subsequent launches
are one-click. The script is opt-in and reversible (`rm
"$HOME/Desktop/Plume (dev).app"`); it does NOT install into
`/Applications`, write to `~/Library`, register a URL scheme, or
touch shell rc files. Existing `scripts/smoke-app.sh` is the
build entry-point this slice points at — D44 does not duplicate
it. No Rust or frontend code changed.

Slice D45 wires chat through the Plume-managed MLX runtime,
consuming both the D40 process supervisor (port + handle) and the
D39 OpenAI-SSE parser. New `chat::mlx_lm` adapter mirrors the
`chat::ollama::stream_chat` shape: POST `/v1/chat/completions` with
`stream: true` + `stream_options.include_usage: true` against
`127.0.0.1:<port>`, drive the SSE parser per wire line, emit
`chat.token` events for content deltas, and surface a terminal
`chat.done` when `data: [DONE]` lands. `chat.send` now takes an
optional `handleId` field — required when `providerId === 'mlx-lm'`,
ignored otherwise — and resolves it via a new
`providers::mlx_lm::lookup_port`. A missing/blank handleId rejects
`BadArgument`; an unknown handleId rejects `NotFound` so the
frontend can drive "start the server again" instead of guessing at
transport failure. MLX `chat.done.stats` populates `outputTokens`
and `promptTokens` from the OpenAI usage chunk but leaves
`evalMs` / `promptMs` / `tokensPerSecond` as `null` (the OpenAI
wire shape carries no per-phase durations — fabricating a
wall-clock fallback would be dishonest). The trust gate,
prompt-assemble pipeline (AGENTS.md + memory + attachment), and
cooperative cancellation are all untouched — they ride before the
adapter dispatch. Cargo suite at 493 (480 + 13 D45 tests: SSE
adapter happy path with deltas + usage + done, inlined-usage
chunk, done-without-usage default, EOF-before-done, cancel
mid-stream, 404 → ModelNotFound for both OpenAI error shapes,
500 → BadStatus, transport refused, in-order message wire shape,
both extractor branches; plus 9 routing tests in commands::chat::send
covering provider-id dispatch, handleId requirement, unknown-handle
NotFound, stats translation, EOF/cancel/error mapping). The
provider-id allowlist now reads "ollama and mlx-lm"; LM Studio and
llama.cpp can ride the same SSE adapter when their slices land.

Codex D45 review-round fixes: (1) MLX chat now sends the
supervisor's launched-model label on the wire's `model` field
instead of the IPC payload's `modelId`. `ServerProcess` records
`model_label` from `options.model_path` at spawn; the new
`lookup_handle_info(id) -> Option<HandleInfo { port, model_label }>`
exposes both atomically; `ChatRoute::Mlx` now carries
`{ port, model_label }` and `chat::mlx_lm::stream_chat` echoes the
label back as the OpenAI `model`. The frontend-visible model id
on `chat.done.modelId` still says the inventory name (e.g.
"gemma-2b") — only the wire's `model` field changed — so the
chat panel's label doesn't shift to an absolute path mid-
conversation. (2) Added a positive routing test
(`resolve_route_returns_mlx_with_port_and_model_label_for_registered_handle`)
that uses the test-only `register_for_test` helper to insert a
synthetic handle and asserts the route carries the supervisor's
model label, not the payload's modelId. Pre-fix
`register_for_test` was unconsumed and `PLUME_FULL_VERIFY=1`'s
clippy step failed on the dead-code lint. Cargo suite at 494
(+1 D45 Codex regression test).

Slice D46 surfaces start/stop/select for Plume-managed MLX servers
in the Local models panel. New `useMlxServers` hook owns per-
modelId lifecycle state (`idle` / `starting` / `running` /
`stopping` / `error`) and wraps `providers.startServer` /
`providers.stopServer` with sensible UX collapses: a `NotFound`
on stop (handle vanished between Plume instances) drops the
status to `idle` instead of leaving the row stuck in "stopping",
and a re-entry guard prevents a double-click on Start from
firing two spawns. Each `mlx-folder` / `transformer-folder` row
grows a Start button (or "starting…" / "port N · Stop" / "stopping…"
depending on state); single-file kinds keep the legacy row
layout since `mlx_lm.server` doesn't consume them. A successful
Start auto-selects the model (`providerId: 'mlx-lm'`,
`providerDisplayName: 'MLX (Plume-managed)'`) so the chat panel
immediately routes through the new handle without a separate
Select click. `useChat.send` gains a `handleId` SendOption that
ChatPanel reads from `mlxServers.handleOf(selected.modelId)` when
the current selection's provider is `mlx-lm`; the field is
omitted on the wire for any other provider so Ollama sends stay
byte-identical to pre-D46.

Codex D46 review-round fixes: (1) HIGH — chat panel
`disabledReason.ts` widened `SUPPORTED_PROVIDER_IDS` to include
`'mlx-lm'` alongside `'ollama'`, so the auto-select on Start
no longer trips `unsupported-provider`. The Ollama-shaped
`providers.health` probe still drives `provider-checking` /
`provider-unreachable`, but the gate skips it for mlx-lm — the
supervisor's handle registry is the source of truth for
"MLX server is running" there. New `'mlx-not-started'` disabled
reason fires when the selection is mlx-lm but no live handle
exists for that modelId; the placeholder + status text point
the user at the Start button in the Local models panel.
Textarea stays enabled so the user can compose a prompt while
walking over to click Start. (2) MEDIUM — `useMlxServers`
gained unmount cleanup: an unmount-tracking ref + cleanup
effect that fire-and-forget `providers.stopServer` for every
`running` handle when the host component tears down, plus a
post-resolve race guard in `start` so a `providers.startServer`
that finishes loading weights AFTER the project closes
immediately stops the freshly returned handle instead of
leaking it as an orphan child. Without the race guard, mlx-lm
loads of 10–15 s would routinely leak when the user opens then
quickly closes a project.

Slice D48 is a design doc + read-only Rust scaffold for memory
distillation. `docs/MEMORY_DISTILLATION.md` lays out the full
roadmap (v1 rule-based dedupe, v2 LLM-driven summary, audit
trail, redactor re-run policy, open questions). The scaffold —
`memory::distill_preview(root) -> Result<DistillPreview, _>` plus
`DistillPreview` / `DuplicateGroup` types and a `normalize_for_distill`
helper — is a pure read that identifies exact-after-normalization
duplicate groups (trim, collapse whitespace runs, lowercase) and
reports what an apply step would remove. The function is reachable
from Rust only; there is no IPC verb, no UI, no JSONL rewrite, no
LLM call, and the production binary cannot route to it. Future
slices wire `memory.distillPreview` / `memory.distillApply` once
the approval flow lands. The id-changes-when-size-grows property
is pinned by a regression test so a future apply step can re-check
"the cluster you confirmed is still current." Cargo suite at 493
(480 + 13 D48 tests: empty-store, no-duplicates, exact match,
case-insensitive, whitespace collapse, tab/newline collapse,
multi-group, group-id stability, group-id changes with size,
no-mutation regression, skip-empty-normalized, symlinked .plume
refusal, plus the documented normalization rules pin).

Codex D48 review-round fix: distill group id now encodes the
SORTED set of member entry ids, not just the normalized text
plus group size. Pre-fix, forgetting one duplicate and
remembering a different one between preview and apply kept the
size constant — so a stale apply would match the new (different-
membership) group with the same id and silently clobber the
wrong entries. The hash mixes a per-id NUL separator after the
normalized key and between member ids so a shorter id can't
collide with a longer one's prefix. Two new regression tests pin
the property: `distill_preview_group_id_changes_when_member_set_drifts_with_same_size`
catches the exact scenario Codex flagged, and
`distill_preview_group_id_is_independent_of_input_order`
documents that two stores with the same members in different
on-disk order still produce the same id (so a future JSONL
compaction doesn't invalidate every saved group id). Cargo
suite at 482 (+2 D48 Codex regression tests).

Slice D47 adds a canonical Gemma-via-Plume-managed-MLX smoke
walkthrough to `docs/MANUAL_TESTING.md` (anchored at
`#gemma-smoke`). The new section covers prerequisites
(`mlx-lm` installed, an `mlx-community/*` Gemma folder on
disk, a trusted project open), where to drop weights using
`$PLUME_MODEL_DIR` / `./plume-models` — no hardcoded user paths
— a step-by-step click path from the Local models panel
through `chat.send`, an `lsof` sanity check, the expected
behavior of MLX's null-eval-time stats, troubleshooting for the
five common failure shapes (kind classifier miss, missing
python/mlx_lm, health timeout, model-id mismatch, cancel
latency, hung stop), and a one-liner cleanup note for the case
where Plume crashes mid-stream. Docs-only — no IPC, no code,
no script. Depends on D40 / D45 / D46 for the surface the
walkthrough exercises.

Codex D47 review-round fixes: (1) Replaced the `pipx install
mlx-lm` / `uv tool install mlx-lm` recommendations with a venv
playbook (`python -m venv`, `uv venv`, or a dedicated `mlx-env`
on PATH). Both `pipx` and `uv tool install` create isolated
envs and only expose CLI shims on PATH; `python -m mlx_lm`
from a normal shell still can't `import mlx_lm` because that
env isn't on `sys.path`, so a user following the original doc
would still hit `spawn failed`. Added an explicit
"`python -c "import mlx_lm"` must exit cleanly" smoke check and
a warning that LaunchServices/Finder-launched apps inherit a
bare PATH (so the user has to launch from the activated shell).
(2) Rewrote the `model 'foo' not found` troubleshooting row to
match the D45 fix: Plume now echoes the supervisor's
`model_label` on the wire, so a model-mismatch surfaces only
when the recorded label has drifted from what mlx-lm has
loaded — the actionable fix is Stop → Start.

Slice D49 adds a no-project chat mode so Plume is usable as a
local chat client before (or without) opening a project. The
open form grows a secondary "Chat without a project" button
below the Open form (separated by a hairline rule so the
project flow stays the primary affordance). Clicking it
transitions the top-level `View` state machine to
`{ kind: 'chat-only' }`, which mounts a new `NoProjectChatView`
shell — a two-zone layout with the existing `ProvidersPanel`
and `LocalModelsPanel` on the left aside and `ChatPanel` on
the right. File navigator, inspector, Memory panel,
AGENTS.md badge, attachment chip, and patch UI all stay
unmounted; closing back via "Open a project" returns to the
open form. The chat path works for Ollama exactly like the
project flow today — `prompts::assemble` already tolerates a
`None` project root and skips AGENTS.md / memory cleanly,
`check_attachment_requires_trust(false, false)` already
passes, and the no-project shell deliberately omits the
attach UI so no `NeedsApproval` is reachable through the
happy path. For Plume-managed MLX, `LocalModelsPanel` gains a
`noProject` prop that renders the Start button as
`disabled` with title "Open and trust a project to start
Plume-managed runtimes." — the smallest safe path that keeps
the D40 trust gate on `providers.startServer` intact (the
"only spawn subprocesses for trusted projects" invariant is
load-bearing, no new approval gate added). Already-running
servers stay reachable: their handles live in the
supervisor's process-wide registry and chat dispatch resolves
them the same way, so a server the user started in a
previous trusted session keeps working in no-project chat
and its Stop button stays live (Stop is a cleanup verb the
backend doesn't gate). Local-model inventory still scans
`$PLUME_MODEL_DIR` / `./plume-models` so the panel shows
what's on disk. New `docs/MEMORY_DISTILLATION.md`-adjacent
contract note in `IPC_CONTRACT.md § chat` spells out the
no-project disposition (instructionsIncluded false, memory
null, attachment must be omitted, handleId still works).
No new IPC verbs, no wire-shape changes. Cargo suite stays
at 509; frontend `npm run typecheck` + `npm run build`
clean.

Slice D53 adds `scripts/smoke-mlx-runtime.sh <model-folder>`, a
standalone end-to-end smoke for the Plume-managed MLX-LM
runtime that runs OUTSIDE the full app. Given a model folder
on disk it (1) verifies `python -c "import mlx_lm"` actually
works in the current shell, (2) checks the folder shape
matches Plume's scanner floor (config.json + tokenizer +
weight), (3) allocates an ephemeral port, spawns `python -m
mlx_lm server --model … --host 127.0.0.1 --port …`, polls
`/health` until 200 (30 s budget), (4) sends one tiny
`/v1/chat/completions` and prints the first ~1 KiB of the
response, (5) SIGINTs the server with a 3 s grace and SIGKILLs
the process group on overrun. No auto-install (missing
`mlx_lm` prints the venv playbook and exits non-zero), no
downloads, no Plume UI dependency. Useful as a debug isolator
when an in-app Gemma Start fails: this script answers "is the
model file + mlx-lm healthy at all" without bringing the
supervisor wiring or chat panel into the picture.
`docs/MANUAL_TESTING.md` gains an "MLX runtime smoke script"
section (`#mlx-runtime-smoke`) referenced from the existing
Gemma walkthrough; the decision-tree table maps the script's
failure modes to the Plume UI errors the operator might also
see. Examples cite `$PLUME_MODEL_DIR`, `~/.lmstudio/models`,
and Locally AI's HF cache as model-folder sources without
hardcoding user paths. Docs + script only; no IPC, no code,
no test count change.

Slice D52 surfaces live diagnostics for running Plume-managed
MLX-LM handles. New read-only IPC verb
`providers.serverDiagnostics({handleId})` returns a snapshot
of the supervisor's view: handleId, bound port, child pid, the
exact `--model` label the supervisor launched with, unix-epoch
`startedAtMs`, derived `uptimeMs`, and the ring buffer's last
~16 KiB of stdout+stderr as `logTail` (lossy-UTF-8). `logBytes`
+ `logCapacity` let the UI surface a "log truncated" hint when
the buffer is at the cap. `NotFound` for an unknown handle id
so the panel can drop the disclosure without surfacing a
confusing error; no trust gate, same posture as
`providers.stopServer` (a server Plume already spawned remains
inspectable even after the launching project's trust is
revoked). The verb never mutates the registry — no spawn, no
restart, no signal.

`ServerProcess` gained `started_at_ms` (captured once at
registration via `now_unix_ms`), the previously dead-coded
`output: Arc<Mutex<RingBuffer>>` field is now consumed by
`lookup_diagnostics`, and `RingBuffer::len()` lost its
`#[cfg(test)]` gate so the snapshot can report
`logBytes` honestly. New TS API `getServerDiagnostics(handleId)`
+ `ServerDiagnostics` type. UI: a small "Logs & diagnostics"
disclosure appears under each `running` row in the Local models
panel; first expand fires the verb, the body shows
port/pid/uptime/model + the log tail in a fixed-height `<pre>`
with a Refresh button. No auto-polling — the dominant question
"is mlx-lm OK?" is answered well by an on-demand snapshot, and
the panel doesn't pay the supervisor's lock acquisition cost on
a timer it doesn't need.

Cargo suite at 513 (509 + 4 D52 backend tests: unknown handle →
None, registered handle returns recorded fields + log tail, ring
buffer cap is honoured in the snapshot, stopped handle returns
None without crashing — pins the "no crash on stopped process"
property the spec called out). New `register_for_test_with_log`
test helper lets the diagnostics tests pre-populate the ring
buffer without spawning a real mlx-lm child. PLUME_FULL_VERIFY=1
clean.

Slice D54 wires the D48 distillation scaffold through to a
read-only `memory.distillPreview` IPC verb plus a tiny "Find
duplicates" affordance in the Memory panel. No apply / rewrite
yet — that's a follow-up slice — but the user can now see what
an apply WOULD compact before any rewrite path exists.

Backend: `DistillPreview` / `DuplicateGroup` lost their D48
scaffold `#[allow(dead_code)]` and gained
`#[derive(Serialize)]` with `rename_all = "camelCase"` so the
wire shape matches `docs/IPC_CONTRACT.md`. Counts moved from
`usize` to `u32` (saturating arithmetic on the running
`would_remove` counter) so JSON serialization is platform-
independent. New handler `memory_distill_preview` in
`commands/memory.rs`, registered in `main.rs`. Same trust gate
as `memory.index` / `memory.search`; no project trust →
`IpcError::NeedsApproval`. `MemoryStoreError` maps to
`IpcError::Internal` for parity with the existing read verbs.

Frontend: new `getMemoryDistillPreview()` + types
(`MemoryDistillPreview`, `MemoryDuplicateGroup`) in
`src/lib/api/memory.ts`. `MemoryPanel` grows a collapsed-by-
default "Find duplicates" disclosure under the entry list; first
expand fires the verb, the body shows duplicate groups inline
with each group's survivor text + count, plus a Refresh button.
No apply / delete buttons — read-only.

Cargo suite at 512 (509 + 3 D54 tests: `DistillPreview` /
`DuplicateGroup` serialize with camelCase field names —
regression canary for serde rename drift; empty-store preview
shape pin under the renamed types). PLUME_FULL_VERIFY=1 clean.
Docs: `IPC_CONTRACT.md § memory` documents the verb shape and
the apply-is-roadmap posture.

Slice D56 was the real Gemma MLX smoke (no PR): proved Plume's
D50 scanner correctly finds and classifies the
`Gemma4ForConditionalGeneration` model in Locally AI's cache as
`mlx-folder`, but mlx_lm 0.31.3 cannot load this particular
vision-language Gemma 4 variant — it fails with `ValueError:
Received 126 parameters not in model: language_model.model.*`.
That's an upstream-mlx_lm support gap, not Plume. The scanner
plus the D52 diagnostics disclosure together surface the
failure honestly (the traceback lands in the log tail), but
the user had to read the Python traceback to know what bucket
of problem they hit. D57 turns that read-the-traceback step
into a one-line classification.

Slice D57 closes the D56 gap: when the supervisor's stdout/
stderr ring buffer contains a known mlx_lm failure shape, the
D52 "Logs & diagnostics" disclosure renders a contextual hint
above the raw log. New module
`src/features/providers/mlxLogPatterns.ts` exports
`detectMlxLogHint(logTail)`, a pure-frontend heuristic with
four kinds today: `unsupported-architecture` (the D56 shape —
`Received N parameters not in model` / `Missing N parameters
from model`), `unknown-model-type` (`KeyError` from
`mlx_lm.utils`/`mlx_lm.models`), `import-error` (`ImportError`
from `mlx_lm.models.*`, usually version skew), and
`cuda-missing` (`RuntimeError: ... CUDA` from a wrong-venv
install). Returns `null` when nothing matches — the raw log
remains the source of truth and the hint is purely additive.
Each hint carries a short `label` and a one-line `suggestion`
("Use a text-only chat model whose architecture mlx-lm
supports", "pip install -U mlx-lm", etc.). Rendered in a
red-bordered block between the meta strip and the log <pre>.
`data-hint-kind` on the block lets future tests / e2e selectors
target a specific failure kind.
`docs/MLX_RUNTIME.md § Model architecture support` documents
the supported-architectures rule, the dominant failure shape
(with the D56 traceback verbatim), the four hint kinds
D57 detects, what model families currently load cleanly
(Gemma 2, Llama 3.x, Qwen 2.5, Mistral, DeepSeek-Coder), what
families to expect failures from (`*ForConditionalGeneration`
vision-language variants until upstream catches up), and the
D53 smoke script as the off-Plume verification path.
Frontend-only; no IPC, no backend, no test count change.
PLUME_FULL_VERIFY=1 clean.

Slice D55 is docs-only. New
`docs/RUNTIME_COMPARISON.md` answers "why MLX-LM as the
Plume-managed runtime, and where do vLLM / llama.cpp / Ollama
/ LM Studio fit?" from the clean-room perspective of a Tauri
desktop app on Apple Silicon. Covers the five product axes
(hardware honesty, operator surface area, editor co-residency,
output quality for code, honest fallback), an at-a-glance
comparison table, runtime-by-runtime notes naming where each
runtime helps Plume and where it doesn't, and a "where vLLM
might help later" section that's explicit about its NVIDIA-
server design center vs Plume's MacBook-target. Decision rules
section so future "should Plume support X?" questions have a
default answer. Locally AI and Hermes are named only as
on-disk locations (D50 source for Locally AI's HF cache) — no
copied code, no copied product decisions. No IPC, no Rust, no
test count change. AGENTS.md key-docs list updated.

Codex D49 review-round fix (MEDIUM): hoisted `useMlxServers`
out of `TrustedView` and `NoProjectChatView` into `App` so
the bus is window-scoped instead of view-scoped. Pre-fix each
view created its own hook; D46's unmount cleanup
(`useMlxServers.ts`) stops every running handle on host
unmount, which meant jumping from a trusted session into
no-project chat tore down the user's running MLX server and
mounted the new view with an empty registry snapshot — the
"already-running servers stay reachable" claim was false.
With the hook hoisted, cleanup only fires when the App itself
unmounts (window close / quit). Selection state
(`useSelectedModel`) stays view-scoped on purpose so leaving
a trusted session doesn't carry the previously selected
model into no-project chat; only the MLX bus is hoisted
because the underlying supervisor registry is process-wide.
`ProjectView` / `TrustedView` / `NoProjectChatView` all take
the bus as a prop now.

Slice D51 surfaces the D50 sources in the Local models panel.
Each row grows a compact "Plume" / "Locally AI" / "LM Studio"
badge after the kind badge — the kind stays the loud signal
("what is it"), the source is pencil-coloured ("where did
Plume find it"). The disclosure body always renders a
`Source: <label> · ~/<short-path>` row at the top of the
details `<dl>`, before any details-fetch state — the source
is honest even while the lazy
`providers.localModelDetails` call is still running or has
just errored. `displayPath` folds `/Users/<name>/...` /
`/home/<name>/...` prefixes into `~/...` so external-cache
paths read cleanly; the badge's `title` attribute carries a
fuller description of what that source represents. Start
already worked for `mlx-folder` / `transformer-folder` rows
from external sources after D50's resolver rewrite, so D51
adds no backend changes. Frontend-only; cargo suite stays at
518, `npm run typecheck` + `npm run build` clean.

Slice D50 extends the local-model inventory beyond
`$PLUME_MODEL_DIR` to a small set of read-only "known sources"
so models the user already downloaded via other local apps
surface in the Local models panel. Two new sources alongside
the primary `plume-model-dir`: `locally-ai-cache`
(`~/Library/Containers/app.locallyai.Locally/Data/Library/
app.locallyai.Locally/huggingface/models`) and
`lm-studio-cache` (`~/.lmstudio/models`). Each scanner pass
goes through `scan_source(root, source)` and tags every entry
with the matching `LocalModelSource`; `scan_all_sources()` is
the merged inventory the IPC verb returns. `LocalModel.id`
became source-prefixed (`<source-tag>:<relative-path>`) so
two roots with an identically named subfolder no longer
collide on the wire; resolvers in `providers.localModelDetails`
and `providers.startServer` split on the first `:` to route
back to the right source root. Ollama's blob store
(`~/.ollama/models/blobs`) is deliberately NOT a source —
content-addressed blobs with no human-readable id outside
Ollama's SQLite manifest can't be pointed at `mlx_lm.server`
honestly; Ollama remains a provider via `/api/tags`. All
existing scan defenses apply per-source unchanged: symlink-as-
noise, dotfile skip, depth cap at 8. A known consequence:
standard HF cache layouts use symlinked snapshot files
(`snapshots/<sha>/<file> -> ../../blobs/<hash>`), so those
folders surface as empty rather than `transformer-folder`
entries; an HF-cache-aware scanner that resolves the target
back into the source's own `blobs/` dir is roadmap. Cargo
suite at 518 (509 + 9 D50 tests: source-tag/serde sync,
source-prefixed id emission, id round-trip via
`parse_inventory_id`, first-colon split, unknown-prefix and
empty-prefix rejection, missing-external-dir → None,
multi-source merge, per-source symlink refusal, dotfile skip
on external sources; plus 1 commands-layer test pinning the
unknown-prefix → stale behaviour). External-source env
overrides (`PLUME_LOCALLY_AI_MODEL_DIR`,
`PLUME_LM_STUDIO_MODEL_DIR`) are test-only entry points; the
production paths win when the env is unset. Tests touching
the env serialize on a module-local `d50_env_mutex`.

Slice D58 adds `PLUME_MLX_PYTHON`, an env override the MLX
supervisor honors when picking the Python interpreter to spawn
`mlx_lm.server` under. New helper `resolve_python_program()` in
`providers/mlx_lm/process.rs` is the single resolution point:
`PLUME_MLX_PYTHON` set + non-empty after `trim` → that value is
used as `MlxLmCommand.program`; unset / empty / whitespace-only
→ falls back to bare `"python"` (the pre-D58 default). Value
taken verbatim — no `~`/env expansion, no executable
pre-check (`Command::spawn` already surfaces clear OS errors
via `StartError::Spawn`).

The motivation is the LaunchServices-bare-PATH gotcha: when
Plume launches from Finder / Spotlight / Dock, it inherits a
PATH that doesn't include an activated venv's `bin/`, so a
`python` on PATH that has `mlx_lm` importable in the user's
shell isn't visible to Plume's child. Setting
`PLUME_MLX_PYTHON=~/.venvs/mlx-env/bin/python` (the resolved
absolute path — Plume does not expand `~`) lets Plume spawn the
venv's interpreter directly. No shell activation required.

Cargo suite at 528 (525 + 3 D58 tests): env override changes
`program` while `args_prefix` stays `-m mlx_lm server`; empty
and whitespace-only env values fall back to default; leading
and trailing whitespace on a real path are trimmed (a trailing
`\n` from copying out of a terminal survives the round-trip).
Tests touching the env serialize on a module-local
`d58_env_mutex`; the existing
`default_command_uses_non_deprecated_subcommand_form` test now
also takes the mutex + removes the env var to pin the unset
default. Docs: `MLX_RUNTIME.md § PLUME_MLX_PYTHON` (new
section) documents the resolution rules; `MANUAL_TESTING.md §
MLX server` cross-references it as the recommended GUI-launch
setup. PLUME_FULL_VERIFY=1 clean.

Slice D59 was the first real Plume-managed MLX smoke with a
supported text-only model (no PR): after downloading
`mlx-community/Qwen2.5-Coder-3B-Instruct-4bit` into
`plume-models/`, the D50 scanner found it as a
`plume-model-dir` row, D36 classified it as `mlx-folder`, D58
resolved `PLUME_MLX_PYTHON=~/.venvs/mlx-env/bin/python`, D40
spawned the venv-backed `python -m mlx_lm server`, `/health`
returned 200, D46 auto-selected the row, and D52 diagnostics
showed the bound port. A direct curl against the supervised
MLX-LM port returned a real Qwen chat completion, proving the
runtime path and OpenAI-compatible wire shape. The in-app chat
round-trip was blocked by a frontend layout bug: once a model
was selected and the mode-card grid unfolded, the chat textarea
and send bar collapsed out of view between the attach row and
the cards.

Slice D60 fixes that D59 UI blocker. The chat form is now a
non-shrinking flex item, the transcript is the part allowed to
compress and scroll, and its minimum height is lowered so the
textarea + send bar keep their intrinsic height when the
selected-model mode cards are visible below the chat panel.
D60 also removes stale "Ollama only" copy from the selected
agent-workspace subtitle / Chat mode card now that D45 routes
through Plume-managed MLX, and lets the Local models row
controls wrap instead of visually colliding with the kind/source
badges on narrow sidebars. Frontend-only; no IPC or Rust
changes.

Slice D61 adds the first frontend test runner. Vitest runs in
`happy-dom` with Testing Library matchers from
`src/test/setup.ts`; `npm run test` and `npm run test:watch`
are the new package scripts, and `scripts/verify.sh` now runs
`npm run test` in the Frontend block whenever `node_modules/`
is present. The initial tests pin two recent frontend-only
surfaces: `mlxLogPatterns.test.ts` covers D57's four log-hint
classifiers plus benign logs, and `ChatPanel.test.tsx` renders
the chat panel in no-selection, MLX-without-handle, and
MLX-with-running-handle states. The running-handle test asserts
the textarea is visible, becomes sendable after typing, and
the `.plume-chat-form` computed flex contract stays
`0 0 auto`, so reverting D60's anti-collapse rule fails the
unit test. No live `mlx-lm`, no Tauri window, no provider
daemon in Vitest; real model latency/memory remains a manual
smoke concern.

Slice D63 is docs/research only. New
`docs/HERMES_AGENT_RESEARCH.md` records a clean-room source pass
over the public `NousResearch/hermes-agent` repo, selected
Teknium PR/issue writeups, public Hermes docs, and the user's
local Hermes backup shape. The doc names what was read and what
was not read, keeps secrets out of scope, and extracts
behavior-level lessons for Plume: typed stream events, scoped
progressive tool disclosure, SQLite/FTS session history, memory
provider lifecycle hooks, prompt cache tiers, observer telemetry,
remote readiness/socket guardrails, real browser UI regression
tests, capped previews versus full logs, and native local-model
setup. It also proposes follow-up Plume slices for rendered UI
smoke, session-store design, typed agent events, memory provider
lifecycle, tool disclosure, observer telemetry, and a model
capability registry. No code or IPC changed.

Slice D64 lands the first writing verb of the distillation
track: `memory.distillApply`. D48 scaffolded the duplicate-group
preview, D54 wired it to `memory.distillPreview` plus a read-only
"Find duplicates" disclosure; D64 turns that preview into an
opt-in compaction. The shared `build_distill_preview` pass is
re-run INSIDE the memory mutex at apply time, the confirmed
`groupIds` are intersected with the live groups, and each matched
group keeps its newest entry while the older duplicates are
removed via the same atomic temp→rename rewrite `forget` uses.
Survivors keep their on-disk order. A `groupId` that went stale
between preview and apply (a `remember`/`forget` changed the
membership-stable group hash) is a no-op returned in
`unmatchedGroupIds`, never an error and never a wrong-entry
delete; an empty id list is a clean no-op. The response carries
`{ removedEntryCount, remainingEntryCount, unmatchedGroupIds }`.
The Memory panel's distill disclosure grows a "Compact N
duplicates" button next to Refresh; the previewed group list is
the confirmation surface (each row shows the surviving newest
text), apply resyncs the index + chat-context badge + preview,
and the outcome shows inline. No undo in v1 — the JSONL stays
hand-editable; the LLM-summary v2 (and its pre-apply snapshot)
remains roadmap per `docs/MEMORY_DISTILLATION.md`.

Slice D65 is a behavior-neutral decomposition: D64 had pushed
`src-tauri/src/memory/mod.rs` past the red 1200-line cap, so the
whole distillation layer (preview + apply + their types +
`normalize_for_distill` / `distill_group_id`) moved into a new
`memory/distill.rs` submodule. It reaches the parent's private
helpers (`memory_mutex`, `resolve_entries_path`, `read_entries`,
`serialize_entries`, `write_atomic`) via `super::`, and `mod.rs`
re-exports the production surface (`distill_apply`,
`distill_preview`, `DistillPreview`, `MemoryDistillApplyResponse`)
while the test module imports the test-only types straight from
`super::distill`. `mod.rs` drops from 1258 to 889 lines (amber),
`distill.rs` is 381. No IPC, behavior, or test assertion changed;
the full memory suite stays green.

Slice D66 makes distillation apply per-group. D64 compacted every
previewed group in one click; the backend's `distill_apply` already
honored whatever subset of `groupIds` it was handed, so this is a
frontend-only slice. The "Find duplicates" disclosure now renders a
checkbox on each duplicate group (default checked) plus a
Select-all / Clear-all toggle; the Compact button passes only the
checked group ids and its label reflects the selected removable
count (disabled at zero selected). Selection re-initialises to
"all checked" whenever the group set changes (a Refresh, or the
reshaped groups after a prior apply), keyed on the joined group-id
signature. New `MemoryPanel.test.tsx` (the first test for that
panel) pins two behaviors: unchecking a group sends only the
remaining id to `applyMemoryDistill`, and Clear-all disables
Compact without any IPC call.

Slice D67 is a frontend decomposition: D66 had pushed
`MemoryPanel.tsx` over the 800-line amber cap, so the distillation
disclosure (`DistillPreviewDisclosure` + its `DistillPreviewBody` /
`DistillGroupSelector` / `DistillGroupRow` children, the
`DistillState` type, and `distillApplyFailureLabel`) moved into a
new presentational `features/memory/MemoryDistill.tsx`. The
fetch/apply state stays in `MemoryPanel` and is passed down as
props. `MemoryPanel.tsx` drops 812 → 533 lines, `MemoryDistill.tsx`
is 298; `MemoryPanel.test.tsx` still drives the moved components
through the panel unchanged, so the split is behavior-neutral.

Slice D68 makes `PLUME_FULL_VERIFY=1` clippy-clean. The
`MemoryPressure::derive` heuristic and its `Normal` / `Warn` /
`High` verdicts are only constructed by the macOS backend
(`system::macos`); on other targets the snapshot reports `Unknown`,
so clippy flagged them as dead code on the Linux CI build. The fix
is a target-gated `#[cfg_attr(not(target_os = "macos"),
allow(dead_code))]` on the enum and the `derive` method — the lint
stays live on macOS (where the variants must remain wired) and is
suppressed only off-Apple. No behavior change; the cross-platform
`#[cfg(test)]` pressure tests still cover the heuristic everywhere.

Slice D69 adds a distillation audit log — the "never hide memory
writes" trail for the one memory verb that deletes data the user
didn't name individually. Every `distill_apply` that removes ≥1
entry appends a record (`{tsMs, rule:"dedupeExact", removedIds,
keptIds}`) to `.plume/memory/distill-log.jsonl`. The write is
best-effort inside apply (the entries rewrite already committed, so
a log failure traces via `tracing::warn!` but never undoes the
compaction or fails the verb), symlink-safe through the shared
`resolve_memory_file` resolver, and bounded to the newest
`DISTILL_LOG_MAX_RECORDS` (50) on each append. New read verb
`memory.distillLog` returns the records newest-first behind the same
trust gate as `memory.index`; it landed backend-first (registered +
six Rust tests) with the UI surface reserved for a follow-up. The
shared resolver also factored `resolve_entries_path` through
`resolve_memory_file` so the entries store and the log honor the
same planted-`.plume` symlink guard.

Slice D70 surfaces that audit log in the UI. The Memory panel's
"Find duplicates" disclosure now fetches the preview and the log
together (`Promise.all`, shared trust gate) and renders a read-only
"Recent compactions" list below the selector — `N duplicates
removed · <relative time>` per record, newest first — visible in
every ready state (so the history shows even once no duplicates
remain). A re-applied compaction refetches both, so the history
updates in place. `MemoryDistill.tsx` grew the `DistillLogList` +
`formatRelativeTime` helper; `MemoryPanel.test.tsx` gains a third
case asserting the log row renders. The `memory.distillLog` verb is
now reachable end-to-end.

Slice D71 opens the curated memory topic-files layer from the North
Star — the human-authored Markdown beyond the flat entries store.
New `memory/topics.rs` reads `.plume/memory/INDEX.md` / `USER.md` /
`SOUL.md` (always-loaded "prompt fuel", 2 KiB cap each) plus
`topics/*.md` (8 KiB cap, sorted, capped to 32 files), behind the
same trust gate, process-wide memory mutex, and planted-`.plume`
symlink guard as the entries store. Reads are bounded (at most
cap+1 bytes per file, keeping the valid UTF-8 prefix so a cap that
lands mid-character can't panic or corrupt), a symlinked core file
refuses while a symlinked `topics/*.md` is skipped, and the core
trio is always returned even when missing so the panel surfaces the
convention. New read verb `memory.topics`, a self-contained
"Topic files" disclosure in the Memory panel (per-file expandable
content), and 10 Rust + 1 frontend tests. Plume does not write these
in D71 — the user authors them in their editor; wiring the
always-loaded trio into the chat prompt context (like entries via
`read_for_prompt`) is the reserved D72 follow-up.

Slice D72 makes the curated trio actual prompt fuel. New
`memory::read_core_for_prompt` projects the existing, non-empty
`INDEX.md` / `USER.md` / `SOUL.md` within a 6 KiB budget
(`TOPICS_CONTEXT_BYTE_CAP`, independent of the 4 KiB memory-entry
budget), mirroring `read_for_prompt`. `prompts::assemble` folds them
into one `system` message inserted ABOVE memory entries and BELOW
AGENTS.md — final order: mode pin, AGENTS.md, topic files, memory
entries, turns (durable curated context over incremental notes, both
under the project contract). Honest skip on any failure, same posture
as memory. A `TopicsSummary` rides on `AssembledPrompt` /
`ContextPreview` and is echoed end-to-end through `chat.send` and
`chat.context` as `topics: ChatTopicsUsage | null` (the proven
`MemorySummary` plumbing, mirrored across `send.rs` / `context.rs` /
`lib/api/chat.ts`). Backend-first: the data flows but the chat-header
badge is a reserved follow-up. Six new `assemble` tests pin the
injection, ordering, skips, and cap; full Rust suite 548 green, clippy
clean, tsc + frontend green.

Slice D73 renders that reserved badge. A new `TopicsBadge` (sibling
to `MemoryBadge` in `InstructionsBadge.tsx`) shows
`✱ Topics · N files · K B` in the chat header — "available" from the
`chat.context` preview before a send, "included" from the `chat.send`
response after, hidden on the honest-skip `null`. `useChat` now tracks
`lastTopicsUsed` (set from `response.topics`, reset on `clear`)
alongside `lastMemoryUsed`. Frontend-only; two new `ChatPanel` tests
pin that the badge shows with topic data and stays hidden without. The
topic-files arc (D71 read → D72 inject → D73 badge) is complete and
visible end-to-end.

Slice D74 is a behavior-neutral decomposition: D72 had pushed
`prompts/assemble.rs` over the 800-line amber cap, so the three pure
system-message builders (`make_memory_message`, `make_topics_message`,
`make_instructions_message`) moved into a `#[path]` sibling
`assemble_messages.rs` (mirroring the existing `assemble_tests.rs`
convention), exposed `pub(super)` and imported back. `assemble.rs`
drops 863 → 755 lines (off amber); the preamble wording and the 99
prompts tests are unchanged.

Slice D75 addresses findings from a fresh-eyes review pass (four
subagents over the D64–D74 diff; the Rust backends and the Rust↔TS
wire contracts came back clean — including the checked-and-cleared
`distill_apply`/audit-log non-reentrant-mutex deadlock concern). Two
real frontend UX bugs are fixed: (H1) the "Removed N duplicates"
success notice was wiped instantly because `fetchDistill` cleared
`distillNotice` and `onApplyDistill` calls it to resync after setting
the notice — `fetchDistill` no longer clears it, and a new
`onRefreshDistill` clears it only on a manual rescan; (H2) the distill
preview and audit log were fetched with one `Promise.all`, so a
corrupt `distill-log.jsonl` would flip the whole disclosure to error —
the log read now degrades to `[]` on failure so a secondary-history
error can't sink the essential preview + Compact action. Two frontend
regression tests pin both (they fail on the old code), plus two
review-suggested backend tests (the `read_core_for_prompt`
budget-overflow skip branch and a duplicated-confirmed-id apply).
Full Rust suite 550 green, frontend 16, `PLUME_FULL_VERIFY` OK.
Deferred (review M1, low): the distill/topics fetch handlers lack the
unmount-cancellation guard the search effect uses — a React no-op
warning, not a crash; left for a follow-up.

Slice D76 clears the repo's last "red" oversized file. About 740 of
`providers/local_models.rs`'s 1279 lines were an inline `#[cfg(test)]
mod tests`; that module moved to a `#[path]` sibling
`local_models_tests.rs` (the same convention `local_model_details.rs`,
`memory_tests.rs`, and `assemble_tests.rs` already use). Production
`local_models.rs` drops 1279 → 538 lines (off red, off amber); the 30
tests are byte-identical and green. The repo now has 0 red files —
remaining size warnings are pre-existing amber code files and two
doc soft-caps.

Slice D77 begins the agent-loop track with its foundation: the
session autonomy-config substrate the IPC roadmap reserved (the
`session.setMode` / `session.setApprovalPolicy` / `session.setAllowlist`
/ `session.state` verbs that were hardcoded to `ask-each` + empty
allowlists). New `src/agent/` module models the two independent axes
from `docs/SAFETY.md` — `agentMode` (`chat`/`propose-diff`/
`scoped-edit`/`agent-loop`) and `approvalPolicy` (`ask-each`/
`ask-on-write`/`ask-on-fail`) — plus the explicit `fileAllowlist` /
`commandAllowlist` / `iterationCap` the higher modes require. It is
pure state + validation (no tool execution, no model, no loop
controller yet). The fail-closed invariant is enforced:
`AgentConfig::validate` makes an `agent-loop` config invalid without
a non-empty file allowlist, a non-empty command allowlist, and an
iteration cap, and every setter validates the *resulting* config and
commits only if valid (a locked read-modify-validate-write), so the
session can never be left half-configured into autonomy. The config
is window-scoped state in `AppState`, reset to the least-privilege
default on every `project.open` so a project's project-relative
allowlists never leak into the next. `fileAllowlist` entries are
path-safety validated (no absolute / `..` / NUL); the verbs are
**not** trust-gated (they touch no disk, only declare intent — the
gated actions are trust/approval-checked when they run). Backend-only
(22 Rust tests across `agent_tests.rs` + `session_tests.rs`); the
frontend mode/policy UI and the loop controller are the next slices.

Slice D78 (agent-loop slice 2) adds the approval **decision core** —
the pure logic from `docs/SAFETY.md § approvalPolicy` that decides
whether the agent's next action runs silently or stops to prompt. New
`agent::approval`: `normalize_command` (rejects empty / blank-program /
`env`-wrapper argv; keeps trailing args verbatim so `npm test` ≠
`npm test --watch`), an in-memory `ApprovalLedger` keyed by normalized
argv, and `decide(policy, request, ledger, run_state)`. The three
policies are modelled faithfully and conservatively: `ask-each` always
prompts; read-only tools run silently under `ask-on-write` /
`ask-on-fail`; **writes always prompt**; an approved command runs
silently on its first run this session, a repeat re-prompts under
`ask-on-write` (the doc's "re-approve every loop iteration" case), and
`ask-on-fail` relaxes that only for the verifier-retry of a
just-*failed* command. Hard guarantees pinned by tests: no policy ever
grants first-run permission to an un-ledgered argv (not even
`ask-on-fail` on a retry), and `ask-each` never auto-runs. Pure +
unit-tested (14 tests); PATH/binary resolution, the persistent
`.plume/approvals.toml` ledger with expiry + binary-match, and the IPC
wiring are deferred to a follow-up — the module is `allow(dead_code)`
until the loop controller (slice 3) consumes it. Full suite 586 green,
clippy clean, `PLUME_FULL_VERIFY` OK.

Slice D79 (agent-loop slice 3) adds the bounded **loop controller** —
the read/edit/test/fix driver. New `agent::controller::run_loop` runs
an abstract step up to the iteration budget (`iterationCap`) and stops
on the first terminal condition: the step reports `Done`, the step
`Paused` for the user (an approval prompt or question), the step
`Failed` (fail-closed — the loop never self-retries), the user aborted
(checked *before* each iteration so the one-key abort stops promptly
without interrupting a step mid-flight), or the budget is exhausted.
The result is a `LoopReport { outcome, iterations_run }` with a tagged
`LoopOutcome` union for a future `agent.*` event. Pure control flow —
the step is a caller-supplied closure and abort is a predicate, so it's
unit-tested with fakes (10 tests: budget exhaustion, 1-based iteration
numbering, done/failed/paused precedence, abort before/between
iterations, zero budget, serialization). This completes the pure
agent-loop foundation (config → approval → controller);
`allow(dead_code)` until slice 4 wires the real step (drive the model,
classify the tool request through `agent::approval`, execute a
read/patch/command) + the IPC/UI, which needs a live model to verify.
Full suite 596 green, clippy clean, `PLUME_FULL_VERIFY` OK.

Slice D80 fills the memory panel's one remaining gap (it advertised
"only add/remove"): `memory.update` edits an entry in place. The
backend `memory::update` mirrors `remember`'s validation + secret
redaction + per-entry/total caps on the new text, but replaces an
existing entry by id while preserving its `id` and `created_ms` (an
edit fixes wording, it doesn't mint a new fact or reorder recency); a
well-formed id matching no entry is `notFound` (vs a malformed `badId`),
and it's symlink-safe and trust-gated like the other write verbs. The
Memory panel rows grow an Edit button → inline textarea with Save/Cancel
(the row leaves edit mode on success, stays showing the error on a
rejected edit). 6 Rust + 1 frontend test. Not an agent-execution
concern — this is the "bank a clean, fully-verifiable feature while
review/runtime aren't available" slice (the agent-loop track resumes
once its safety-critical foundation can be reviewed). Full suite 602
green, frontend 17, clippy clean, `PLUME_FULL_VERIFY` OK.

Slice D81 closes the review M1 follow-up deferred in D75: the
event-driven distill/topics fetch handlers (`fetchDistill`,
`onApplyDistill`, `MemoryTopicsDisclosure.fetchTopics`) can't use the
search effect's cleanup flag, so they now carry a `mountedRef` and skip
their post-`await` state writes if the panel unmounted mid-request —
matching the search path's cancellation posture. Frontend-only,
behaviour-neutral while mounted (the 17 tests are unchanged and green);
also de-duplicated a stale `fetchDistill` doc comment. The agent-loop
track's review-and-resume status is unchanged.

Slice D82 addresses Codex's review of the loose branch (two findings):
(MEDIUM) the `distillApply` audit log is a best-effort append after the
entries rewrite has already committed, so "never hide memory writes"
wasn't fully true if the append failed — `distillApply` now reports
`auditLogged: bool` (false = removed-but-unrecorded), the panel appends
"(not recorded in the audit log)" to its notice, and a no-op apply
reports `true`. (LOW) `read_distill_log` / `append_distill_log`
dereferenced a symlinked final `distill-log.jsonl` (the resolver only
guarded the `.plume` / `.plume/memory` dirs); both now `refuse_symlink`
on the final file, and the same guard was extended to
`read_entries` (`entries.jsonl`) since every memory reader funnels
through it — closing the identical class for the whole store. Four new
Rust tests (audit-logged true/false, both final-file symlink
regressions) + one frontend test (the unrecorded-compaction notice).
Full suite 606 green, frontend 18, clippy clean, `PLUME_FULL_VERIFY` OK.

Slice D83 (agent-loop slice 2b) lands the **persistent approval ledger**
— the on-disk follow-up D78 deferred. New `agent::ledger` records the
command identities the user has approved at `<project>/.plume/approvals.json`
so a later `ask-on-write` / `ask-on-fail` run can skip the prompt. Each
record is camelCase JSON (`serde_json` is already a dep — no `toml`, no
date crate, no new download) carrying the normalized `argv`, the
resolved binary's `basename` + absolute `binary` path, `createdMs` /
`updatedMs`, an `expiresMs` (90-day default; `null` = never), and
`approvedBy` (`"user"`; `"agent"` reserved, not honored). Safety
properties match `docs/SAFETY.md § Approval ledger`: env wrappers reuse
`approval::normalize_command` so they can never be recorded; a lookup
re-resolves the program and reports `BinaryMismatch` if the absolute
path moved or no longer resolves (never auto-updates); a lookup at/past
`expiresMs` reports `Expired`; the `.plume` dir and `approvals.json`
file are `refuse_symlink`-guarded; a corrupt file is fail-safe (treated
as empty, replaced on next write, bytes left for manual recovery);
writes are atomic (temp + rename), and the store is capped at
`MAX_RECORDS` (256). PATH resolution is abstracted behind a
`BinaryResolver` trait (`PathResolver` in prod, a deterministic
`MapResolver` in tests). Pure + unit-tested (17 tests: first approval,
reload, re-approval keeps `createdMs`, binary mismatch + unresolvable,
revoke + no-op, env-wrapper rejection at approve and lookup,
unresolvable-binary rejection, expiry boundary, corrupt/empty/missing
recovery, both symlink refusals, max-records cap). No consumer yet —
`allow(dead_code)` until the loop controller + an `approvals.*` verb
wire it up, which needs a live model to verify. Full suite 623 green,
clippy clean, `PLUME_FULL_VERIFY` OK.

Slice D84 gives the D77 agent-autonomy config a **visible settings
surface**. New `AgentSettingsPanel` (a compact left-column card, peer of
Memory, in the trusted-project workspace) reads `session.state` on mount
and drives the four `session.*` verbs through a typed
`src/lib/api/session.ts` wrapper: mode and approvalPolicy apply
immediately (one verb each), the file/command allowlists + iterationCap
are edited as text and committed together with "Apply gates". The
backend stays the source of truth — every setter validates the
*resulting* config and the panel mirrors only what the backend commits,
so the fail-closed rule shows through verbatim: flipping to `agent-loop`
without gates is refused and the panel lists every broken invariant
inline while the select reverts to the committed mode (no optimistic
half-configured autonomy). Command lines tokenize on whitespace into the
argv vectors the backend stores; a blank cap means "no cap"; a
non-numeric cap blocks Apply with an inline hint. No tool execution —
this only declares intent; the gated actions are trust/approval-checked
when they run, in later slices. The panel is a new toggleable inner
panel (`useInnerPanels` gains `agent`, visible by default). 5 frontend
tests (load→reflect, immediate mode flip, refusal surfaces reasons +
reverts, allowlist parsing on Apply, blank-vs-non-numeric cap). Frontend
23 green, build clean, `PLUME_FULL_VERIFY` OK.

Slice D85 fixes the **agent event protocol** — the typed shapes a future
agent run streams to the UI. New `agent::events`: an internally-tagged
(`kind`) `AgentEvent` union (`messageChunk`, `toolProposed`,
`approvalRequired`, `toolStarted`, `toolFinished`, `toolFailed`,
`paused`, `done`) wrapped in an `AgentEventEnvelope { seq, tsMs,
#[flatten] event }` so a dropped/replayed frame is detectable, matching
the Hermes-style structured stream in `docs/HERMES_AGENT_RESEARCH.md`.
The tool lifecycle (`toolProposed` → optional `approvalRequired` →
`toolStarted` → `toolFinished`|`toolFailed`) shares a `callId` for
collapsing into one row; `paused`/`done` mirror `LoopOutcome`'s tags.
Events carry only *descriptive* fields — an `approvalRequired` reports
the stop, it never pre-authorizes (the decision stays the gate's call).
Frontend mirror in `src/lib/api/agentEvents.ts` (discriminated union on
`kind`) + a presentational `AgentEventLog` renderer skeleton (one row
per frame, keyed by `seq`, tinted by lifecycle stage). Scaffold only —
no channel emits these and no screen mounts the log yet
(`allow(dead_code)` backend); the executing slice wires a real stream
into shapes both ends already agree on. 7 Rust tests (each variant's
wire shape + round-trip, envelope flatten, all tool kinds, approval has
no decision field) + 4 frontend (empty state, ordered rows by kind,
failed-row tint, bare done). Full suite 630 Rust green, frontend 27,
clippy clean, `PLUME_FULL_VERIFY` OK.

Slice D86 designs **progressive tool disclosure** — how a local model
sees a small core toolset and retrieves the long tail by search, so the
serialized tool surface stays ~constant no matter how many plugin / MCP /
connector tools exist. New `docs/TOOL_DISCLOSURE.md` (the doc the Hermes
research pass reserved; clean-room — the *idea* is borrowed, none of
Hermes' code/scoring) plus a pure `agent::catalog` scaffold:
`ToolTier` (Core / Optional), `ToolParam`, `ToolSpec`, and a stateless
`ToolCatalog` with `core()` / `optional()` / `search(query, limit)` /
`visible_specs(query, limit)`. Search ranks **only optional** tools
(core is already in the prompt) by a deterministic, case-insensitive
weighted substring/token scan over name (exact > prefix > substring) +
summary + param names — no BM25, no index, no embeddings; the catalog is
rebuilt from live definitions each assembly so there's no stale state.
`visible_specs` = core ⧺ search hits, the one call the assembler makes.
Hard line, documented + tested: the catalog is a **presentation**
concern (what the model may see), never **authorization** (whether a
tool may run — that stays the approval/allowlist gate's call). Scaffold
only: no assembler consumes it, no MCP, no execution (`allow(dead_code)`).
11 Rust tests (tier split, search excludes core, ranking ladder,
summary/param matches, multi-token accumulation, limit + ordering,
blank/zero/no-match, visible-set composition, case-insensitivity). Full
suite 641 Rust green, clippy clean, `PLUME_FULL_VERIFY` OK.

Slice D87 is a **UI cleanup pass** — declutter the center zone and fix
the local-model row layout. The agent workspace used to render a
four-card grid naming every safety mode plus a docs footnote below the
chat panel; those cards were descriptive only (the real controls live in
the chat header's response-mode toggle and the left-column Agent card
from D84), so they're removed — the center is now a one-line orientation
sentence, the selected-model banner, and the chat panel. The Local
models row is restructured: the actionable header is now just caret +
name + the Start/Stop (or running) controls on a single `nowrap` row, so
a model that is both selected and running no longer wraps its "selected"
badge / port / Stop under the name; the descriptive kind / source / size
badges drop to a quiet meta line below that wraps freely. Dead mode-card
CSS removed. 7 new frontend tests (4 AgentWorkspace: textarea stays
visible, mode-card grid + footnote gone, orientation copy, calm empty
state; 3 LocalModelsPanel: header/meta split, selected+port+Stop in one
controls cluster, header + controls `nowrap` in CSS). Frontend 34 green,
build clean, `PLUME_FULL_VERIFY` OK.

Slice D88 syncs the docs to the D83–D87 batch. `docs/SAFETY.md §
Approval ledger` rewritten to the shipped JSON format (D83); the stale
`approvals.toml` references in `docs/ARCHITECTURE.md` and the
`agent::approval` module comment corrected to `.json`. `docs/MANUAL_TESTING.md`
gains an **Agent autonomy settings (D84)** walkthrough (load, immediate
mode flip, fail-closed agent-loop refusal, Apply gates, cap validation,
per-project reset) plus a note that the D85 event transcript and D86 tool
catalog are typed scaffolds with no manual surface yet. **IPC_CONTRACT
unchanged** — no wire changed this batch: D84 reused the existing
`session.*` verbs (documented since D77), and D83/D85/D86 are internal
Rust / scaffold types with no registered verb. AGENTS.md status entries
(these paragraphs) landed per-slice. No new design doc beyond
`docs/TOOL_DISCLOSURE.md` (D86).

PR #76 review (Codex) fixes: (MEDIUM) `commandAllowlist` validation
accepted env-wrapper commands (`env A=1 npm test`, a leading `KEY=VAL`
token) that the approval / ledger layer rejects, so the D84 settings UI
could commit a command identity the gate would never honor.
`validate_allowlist_argv` now reuses `approval::normalize_command`, so
the allowlist refuses exactly what the gate refuses (3 new Rust tests:
`env …`, `FOO=1 npm`, absolute `/usr/bin/env`). (LOW) D87 removed the
mode cards but several current-state docs still described them as
present — corrected in `docs/ARCHITECTURE.md`, `docs/UI_STYLE.md`,
`docs/AGENT_OPERABILITY.md`, the AGENTS.md file map, and the `chat.css`
header comment (historical slice-log entries and the forward-looking
Simple-Mode design prose are left as-is). 642 Rust green, frontend 34,
`PLUME_FULL_VERIFY` OK.

Slice D89 is a **UI rescue pass** — make the trusted view feel real
instead of theoretical. The center-zone "Selected model" banner used to
hedge ("…no chat, no loading, no downloads happen yet") back when chat
wasn't wired; that copy is gone, and the banner now carries inline
**Start / Stop / running** controls for a selected Plume-managed MLX
model (reusing the same `useMlxServers` bus the Local models panel
drives), so the model you're about to chat with can be brought online
from the chat zone instead of hunting in the left column. The empty
state is a single point-at-the-panels sentence; the app hero subtitle
drops "early scaffold". (The Local models row overlap was already fixed
in D87's header/meta split + `nowrap`.) Frontend/CSS only, plus the one
`mlxServers` prop the banner needed. 5 new frontend tests (stale copy
gone, provider·model display, idle→Start calls the bus, running shows
port + Stop, error re-offers Start). Frontend 39 green, build clean,
`PLUME_FULL_VERIFY` OK.

Slice D90 adds the **scripted, UI-free Qwen MLX chat smoke** —
`scripts/smoke-qwen-mlx.sh`, the "does my local Qwen actually answer?"
one-command proof of the local-first happy path. No computer-use, no UI
driving, no Ollama, no downloads. It resolves the interpreter the way
the MLX supervisor does (`PLUME_MLX_PYTHON` → `~/.venvs/mlx-env` →
`python3`/`python`, accepting only one that can `import mlx_lm`),
auto-discovers a Qwen checkpoint under `$PLUME_MODEL_DIR` /
`<repo>/plume-models` (preferring Qwen2.5-Coder-3B-4bit), then hands off
to the D53 `smoke-mlx-runtime.sh` — the closest UI-free mirror of the
supervisor's spawn → `/health` → `/v1/chat/completions` → shutdown
path — and prints a single `SMOKE: PASS` / `FAIL` banner with
diagnostics. Verified structurally in-container (Linux): graceful FAIL
at the interpreter step with the venv playbook, and — with a stub
`mlx_lm` on `PYTHONPATH` — correct interpreter resolution, Qwen
discovery (the Coder-3B-4bit preference), and handoff. **A real PASS
requires Apple Silicon + the venv + the model**, so it can only be
confirmed on the user's Mac; that is documented, not hidden. Documented
in `docs/MANUAL_TESTING.md § Qwen MLX chat smoke`.

Slice D91 adds the **Qwen propose-diff model-quality smoke** — can a
local 3B/4-bit model produce a diff that survives Plume's *own* patch
path? The cycle (validate → apply → revert) is exercised through the real
`validate_patch` / `apply_patch` / `revert_patch` entry points, never a
reimplementation. Because plume is a bin-only crate (no lib target for an
example/integration test to link), the cycle lives in a bin-internal test
file `patch/propose_diff_smoke_tests.rs`: three **non-ignored** tests run
in the normal suite and prove the orchestration on hand-authored diffs in
a temp fixture (valid → applied + reverted, with disk restored to seed;
invalid → reported, disk untouched; pre-image mismatch → apply fails +
rolls back), plus one `#[ignore]`d `qwen_propose_diff_smoke` that reads a
model diff + seeded fixture from the env. The harness
`scripts/smoke-qwen-propose-diff.sh` seeds `greet.py`, starts mlx-lm,
asks Qwen for only a unified diff, captures + de-fences it, and drives the
ignored test. An invalid/non-applying diff is a clearly-reported
MODEL-QUALITY fail, never a half-applied tree (apply runs only after
validate and is all-or-nothing with a pre-apply checkpoint). Verified
in-container: 3 cycle tests green, and the ignored test driven with a real
valid diff reverts the fixture to seed. The model half needs Apple
Silicon (documented). `docs/MANUAL_TESTING.md § Qwen propose-diff smoke`.
645 Rust green (3 new + 1 ignored), clippy clean, `PLUME_FULL_VERIFY` OK.

PR #77 review (Codex) fixes: (MEDIUM) the D89 banner rendered MLX
**Start** in the no-project chat shell, regressing D49's rule that
chat-only mode can't start a Plume-managed runtime (the supervisor gates
`providers.startServer` on a trusted project). The banner gained a
`noProject` prop — `NoProjectChatView` passes it — that disables Start
with the same "open and trust a project" hint the Local models panel
uses, while keeping Stop / running live (Stop is an ungated cleanup
verb). 2 new frontend tests (Start disabled + model still shown; Stop
still works). (MEDIUM) the D90/D91 chat round-trips had no request
timeout, so a model that became healthy but hung during generation (the
Gemma class) could stall the smoke forever despite the PASS/FAIL
promise. `smoke-mlx-runtime.sh` and `smoke-qwen-propose-diff.sh` now
bound the chat curl with `--max-time "$CHAT_TIMEOUT"` (default 60 / 90 s)
and report the `curl` exit-28 timeout with a log tail; `smoke-qwen-mlx.sh`
forwards `CHAT_TIMEOUT`. Verified the timeout path in isolation
(health 200 + hanging POST → rc 28 detected cleanly). Frontend 41 green,
`PLUME_FULL_VERIFY` OK.

Slice D92 wires a **read-only tool-catalog IPC** over the D86 scaffold —
`tools.list` / `tools.search`. New `agent::catalog::builtin_catalog()`
provides Plume's concrete core/optional split as data (file read/search,
patch validate/apply/revert, memory, verifier, stop are core; GitHub /
Hugging Face / browser / computer-use are optional). `commands::tools`
exposes two unprivileged pure reads (no trust gate, no disk, no
execution, no MCP): `tools.list` returns every tool with its `tier`;
`tools.search` returns `core` (always) plus ranked `matched` **optional**
hits — never a core tool, the progressive-disclosure scoping the catalog
promises. Search rejects `limit` outside `1..=50` and a query over 256
bytes with `BadArgument` (mirroring `memory.search`). `ToolTier` /
`ToolParam` / `ToolSpec` gained camelCase `Serialize`; the handlers wrap
sync `list_response` / `search_response` cores so the logic is unit-
testable without an async runtime. Listing/finding a tool grants
*visibility*, never permission to run it — there is no execution verb
(the executor + approval gate are a later slice). Backend-first: a typed
`src/lib/api/tools.ts` wrapper lands for a future panel, but no UI
consumes it yet. 8 Rust tests + 2 frontend (wrapper call shape).
`docs/IPC_CONTRACT.md § tools` documents the wire. 654 Rust green,
frontend 43, clippy clean, `PLUME_FULL_VERIFY` OK.

Slice D93 is the **agent event dry-run** — a plumbing proof that the
typed D85 event stream drives the existing `AgentEventLog` surface with
no real tools. New `agent::dry_run::scripted_dry_run(now_ms)` returns a
deterministic sequence walking every event kind (message chunks, a tool
lifecycle that auto-runs, one that stops for `approvalRequired` then
runs, a `toolFailed` + `paused`, and a terminal `done`), with 0-based
strictly-increasing `seq` and shared `callId`s. `agent.dryRun`
(`commands::agent`) is an unprivileged pure read that hands back the
stream — no model, no shell, no patch, no file writes. Frontend:
`src/lib/api/agent.ts` wrapper + a new `AgentDryRunPanel` (mounted under
the left-column agent inner panel, peer of the settings card) whose
"Run dry-run" button fetches the stream and renders it through the
unchanged `AgentEventLog`. 8 Rust tests (seq ordering, single terminal
`done`, every kind covered, each proposed tool reaches a terminal
lifecycle event, approval precedes start, determinism; + command
wire-shape × 2) and 3 frontend (empty log, fetch→render the typed
stream, IPC-error surfaced). `docs/IPC_CONTRACT.md § agent` documents the
wire. 661 Rust green, frontend 46, clippy clean, `PLUME_FULL_VERIFY` OK.

Slice D94 syncs docs/status to the D89–D93 batch. A new **Local-first
positioning** section (top of this file) states the three things plainly:
MLX/Qwen is the current local-first proof path (the two smoke scripts);
Ollama is compatibility, not the happy path; and the tool catalog +
agent event loop are scaffolds with no execution until a real executor
lands behind an explicit gate. `docs/IPC_ROADMAP.md § Tools` updated to
mark `tools.list` / `tools.search` (D92) and `agent.dryRun` (D93) as
shipped read-only/dev-only surfaces, with a future `tools.invoke` named
as where gated execution lands. `docs/MANUAL_TESTING.md` gains an Agent
event dry-run walkthrough and the same Ollama-vs-MLX / scaffold framing.
`docs/IPC_CONTRACT.md` already carries the `tools` + `agent` wire (added
in D92/D93). Docs-only; `PLUME_FULL_VERIFY` OK.

Slice D95 is a post-merge portability fix. **The local-first path is
confirmed real on hardware:** on Apple Silicon both smokes PASS — Qwen
through Plume-managed MLX answers a chat (D90), and produces a unified
diff that Plume validates, applies to a temp fixture, and reverts (D91,
the ignored Rust smoke included). But the merged scripts used Bash 4's
`mapfile`, and stock macOS `/bin/bash` is **3.2** (`mapfile: command not
found`) — so they died before reaching the model. `smoke-qwen-mlx.sh`
and `smoke-qwen-propose-diff.sh` now collect the Qwen candidate folders
with a `while IFS= read -r` loop instead (identical behaviour, Bash-3.2
safe; `mapfile` was the only Bash-4-ism — `check-file-sizes.sh` was
already portable). Verified `bash -n` clean and that discovery + handoff
still work in-container.

Slice D96 is the **single-step local agent** — the first slice where the
scaffolds carry a real model turn. `agent.singleStep` (`commands/agent.rs`)
sends a propose-diff prompt to the selected, running MLX model (reusing the
`chat::mlx_lm` adapter), classifies the reply, runs the one safe action —
read-only `patch.validate`, which writes nothing — and surfaces *applying*
behind the D83 approval gate (`approval::decide` returns `Prompt` for any
write, so the run emits `approvalRequired` + `paused`). It then returns the
real D85 event stream, which the new **Run one step** panel
(`AgentSingleStepPanel`) renders via the existing `AgentEventLog`. The
decision logic is a pure, fully-tested core (`agent::single_step`:
`classify_action` + `build_single_step_events`); the command is the thin
I/O shell (model round-trip + the `validate_patch` → `ValidateSummary`
bridge). Gates kept verbatim: **never applies a diff, never runs a shell
command, never recurses, no computer use, no downloads, no Ollama path** —
an unsupported tool request (the documented `TOOL_REQUEST:` sentinel)
becomes a blocked `toolFailed`. Both autonomy axes gate it: it requires a
trusted project (`NeedsApproval`), a live MLX server (`NotFound`), and an
`agentMode` of `propose-diff` or higher — a step is refused with
`BadArgument` while the session is in `chat` (the mode axis controls what
the model may do, checked before the model is ever called; the frontend
also disables the **Run one step** button and explains why). The
end-to-end model path is
Mac-only, so in-container tests cover the pure core + wire shapes and the
real round-trip is exercised by the Qwen smoke scripts. `PLUME_FULL_VERIFY`
OK.

Slice D97 is the **in-app single-step verification** (no feature code). The
D96 `agent.singleStep` → `AgentEventLog` round-trip — authored and
unit-tested in the cloud but never run against a real Mac MLX server — was
confirmed end-to-end on Apple Silicon (2026-06-27). With a Plume-managed
`Qwen2.5-Coder-3B-Instruct-4bit` server running and Agent mode **Propose
diff**, a *modify* instruction in **Run one step** rendered the full
happy-path stream (`messageChunk` → `patch.validate` proposed / started /
finished → apply proposed → `approvalRequired` → `paused`), and a
*create-a-new-file* instruction drove the blocked path (`TOOL_REQUEST:
create-file` → `toolFailed`). Disk stayed untouched in both runs — no
apply, no checkpoint; the validate-only path writes nothing. The mode gate
(`chat` refuses with a disabled button + reason, `propose-diff` allows) was
confirmed live. Docs-only: a "Verified in-app (D97)" note added to
`docs/MANUAL_TESTING.md § Single-step agent`. No IPC, no code, no test
count change.

Slice D98 is a **UI rescue / operability cleanup** (frontend only, no IPC,
no Rust, no new agent capability). It makes the trusted-project workspace
usable before more agent features land. (1) **The left column scrolls.**
`.plume-workspace-left` is now `overflow-y: auto` with its panels held at
natural height (`> * { flex-shrink: 0 }`), so the Agent settings / Run-one-
step / dry-run cards (and a long agent event log) are reachable at any
window height instead of being clipped by the shell's `overflow: hidden`
(the D97 session had to maximize the window to see the event stream). The
inner-toggle strip is `position: sticky` so the panel chips stay reachable
mid-scroll; the file navigator caps at `--nav-max-height` (50vh) and scrolls
internally instead of `flex: 1`-filling (which collapses to nothing once
siblings overflow a scroll container). (2) **The Agent card is less
soup-like.** The dev-only event dry-run is tucked behind a collapsed
`<details>` disclosure, and the Agent settings allowlist/cap gates render
only for the modes that consume them (scoped-edit / agent-loop, plus
whenever a refused mode flip needs them fixed) — chat / propose-diff show
just mode + approval. (3) **The Local models row can't collide.** The row
header may wrap so the controls cluster (selected · port · Stop) drops to
its own line on a narrow column instead of overflowing the name, while the
cluster itself stays non-wrapping so the badges never scatter. (4) **The
center is the primary surface.** `.plume-agent-workspace` dropped its own
`ink-panel` border so the selected-model banner and chat panel are the only
cards — no card-inside-a-card. Frontend tests: a left-column scroll-contract
test, a "no conflicting Start/Stop/Selected" local-models test, and
gates-visibility tests; the single-step block/enable coverage is unchanged
and still green. `npm run test` 61 green, `npm run build` clean,
`PLUME_FULL_VERIFY` OK. Visual confirmation of the live layout is left to
review (no computer-use in this slice).

Slice D99 adds **read-only file context to the single step** — the
attach-a-file affordance the D96 prompt explicitly deferred ("this step
does not read files into the prompt yet"). `agent.singleStep` gains an
optional `attachment` field, the same `ChatAttachment` shape (`relPath` +
optional D10 line range) `chat.send` takes; the command folds the redacted
file into the step's final user message through the **same**
`prompts::apply_attachment` path the chat panel uses — `apply_attachment`
was made `pub` so both callers share one redact-then-slice-then-wrap
implementation. The folded content rides in the message, not the
8 KiB-capped `prompt`, so the 256 KiB attachment cap (and the secret
redaction / binary / hardlink / `.git` blocks) govern; a blocked attachment
rejects with the same typed `IpcError` `chat.send` raises, before the model
is called. Frontend reuses the chat panel's `<AttachBar>` verbatim: the
**Run one step** card gains the "Attach current file / Attach selection"
control + chip (one-shot, cleared after a successful run), fed by the
inspector selection now passed down from `App.tsx`. Backend tests cover the
wire shape, the whole-file fold into the single-step messages, and the
secret-file block; frontend tests cover attaching a file / a line range and
the cleared chip. `npm run test` 63 green, `PLUME_FULL_VERIFY` OK. The
end-to-end model path stays Mac-only (the Qwen smokes exercise it); the
in-container suite covers the pure fold + wire shapes.

PR #82 review (Codex) fix (MEDIUM): the single-step fold skipped the
attachment **shape** validator. `chat.send` / `chat.context` run
`validate_attachment` before `attachment_to_request` — and that converter
(plus `slice_lines` downstream) *assumes* it ran: a half range
(`startLine` without `endLine`) silently became whole-file, and a
`startLine: 0` reached `slice_lines`' `start - 1` underflow. The fold is
now a small testable helper `commands::agent::fold_attachment` that runs
`validate_attachment` first (the validator was widened to `pub(crate)` and
re-exported from `commands::chat`), so the single-step path rejects the
same malformed shapes chat does. Two regression tests pin the half-range
and zero-start rejections on the agent path (they panic/whole-file without
the fix); the existing fold + secret-block tests now drive `fold_attachment`
(the real path) rather than `apply_attachment` directly. `PLUME_FULL_VERIFY`
OK.

Slice D100 is the **first mutating agent path — patch-only**. The agent
loop can now complete one safe edit end-to-end: `agent.singleStep` asks the
model for a diff, validates it read-only (D96), and — new in D100 — returns
the validated diff as `applicableDiff` so the user can **explicitly apply**
it. The command still writes nothing; the apply runs through the EXISTING
`patch.apply` (server-side re-validate → checkpoint → atomic write →
rollback-on-failure) and revert through `patch.revert` — the same verbs the
chat panel uses (D31/D33), reused, no new applier and no duplicated
validator. The **Run one step** panel gains an Apply button (then Revert)
below the event log; the apply/revert outcome is appended to the SAME log
as `toolStarted`/`toolFinished`/`toolFailed` frames built from the real
`patch.apply` result, so the log shows the full lifecycle (model turn →
proposed diff → validation → apply → revert). Hard boundaries kept:
**no automatic apply** (only a user click writes; the approval ledger is
command-only and never auto-approves a patch), **no shell command
execution, no arbitrary `tools.invoke`** — patch-only mutation. Tests:
backend pins `applicable_diff` (Some only for a valid propose-diff) and the
camelCase wire; frontend pins invalid-diff → no Apply, valid-diff → Apply
without writing before the click, apply-failure surfacing + recoverable
Apply, and the apply→Revert round-trip. `npm run test` 67 green,
`PLUME_FULL_VERIFY` OK. End-to-end with a live model stays Mac-only (the
Qwen smokes); in-container covers the pure handoff + wire + the frontend
apply/revert orchestration. `docs/IPC_CONTRACT.md § agent` documents the
patch-only-mutation contract.

Slice D101 is a **patch-path polish** (frontend only, no IPC, no backend, no
new writes). The validated diff now renders as a **Proposed change** card
directly under the single-step event log, instead of a bare Apply button: a
shared `DiffBody` renderer (extracted from chat's `DiffPreview`, the same
colored unified-diff body — no duplicated `classifyDiffLine`) shows the diff,
a tiny changed-files summary (`summarizeDiffFiles`, a pure parse of the
already-validated diff text — a UI hint, not a gate) names what changes above
the action, and the Apply/Revert row sits inside the same bordered card so
the controls are unmistakably tied to the current run. The card unmounts the
instant a new run starts (the D100 stale-control reset), so a superseding run
clears the preview, summary, and action together. **Writes are unchanged from
D100**: explicit Apply click → existing `patch.apply`; no auto-apply, no
shell, no arbitrary `tools.invoke`. Tests pin: valid diff → preview + summary
+ Apply; invalid/no diff → none of them; a new run clears the prior preview
and actions; the apply→revert round-trip still works. Plus pure unit tests
for `summarizeDiffFiles`/`changedFilesSummary` and the shared `DiffBody`.
`npm run test` 84 green, `PLUME_FULL_VERIFY` OK.

Slice D102 adds **window-local run history** to the single-step panel
(frontend only, no IPC, no backend, no disk). Each run the user starts is the
"live" run; the one it supersedes is frozen into an in-memory list
(`runHistory.ts`, newest-first, capped at 5). A compact **Recent runs**
switcher (shown only once there are ≥2 runs) lets the user revisit a past
run's event log + diff card **read-only** — a non-current run renders no
Apply/Revert controls at all, so it can never write; starting a new run snaps
the view back to live. The live run's apply/revert behavior is unchanged from
D100/D101 (the existing live state is untouched; history is a parallel
snapshot read via a `liveRef` mirror so `onRun` captures the superseded run's
final apply/revert state). Tests pin: a run appears in the switcher, selecting
a past run restores its diff preview read-only, a new run returns the view to
live, and a non-current run exposes no apply control; plus pure unit tests for
the history helpers. `npm run test` 95 green, `PLUME_FULL_VERIFY` OK.

Slice D63A lands the **chat-session persistence spine** (backend/IPC
only — no visible UI change; the sidebar wiring is D63B, per
`docs/superpowers/specs/2026-07-10-chat-session-persistence-design.md`).
New `sessions/` Rust module plus the `sessions.*` IPC family
(`list/create/load/rename/archive/delete/saveTranscript`) over SQLite
(new crate dependency: `rusqlite` 0.40.1 with bundled SQLite, so
packaged builds carry their own library). One schema, two physically
separate databases: local chats in `<app-data>/sessions/state.sqlite`
(available without a project), project chats in
`<trusted project>/.plume/sessions/state.sqlite` behind the same trust
gate as the memory/patch verbs. No command accepts a filesystem root;
ids are backend-minted and validated before lookup; symlinked
`.plume`/sessions/database paths and hardlink-aliased database files
are refused (memory/checkpoint/`safety::path` posture); transcript
snapshots replace atomically at stable boundaries
only — never per token, and the wire enum has no `streaming` variant.
Caps: 200 sessions per database, 500 entries, 256 KiB per entry, 8 MiB
per transcript. `src/lib/api/sessions.ts` ships the typed wrapper;
nothing imports it yet. Cargo suite is at 740 (710 + 30 new store and
command-layer tests); frontend suite unchanged at 113.

Slice D63B wires the sidebar to the D63A spine — the placeholder
`Local chat` / `Project chat` rows and `window.prompt` renames are
gone. New `features/sessions/` units: `useSessions` (per-scope summary
lists, database-first mutations), `usePersistedChat` (one hoisted
`useChat` per window shell; saves ONLY at stable boundaries — the
accepted user turn and each terminal outcome — detected by reference
comparison so token frames never touch the database), `SessionRow`
(row + accessible `…` menu), `SessionDialogs` (Plume-styled rename /
delete-confirm / archived-chats modals), and `SessionNotices` (visible
switch-block and save-failure banners). `ChatPanel` accepts an
optional externally-owned `chat` instance and `useChat` gained
`restore()` for transcript hydration; streaming orchestration is
otherwise untouched. Switching sessions or scopes while a reply
streams is refused with a visible notice — never silently cancelled.
Local chats stay simple surfaces (no attach, no project context) even
inside a project window; project rows list only that project's
database. Relaunch restores the most recently updated session of the
active scope. No backend changes; cargo suite unchanged at 740.
Codex's #108 review caught three real gaps, all fixed with pinned
regressions: the project shell now remounts per project root so a
project switch can never leave the previous project's rows or
transcript visible; explicit New-chat creation is serialized through
the same queue as lazy boundary creation (a slow lazy create can no
longer clobber it); and the archived-chats modal refuses to delete the
actively-streaming chat, same guard as the normal delete dialog.
Frontend suite is at 151 (114 + 37 across sessions hooks/dialogs/
sidebar/topbar/transcript mappers, ChatPanel pins, and the App-level
project-switch regression).

## Key documents

- `docs/PLUME_PROJECT_SPEC.md` — product brief
- `docs/LOCAL_AGENT_NORTH_STAR.md` — MLX-first local agent direction,
  Hermes/Sass lessons, memory/personality/skills roadmap
- `docs/HERMES_AGENT_RESEARCH.md` — clean-room Hermes/Teknium research
  pass and Plume adaptation roadmap
- `docs/TOOL_DISCLOSURE.md` — progressive tool disclosure: core vs.
  optional tiers, the stateless tool catalog + search ranking (D86)
- `docs/ARCHITECTURE.md` — process model, modules, IPC contract
- `docs/AGENT_OPERABILITY.md` — visible UI contract for human/agent control
- `docs/MODEL_PROVIDERS.md` — provider trait and per-runtime notes
- `docs/UI_STYLE.md` — hand-drawn cafe visual system
- `docs/SAFETY.md` — file/command sandbox + agent staging
- `docs/DEVELOPMENT.md` — dev setup, run, verify, test
- `docs/SMOKE_TESTING.md` — packaged app smoke checklist
- `docs/DEPENDENCY_ISOLATION.md` — local caches, venv, and no-global-install rules
- `docs/BOOTSTRAP.md` — implemented `~/scripts/setup-tauri-project.sh` contract
- `docs/DECOMPOSITION.md` — file-size guardrail + concrete refactor map for oversized files
- `docs/MLX_RUNTIME.md` — implementation-ready plan for the Plume-managed MLX-LM server (D38)
- `docs/RUNTIME_COMPARISON.md` — clean-room read of MLX-LM, llama.cpp, vLLM, Ollama, LM Studio from Plume's perspective (D55)

## Commands

- Verify (always available): `./scripts/verify.sh`
- Run a command with project-local caches: `./scripts/dev-env.sh <command>`
- Verify with clippy: `PLUME_FULL_VERIFY=1 ./scripts/verify.sh`
- Frontend dev (after `npm install`): `npm run dev`
- Tauri dev (after Rust + Node deps installed): `npm run tauri dev`
- Frontend tests: `npm run test`
- Frontend test watch: `npm run test:watch`
- Rust lint: `cargo fmt --check && cargo clippy --all-targets -- -D warnings`
- TS type check: `npm run typecheck`

## Project structure

```
plume/
  AGENTS.md
  README.md
  package.json
  tsconfig.json
  vite.config.ts
  index.html
  src/                          React + CodeMirror frontend
    main.tsx
    App.tsx
    features/
      agent/AgentWorkspace.tsx       center-zone shell — banner (D6) + ChatPanel (D7); mode cards removed in D87
      agent/AgentSettingsPanel.tsx   left-column agent autonomy settings (D84)
      chat/                          ChatPanel + useChat (D7 read-only chat)
      editor/ReadOnlyEditor.tsx      CodeMirror display surface
      file-tree/FileBrowser.tsx      useFileNavigator + Navigator + Inspector
      model-picker/                  useSelectedModel + SelectedModelBanner (D6)
      providers/ProvidersPanel.tsx   provider registry + reachability + Select button (D6)
      providers/LocalModelsPanel.tsx local model file inventory (D27, split out D32)
      providers/useProviderInventory.ts shared loader for the two panels (D32)
      memory/                        MemoryPanel + distill/topics disclosures (D37+)
      system/                        SystemChips + useSystemSnapshot (D5)
    lib/api/                    typed Tauri-invoke wrappers
    styles/                     tokens.css, ink.css, layout.css
  src-tauri/                    Rust backend (Tauri)
    Cargo.toml
    tauri.conf.json
    capabilities/default.json   narrowed to core:event:default
    src/
      main.rs
      agent/                    single-step agent loop (D96), approval + ledger, tool catalog (D92)
      chat/                     D7 + D7.1 streaming chat; D45 added MLX routing
      memory/                   JSONL memory store, distillation, topics (D37+)
      patch/                    diff parse/validate/apply/revert (D16, D31, D33)
      prompts/                  D8 Rust-private prompt-read + redactor + assemble (no IPC verb)
      commands/                 IPC handlers
      project/                  project open + persisted trust
      fs/                       sandboxed display reads
      providers/                registry + reachability + local-model scan (D27) + MLX-LM supervisor (D40)
      system/                   host machine introspection (D5)
      safety/                   path validation
      error.rs                  IpcRequest envelope + IpcError
  docs/
  scripts/
    verify.sh                   structural + guardrail + tool-aware
    dev-env.sh                  project-local cache wrapper
    smoke-app.sh                build + launch real Plume.app for agents
  reference/visual/             inspiration images, not bundled
```

## Hard rules

1. **No Electron.** This project exists partly to avoid Electron's memory
   cost on local-model laptops.
2. **No default cloud model calls.** Cloud providers must be opt-in and
   visibly labeled in the status strip.
3. **No filesystem writes outside the open project root** without explicit
   user approval. The Rust backend enforces this; the frontend never touches
   the disk directly.
4. **No shell command execution without user approval.** Verification
   commands detected from project files require an explicit approval prompt
   the first time, and that approval is scoped per-project.
5. **AGENTS.md beats CLAUDE.md.** If a `CLAUDE.md` ever appears, consolidate
   into AGENTS.md and remove the duplicate.
6. **Resource honesty in the UI.** Models too large for the user's machine
   must be flagged before load, not silently attempted.
7. **No unsolicited installs.** Never run `npm install`, `cargo install`,
   `brew install`, `pip install`, `npx create-*`, or any other dependency
   command without an explicit ask. Listing a dep in a manifest is fine;
   running an installer is not.
8. **Use the project env wrapper for dependency commands.** Run dependency,
   model-download, and build commands through `./scripts/dev-env.sh` so caches
   stay under the project instead of spreading into global user directories.

## Code style

- **Rust:** `cargo fmt`, idiomatic Rust 2021. Errors with `thiserror` /
  `anyhow` once adopted; never `unwrap` in production paths. Prefer typed
  errors over stringly-typed ones at module boundaries.
- **TypeScript:** strict mode, no `any` without a one-line comment justifying
  it, ES modules only, `camelCase` / `PascalCase` / `UPPER_SNAKE_CASE`.
- **Bounded collections only.** Caches, model registries, session histories
  must have a size cap or eviction policy. Memory leaks here directly hurt
  the model running alongside Plume.
- **Guard clauses over deep nesting** in both languages.
- **Comments only when WHY is non-obvious.** Don't restate the code.

## Before declaring a task done

1. `./scripts/verify.sh` passes.
2. Any new Rust module has at least one happy-path test and one failure-mode
   test (especially path-safety and command-validation paths).
3. Any new TS module has a smoke test or is exercised by the running app.
4. New user-facing strings respect the visual identity (no emoji, no glossy
   SaaS language, no purple-blue AI vibes).
5. `docs/` is updated when behavior or structure changes — doc-first prevents
   UI drift.

## Things to ask first

- Installing new crates or npm packages.
- Adding a new local model runtime, or removing an existing one.
- Anything that changes file/command sandbox rules.
- Renaming the project or top-level directories.
- Touching the user's global `~/scripts/` or `~/.claude/` directories.
- Initializing git, force-pushing, rewriting history, or anything destructive.
