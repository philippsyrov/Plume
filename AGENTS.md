# Plume — Agent Instructions

Plume is an experimental open-source local AI coding editor. The product brief
is `docs/PLUME_PROJECT_SPEC.md`. Treat it as the source of truth for product
direction.

## Stack

Tauri 2 (Rust) shell + TypeScript / React 19 frontend with CodeMirror 6 as the
editor surface. Local model runtimes (MLX-LM, Ollama, LM Studio, llama.cpp)
reach the app through a `Provider` trait in `src-tauri/src/providers/`. No
Electron. No default cloud calls.

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

## Key documents

- `docs/PLUME_PROJECT_SPEC.md` — product brief
- `docs/LOCAL_AGENT_NORTH_STAR.md` — MLX-first local agent direction,
  Hermes/Sass lessons, memory/personality/skills roadmap
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

## Commands

- Verify (always available): `./scripts/verify.sh`
- Run a command with project-local caches: `./scripts/dev-env.sh <command>`
- Verify with clippy: `PLUME_FULL_VERIFY=1 ./scripts/verify.sh`
- Frontend dev (after `npm install`): `npm run dev`
- Tauri dev (after Rust + Node deps installed): `npm run tauri dev`
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
      agent/AgentWorkspace.tsx       center-zone shell — banner (D6) + ChatPanel (D7) + mode cards
      chat/                          ChatPanel + useChat (D7 read-only chat)
      editor/ReadOnlyEditor.tsx      CodeMirror display surface
      file-tree/FileBrowser.tsx      useFileNavigator + Navigator + Inspector
      model-picker/                  useSelectedModel + SelectedModelBanner (D6)
      providers/ProvidersPanel.tsx   provider registry + reachability + Select button (D6)
      providers/LocalModelsPanel.tsx local model file inventory (D27, split out D32)
      providers/useProviderInventory.ts shared loader for the two panels (D32)
      system/                        SystemChips + useSystemSnapshot (D5)
    lib/api/                    typed Tauri-invoke wrappers
    styles/                     tokens.css, ink.css, layout.css
  src-tauri/                    Rust backend (Tauri)
    Cargo.toml
    tauri.conf.json
    capabilities/default.json   narrowed to core:event:default
    src/
      main.rs
      chat/                     D7 + D7.1 read-only chat transport (Ollama only, streaming + cancel)
      prompts/                  D8 Rust-private prompt-read + redactor + assemble (no IPC verb)
      commands/                 IPC handlers
      project/                  project open + persisted trust
      fs/                       sandboxed display reads
      providers/                static registry + TCP reachability
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
