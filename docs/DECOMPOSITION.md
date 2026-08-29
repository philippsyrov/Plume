# Decomposition Guardrail

Plume is built by humans collaborating with LLM agents. Both humans
and agents review code one file at a time. As individual files cross
the 1,000-line mark, reviewer attention degrades, agent tool calls
have to skim instead of read, and the cost of a "small" change in
the file goes up disproportionately. This doc pins the rule and
lists the concrete split plan for the current oversized files.

As of D122 this is a hard gate for non-test Rust and TypeScript production
files: the check script (see below) FAILs on any such `*.rs` / `*.ts` /
`*.tsx` file past 800 lines, and `scripts/verify.sh` (and with it the
pre-commit hook and GitHub Actions) blocks on that failure. Standalone test
files and test directories are exempt from the automated size gate; Markdown
files keep a soft warn.
There is no grandfather list — the D108–D120 refactor slices
cleared every amber/red file first, so the gate started from zero.

## Thresholds

The rule is the same for every non-test production file (`*.ts`, `*.tsx`,
`*.rs`):

| Range          | Status     | Action                                                              |
|----------------|------------|---------------------------------------------------------------------|
| ≤ 400 lines    | green      | Healthy. No action.                                                 |
| 401–800 lines  | yellow     | Acceptable. Watch growth; resist additions that push it past 800.   |
| 801–1,200 lines| amber      | FAILs `check-file-sizes.sh` and verify (D122). Split before merge.  |
| > 1,200 lines  | red        | FAILs `check-file-sizes.sh` and verify (D122). Split before merge.  |

Doc files (`*.md`) follow a looser rule because reference docs can
be legitimately long. Soft warn at 1,500 lines; no hard target.
Reviewers should still ask whether a long doc would be clearer as
two narrower docs, but length alone is not a defect for prose.

Frozen lossless snapshots under `docs/history/` remain visible to this
advisory rather than receiving a silent exemption. In particular,
`docs/history/slice-ledger.md` intentionally exceeds the soft cap because it
preserves the former agent-entrypoint chronology without dropping decisions.
Its warning is accepted archival evidence, not an instruction to rewrite or
split the snapshot. The separate 400-line hard gate on active `AGENTS.md`
prevents that history from regrowing in the current entrypoint.

Standalone test files and test directories are exempt from the automated
size gate. They still need ordinary clarity and review: a large test suite is
not automatically easy to maintain. Inline tests still count toward the gate
because they live inside a production file; extract them to a standalone test
file or `tests/` directory when they would push that production file over the
threshold.

## Enforcement (active as of D122)

`scripts/check-file-sizes.sh` runs as part of `scripts/verify.sh`
and emits a `[FAIL]` (exit 1) for every non-test production file at amber or
red. Standalone tests are excluded by the checker; inline tests remain part of
their production file's count.
Verify maps that into its own hard-fail path, so the pre-commit
hook and the GitHub Actions workflow both block the commit/merge
on an oversized code file. Doc files past the 1,500-line soft cap
still only `[WARN]` (pass `--strict` to the script manually to
make doc soft-caps fail too).

The cadence rule below still applies: when the gate fires on a PR,
the split should land as its own preceding slice, not get bundled
into the feature diff. Forcing splits at the same time as feature
work produces sloppy splits (or sloppy features).

History: D21 landed this check warn-only, with the pre-existing
oversized files grandfathered and a plan to harden once the map
was "mostly executed" (originally via a NEW-files-only gate plus a
`decomposition-grandfather.txt` of shrinking stragglers). The
D108–D120 slices cleared the entire map first — 0 amber / 0 red —
so D122 skipped the grandfather machinery entirely and made the
rule unconditional for all non-test production files.

## Refactor map — current oversized files

The numbers below are line counts as of the slice that introduced
this guardrail. They drift; treat them as the starting state, not
the contract. Each entry lists the file, what's actually inside,
and a credible split sketch. The sketch is NOT a prescription —
the refactor PR for each file is free to disagree, but should say
why.

Status as of D120: every split this map has called for is executed,
and `scripts/check-file-sizes.sh` reports **0 amber / 0 red** code
files. The remaining entries are either done (kept below as the
record of what landed where) or watch-only yellows. Only the two
doc-side soft-caps warn today (see "Doc-side" below).

### `src-tauri/src/commands/chat.rs` — 306 lines (green, post-D23 split)

