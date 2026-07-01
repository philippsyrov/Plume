# Decomposition Guardrail

Plume is built by humans collaborating with LLM agents. Both humans
and agents review code one file at a time. As individual files cross
the 1,000-line mark, reviewer attention degrades, agent tool calls
have to skim instead of read, and the cost of a "small" change in
the file goes up disproportionately. This doc pins the rule and
lists the concrete split plan for the current oversized files.

This is a guardrail, not a hard gate. The check script (see below)
warns; it does not fail CI yet. Existing files past the threshold
are grandfathered. The goal is to stop growing the long tail and
to make a credible plan for shrinking the head of the distribution
across explicit refactor slices.

## Thresholds

The rule is the same for every code file (`*.ts`, `*.tsx`, `*.rs`):

| Range          | Status     | Action                                                              |
|----------------|------------|---------------------------------------------------------------------|
| ≤ 400 lines    | green      | Healthy. No action.                                                 |
| 401–800 lines  | yellow     | Acceptable. Watch growth; resist additions that push it past 800.   |
| 801–1,200 lines| amber      | Should be planned for split. Pre-existing files: see refactor map.  |
| > 1,200 lines  | red        | Must be split. Pre-existing files: planned in the refactor map.     |

Doc files (`*.md`) follow a looser rule because reference docs can
be legitimately long. Soft warn at 1,500 lines; no hard target.
Reviewers should still ask whether a long doc would be clearer as
two narrower docs, but length alone is not a defect for prose.

Tests are NOT counted separately. A 1,500-line file that is 1,300
lines of `#[cfg(test)] mod tests` is still over the threshold —
extract the test module into a sibling file or its own
`tests/` directory. Test bloat is a real code-review tax.

## Soft enforcement (today)

`scripts/check-file-sizes.sh` runs as part of `scripts/verify.sh`
and emits a `[WARN]` for every file at amber or red. It never
emits a `[FAIL]`, and the verify summary still says "OK" if the
only complaints are file sizes. The pre-commit hook and the
GitHub Actions workflow both pick up the warnings via verify but
do not block the commit/merge on them.

This is deliberate. Forcing splits at the same time as feature
work produces sloppy splits (or sloppy features). Decomposition
gets its own slices.

## Future enforcement (later)

Once the refactor map is mostly executed (the four red-zone files
brought below 800), we will:

1. Tighten the script so NEW files added in a PR that exceed
   800 lines cause `[FAIL]`. Existing grandfathered files keep
   warning.
2. Maintain a `scripts/decomposition-grandfather.txt` of paths
   still over the threshold but actively being reduced.
3. Eventually remove the grandfather list when the longest
   remaining file is under 800.

This file documents the intent; the script does not enforce step 1
yet. When it does, the bump will land in its own slice.

## Refactor map — current oversized files

The numbers below are line counts as of the slice that introduced
this guardrail. They drift; treat them as the starting state, not
the contract. Each entry lists the file, what's actually inside,
and a credible split sketch. The sketch is NOT a prescription —
the refactor PR for each file is free to disagree, but should say
why.

### `src-tauri/src/commands/chat.rs` — 306 lines (green, post-D23 split)

D23 split along the verb seams originally sketched here:
`commands/chat/send.rs` (`chat_send` + `run_stream` + stats
translation, now 1,052 lines — amber; D45 MLX routing and D42
memory-context wiring landed on top of the D23 split and pushed it
back past 800), `commands/chat/cancel.rs` (`chat_cancel`, 38 lines),
`commands/chat/context.rs` (`chat_context` + attachment/outcome
mapping, 640 lines, yellow), `commands/chat/validate.rs`
(payload-shape validators, 460 lines, yellow). `chat.rs` itself
stays the orchestrator: shared constants, the `AttachmentPayload`
wire enum, the small helpers every submodule reaches for, and the
re-exports `main.rs` consumes. `send.rs` is the one child worth a
follow-up split.

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
lines), `chat/ollama/streaming.rs` (`stream_chat` + polling helpers,
727 lines, yellow), `chat/ollama/http.rs` (shared HTTP-frame
helpers, 137 lines). `ollama.rs` itself is now just re-exports plus
the shared types (`OllamaFrameStats`, `ChatError`).

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

### `src-tauri/src/memory/mod.rs` — 1,067 lines (amber, post-D108 split)

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

### Currently amber, not yet mapped

`scripts/check-file-sizes.sh` is the authoritative current list.
As of this sweep it flags two files with no refactor-map entry yet:
`src-tauri/src/commands/chat/send.rs` (1,052 lines — see the
`commands/chat.rs` entry above) and
`src-tauri/src/providers/mlx_lm/process.rs` (936 lines, the D40
supervisor: port allocation, spawn, health probe, ring buffer, and
shutdown all in one file). Neither has a split sketch yet — that's
a future decomposition slice, not a design decision made here.

## Doc-side: `docs/PLUME_PROJECT_SPEC.md` — 1,519 lines, `docs/IPC_CONTRACT.md` — 1,764 lines

These are spec docs; length is justified by the surface area.
`IPC_CONTRACT.md` has crossed the 1,500-line doc soft cap
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
- Soft check script: `scripts/check-file-sizes.sh`.
- Wired into: `scripts/verify.sh § File sizes`.
