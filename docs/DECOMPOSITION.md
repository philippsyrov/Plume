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

### `src-tauri/src/commands/chat.rs` — 1,860 lines (red)

Hosts three IPC commands plus their shared validation and stream
plumbing. The split has natural seams along the verbs:

- `commands/chat/send.rs` — `chat_send` + `run_stream` + stats
  translation (`translate_stats`, `ns_to_ms`,
  `compute_tokens_per_second`, `format_chat_error`).
- `commands/chat/cancel.rs` — `chat_cancel`.
- `commands/chat/context.rs` — `chat_context` +
  `chat_context_attachment_from_outcome` + `block_reason_for`.
- `commands/chat/validate.rs` — `validate_payload`,
  `validate_attachment`, `validate_line_range`.
- `commands/chat/mod.rs` — `check_attachment_requires_trust`,
  `optional_trusted_open`, `attachment_to_request`, and the
  re-exports the Tauri builder consumes.

Target: each child file < 600 lines; `mod.rs` < 200.

### `src/features/chat/ChatPanel.tsx` — 1,523 lines (red)

A single `.tsx` file that owns the chat panel plus eight
collaborator components. The collaborators have stable seams:

- `chat/AttachBar.tsx` — `AttachBar`, `chipMatchesSelection`,
  `describeAttachCandidate`, `formatChipPath`, `attachButtonLabel`,
  `attachButtonTitle`, `attachHintText`.
- `chat/ChatEntryRow.tsx` — `ChatEntryRow`.
- `chat/ModeToggle.tsx` — `ModeToggle`, `MODE_OPTIONS`,
  `ModeOption`.
- `chat/DiffPreview.tsx` — `DiffPreview`, `classifyDiffLine`,
  `extractDiffBlock`, `useDiffValidation`, `DiffValidationPill`.
- `chat/CopyReplyButton.tsx` — `CopyReplyButton`.
- `chat/ContextPreview.tsx` — `ContextPreview`,
  `InstructionsPreviewItem`, `AttachmentPreviewItem`,
  `blockedReasonLabel`, `formatAttachmentLabel`.
- `chat/InstructionsBadge.tsx` — `InstructionsBadge` +
  `instructionsSubtitleHint`.
- `chat/disabledReason.ts` — `DisabledReason`,
  `computeDisabledReason`, `isProviderUnreachable`,
  `isProviderChecking`, `inputPlaceholder`, `isInputDisabled`,
  `chatStatusText`.
- `chat/formatters.ts` — `formatStatsLine`, `formatStatsTitle`,
  `formatDuration`, `formatBytes`.

After extraction `ChatPanel.tsx` should hold only the top-level
component, its props, the chip-state type, and the JSX glue.
Target: `ChatPanel.tsx` < 400 lines, every child < 400.

### `src-tauri/src/prompts/assemble.rs` — 1,323 lines (red)

Mostly tests. The production surface is < 400 lines; the bulk is
the `#[cfg(test)] mod tests` block at the bottom.

- `prompts/assemble.rs` (production) — `preview_context`,
  `preview_attachment`, `assemble`, `apply_attachment`,
  `make_instructions_message`, `slice_lines`, `resolve_and_read`,
  `wrap_with_attachment`.
- `prompts/assemble/tests.rs` (or `prompts/assemble_tests.rs`) —
  the inline test module. Either approach works; Rust idiom
  prefers a sibling file with `#[path = "assemble_tests.rs"]`
  or a `tests/` subdirectory.

Target: production < 500 lines; tests file no specific cap (tests
are exempt from the code-file rule when they live in their own
file).

### `src-tauri/src/chat/ollama.rs` — 1,317 lines (red)

Two IPC entry points (`send_chat`, `stream_chat`) plus a layer of
HTTP-frame helpers that they share. The split mirrors the verbs:

- `chat/ollama/blocking.rs` — `send_chat`, `build_request_body`,
  `extract_error_message`, plus blocking-only helpers.
- `chat/ollama/streaming.rs` — `stream_chat`,
  `build_request_body_streaming`, `read_line_polled`,
  `is_timeout_kind`.
- `chat/ollama/http.rs` — `read_response_head`,
  `parse_status_line`, `drain_body_to_string`, `role_str`, and
  any other helper called from both.
- `chat/ollama/mod.rs` — re-exports + the shared types
  (`OllamaFrameStats`, `ChatError`).

Target: each child < 500 lines; `mod.rs` < 100.

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

### `src-tauri/src/patch/parse.rs` — 862 lines (amber)

Diff parser. Crossed 800 once D31's hunk-body capture landed and
D33 relaxed the no-hunks rule for renames. The split into
`parse/header.rs` (line-by-line stream, `--- /+++ ` pairing,
git rename markers) and `parse/hunk.rs` (hunk header parsing +
HunkLine assembly) is still the natural cut if growth resumes;
plan but don't force yet.

### `src-tauri/src/patch/apply.rs` — ~985 lines (amber, post-D33 split)

D33 was about to push this past the red line (1200). The split
moved manifest types + on-disk read/write/GC into a sibling
`src-tauri/src/patch/checkpoint.rs` (~340 lines). What stays in
apply.rs is the public entry, the per-file planner, the hunk
walker, and the rollback path — all things that have to know
about `ApplyPlan`. `revert.rs` (~620 lines) is the third file in
the family; it consumes `checkpoint::read_checkpoint` for the
read side and `apply::write_atomic` for the write side. If
apply.rs creeps past 1100 again, the next extraction is the
hunk-application code (`apply_hunks_to` + `create_from_hunks`)
into a sibling `hunks.rs`.

### `src/features/providers/ProvidersPanel.tsx` (D32 split)

D32 split the legacy `ProviderPanel.tsx` (~527 lines, yellow at
the time) into `ProvidersPanel.tsx` (reachability + per-row
model expansion), `LocalModelsPanel.tsx` (local model file
inventory), and `useProviderInventory.ts` (shared loader). The
per-row model expansion is still the natural future extraction
inside `ProvidersPanel.tsx` if growth resumes — watch.

### `src-tauri/src/prompts/redact.rs` — 512 lines, `src-tauri/src/prompts/read.rs` — 511 lines (yellow)

Both production-heavy with smaller test blocks. No urgent split.

## Doc-side: `docs/PLUME_PROJECT_SPEC.md` — 1,506 lines, `docs/IPC_CONTRACT.md` — 973 lines

These are spec docs; length is justified by the surface area.
Soft watch: if either crosses 2,000 lines, plan a narrower split
(e.g. extract `docs/IPC_CONTRACT_CHAT.md`). No action today.

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