D23 split along the verb seams originally sketched here:
`commands/chat/send.rs` (`chat_send` + `run_stream`; regrew to
amber after D42/D45 and was re-split by D116/D118/D120 — see its
own entry below), `commands/chat/cancel.rs` (`chat_cancel`, 38
lines), `commands/chat/context.rs` (`chat_context` +
attachment/outcome mapping, 640 lines, yellow),
`commands/chat/validate.rs` (payload-shape validators, 460 lines,
yellow). `chat.rs` itself stays the orchestrator: shared constants,
the `AttachmentPayload` wire enum, the small helpers every
submodule reaches for, and the re-exports `main.rs` consumes.

### `src/features/chat/ChatPanel.tsx` — 419 lines (yellow, post-D22 split)

D22 split along the seams originally sketched here — `AttachBar.tsx`
(258), `ChatEntryRow.tsx` (147), `ModeToggle.tsx` (70),
`DiffPreview.tsx` (597, yellow), `CopyReplyButton.tsx` (53),
`ContextPreview.tsx` (212), `InstructionsBadge.tsx` (189),
`disabledReason.ts` (202), `formatters.ts` (57) — plus `useChat.ts`
(678 lines, tracked separately below). `ChatPanel.tsx` now holds
the top-level component, its props, the chip-state type, and the
JSX glue.

### `src-tauri/src/prompts/assemble.rs` — 764 lines (yellow, post-D24 split)

D24 extracted the inline `#[cfg(test)] mod tests` block into a
sibling `assemble_tests.rs` (1,071 lines, test-exempt) and pulled
message-construction helpers into `assemble_messages.rs` (122
lines). What stays in `assemble.rs` is `preview_context`,
`preview_attachment`, `assemble`, `apply_attachment`,
`slice_lines`, and `resolve_and_read` — production code only, no
tests inline.

### `src-tauri/src/chat/ollama.rs` — 117 lines (green, post-D25 split)

D25 split along the verb seams originally sketched here:
`chat/ollama/blocking.rs` (`send_chat` + blocking-only helpers, 388
lines), `chat/ollama/streaming.rs` (`stream_chat`, 313 lines,
green), `chat/ollama/http.rs` (shared HTTP-frame helpers, 137
lines). `ollama.rs` itself is now just re-exports plus the shared
types (`OllamaFrameStats`, `ChatError`). The Thermos-L1 slice
(PR #156) then moved the polling line reader into the shared
`chat/stream_read.rs` (bounded, used by both streaming adapters)
and extracted the inline test module to a sibling
`chat/ollama/streaming_tests.rs` (test-exempt) to stay under the
800-line cap.

### `src-tauri/src/patch/validate.rs` — 764 lines (yellow, upper)

Single function `validate_patch` with extensive support: path
checking, parse-error translation, ancestor walks. Split is
optional today but worth planning before any feature work touches
this file.

- `patch/validate.rs` — `validate_patch`, the public entry
  point.
- `patch/validate/path.rs` — `check_diff_path`,
  `ensure_inside_or_existing_ancestor`.
- `patch/validate/errors.rs` — `parse_error_to_response`,
  `map_change_type`.

Target: each piece < 400 lines. If staying under one file with
the test block extracted lands at < 600, that's also acceptable.

### `src/features/chat/useChat.ts` — 625 lines (yellow)

Single hook with several reducer cases. Likely splittable into
`useChat.ts` (the hook + public types) + `useChat/reducer.ts`
(state shape + cases) + `useChat/events.ts` (event subscriptions).
Watch for growth; no immediate action required.

### `src-tauri/src/patch/parse.rs` — 492 lines (green, post-D35 split)

D35 extracted the inline `#[cfg(test)] mod tests` (~371 lines)
into a sibling `parse_tests.rs` via `#[path]`, mirroring the
`apply_tests.rs` / `revert_tests.rs` pattern from earlier
decomposition slices. The production split into
`parse/header.rs` + `parse/hunk.rs` outlined here originally is
no longer needed at this size; revisit if production lines grow
past 800 again.

### `src-tauri/src/patch/apply.rs` — 772 lines (yellow, post-D35 split)

D35 followed the path the original entry sketched: extracted
`apply_hunks_to` + `create_from_hunks` into sibling
`apply_hunks.rs` (~166 lines), and `rollback_apply` into sibling
`apply_rollback.rs` (~146 lines). What stays in apply.rs is the
public entry, the per-file planner, plan execution + atomic
write, and the small `change_type_to_wire` adapter. D33 had
already moved manifest types + on-disk read/write/GC to
`checkpoint.rs` (~446 lines now); revert lives in `revert.rs` +
`revert_planning.rs` (D35). The whole patch module is now green
or yellow.

### `src-tauri/src/patch/revert.rs` — 445 lines (green, post-D35 split)

D35 extracted per-entry planning (`RevertPlan`,
`plan_revert_entry`, `validate_manifest_path`, `drift_check`,
`load_pre_image`, `change_type_to_wire`) into a sibling
`revert_planning.rs` (~392 lines). What stays in revert.rs is
the public entry, the wire types, and the
snapshot/execute/rollback path. The split keeps the safety-
critical drift-detection and path-validation surface in a single
focused file that's easier to audit.

### `src-tauri/src/memory/mod.rs` — 646 lines (yellow, post-D108 split)

D108 split by behavior boundary, matching the `distill.rs`/`topics.rs`
re-export pattern already used in this file: `types.rs` (~259 lines,
every wire/response type — `MemoryEntry`, `MemoryIndex`,
`MemoryPromptRead`, the `MemoryRemember`/`Update`/`Forget`/`Search`
response families, `MemoryStoreError`; no logic) and `store.rs`
(~211 lines, the on-disk storage layer — symlink-safe path
resolution, JSONL read/write, atomic write, id minting; no verb
logic). `mod.rs` (~646 lines, yellow) keeps the module doc, the
process-wide `memory_mutex`, the caps, and the five CRUD verbs
(`read_index`/`read_for_prompt`/`remember`/`update`/`forget`/`search`).
Every external `crate::memory::X` path is unchanged: types re-export
`pub` (same as `distill`/`topics`); the storage helpers re-export at
their original bare-`fn` visibility (module + descendants), so
`distill.rs`/`topics.rs`'s existing `use super::{resolve_entries_path,
refuse_symlink, ...}` needed no changes.

### `src/features/providers/ProvidersPanel.tsx` (D32 split)

D32 split the legacy `ProviderPanel.tsx` (~527 lines, yellow at
the time) into `ProvidersPanel.tsx` (reachability + per-row
model expansion), `LocalModelsPanel.tsx` (local model file
inventory), and `useProviderInventory.ts` (shared loader). The
per-row model expansion is still the natural future extraction
inside `ProvidersPanel.tsx` if growth resumes — watch.

### `src-tauri/src/prompts/redact.rs` — 512 lines, `src-tauri/src/prompts/read.rs` — 511 lines (yellow)

Both production-heavy with smaller test blocks. No urgent split.

### `src-tauri/src/commands/chat/send.rs` — 401 lines (yellow, post-D116/D118/D120 split)

D116/D118/D120 executed the sketch that used to live here, in the
safest-first order it proposed:

- `send_tests.rs` (393 lines, test-exempt) — the whole
  `#[cfg(test)] mod tests` block via `#[path]`, mirroring
  `assemble_tests.rs` / `parse_tests.rs` (D116).
- `send_route.rs` (72 lines) — `ChatRoute` + `resolve_route` at
  `pub(super)`, re-imported into `send.rs` with a bare `use` so
  their module-private visibility never widened (D118).
- `send_outcome.rs` (232 lines) — both `*_outcome_to_done` mappers,
  the D9 stats math (`translate_stats` / `ns_to_ms` /
  `compute_tokens_per_second`), and both error formatters (D120).
  `send.rs` re-imports only the two entry points `run_stream`
  calls; the test-only helpers are imported directly by
  `send_tests.rs` so non-test builds carry no unused imports.

The sketch's `send_types.rs` step was NOT taken: with the other
three out, `send.rs` landed at 401 lines holding `chat_send`,
`run_stream`, and the four wire types — splitting the types would
have bought one line of green for a new file. All 24 tests held
green across the three slices with import-only edits, including the
D45 Codex `model_label` regression
(`resolve_route_returns_mlx_with_port_and_model_label_for_registered_handle`).

### `src-tauri/src/providers/mlx_lm/process.rs` — 792 lines (yellow, post-D117/D119/Thermos-I1/Codex-#154 splits)

D117/D119 executed the sketch that used to live here. All three new
files are `#[path]` submodules of `process.rs` with `pub use`
re-exports at the original `process::` paths (the
`assemble_messages.rs` mechanism), so `process_tests.rs`'s
`use super::process::*;` and every internal caller resolved with
zero edits:

- `process_launch.rs` (109 lines) — `allocate_port`,
  `MlxLmCommand`, `default_mlx_lm_command`, `build_command_args`;
  `resolve_python_program` stays private inside it, its only
  caller having moved with it (D117).
- `process_ring_buffer.rs` (64 lines) — `RingBuffer` +
  `RING_BUFFER_CAP` (D117).
- `process_health.rs` (125 lines) — `poll_health` + `HealthError`;
  `try_health_probe` and the two backoff consts stay private
  inside it (D119).

Two later slices continued the same mechanism:

- `process_stop.rs` (Thermos I1) — `stop_child`'s SIGINT-grace →
  SIGKILL escalation, `stop_server`, the exit sweep
  (`shutdown_all_managed_servers` + `ShutdownSummary`), and the
  recovery listing (`list_managed_servers` + `ManagedServerInfo`).
- `process_diagnostics.rs` (Codex #154) — the D52 diagnostics
  surface (`lookup_diagnostics` + `ServerDiagnostics`). This is the
  sketch step the previous revision deliberately deferred "only if
  it grows past 800 again": the #154 reservation/reap rework pushed
  `process.rs` to 872, so the deferred split was taken. Child-module
  siblings share the registry internals, so "needs the registry lock
  and private fields" turned out not to block the move.

What stays in `process.rs` (792 lines, yellow) is the registry +
slot-reservation start lifecycle core, `lookup_handle_info`, the raw
`kill`/`setsid` FFI bindings, and the test-only registry helpers.
The full `providers::mlx_lm::tests` module (41 tests, including the
D110 lookup, D112 allocator, D114 timeout, Thermos-I1 lifecycle, and
Codex-#154 reservation/reap suites) held green across every split.

### Apple and Qwen onboarding owners (current watch map)

The fixed-catalog slice stayed below the hard production-code cap by keeping
authority seams separate:

- `src/features/model-picker/useModelCatalog.ts` (601 lines, yellow) owns the
  window catalog lifecycle and race fences; `ModelChooser.tsx` (288 lines)
  owns presentation.
- `src-tauri/src/commands/providers.rs` (714 lines, yellow) keeps provider IPC
  validation while catalog-download handlers live in
  `providers_catalog_download.rs` (168 lines).
- `src-tauri/src/providers/catalog_download.rs` (620 lines),
  `catalog_download_fs.rs` (702 lines), `catalog_download_publish.rs`
  (361 lines), and `catalog_download_runtime.rs` (298 lines) split policy,
  descriptor-safe filesystem work, publication, and transfer execution.
- `src-tauri/src/providers/apple_foundation.rs` (560 lines) owns the bounded
  helper process; chat event adaptation stays in
  `src-tauri/src/chat/apple_foundation.rs` (126 lines).

These are watch-only yellows, not a commissioned refactor. Future model-catalog
work should extend the existing seams instead of regrowing a single catalog or
provider command file past 800 lines.

### Research-note harness owners (current watch map)

The Stage A research workflow is decomposed by authority and pure logic:
`research/run.rs` (683) owns the bounded controller, `research/bundle.rs` (729)
owns immutable session-local versions, `commands/research.rs` (623) owns strict
IPC and provider launch, and the smaller `budget`, `model`, `evidence`,
`context`, `citations`, `markdown`, `export`, and `run_registry` modules own
their named seams. Tests live in sibling `*_tests.rs` files.

`src/features/chat/ChatPanel.tsx` is 778 lines after the calm research-note
entrypoint. The next chat-composer or research-flow growth must split
orchestration/glue out before the 800-line gate rather than compressing the UI
or weakening tests.

## Doc-side: long current contracts

`docs/PLUME_PROJECT_SPEC.md` and `docs/IPC_CONTRACT.md` are long spec docs;
their length is justified by the surface area. `IPC_CONTRACT.md` has crossed
the 1,500-line doc soft cap
(`scripts/check-file-sizes.sh` warns on it); soft watch: if either
crosses 2,000 lines, plan a narrower split (e.g. extract
`docs/IPC_CONTRACT_CHAT.md`). No action today.

## Cadence rule

Decomposition gets its own slices. Do NOT bundle a refactor of
`chat.rs` into a PR that also adds a new feature to `chat.rs` —
the diff becomes unreviewable and the failure mode of either half
can mask the other.

A decomposition slice should:

- Touch ONE oversized file and the files it splits into.
- Preserve every public name unless explicitly noted.
- Keep tests green without modifications (test movement is fine;
  test behavior changes are a separate PR).
- Be reviewable by reading the new files in isolation, not by
  diffing the old.

`git mv` + add followed by an explicit re-export PR is the
preferred shape: phase 1 moves code without changing call sites,
phase 2 updates call sites if needed. The bisect surface stays
clean.

## Pointer

- Rule + thresholds + map: this file.
- Check script (hard gate for code as of D122): `scripts/check-file-sizes.sh`.
- Wired into: `scripts/verify.sh § File sizes`.
