# Memory distillation (roadmap + D48 scaffold)

This doc describes how Plume will compress, deduplicate, and (later)
LLM-summarize accumulated memory entries while preserving the
properties D37 already guarantees: trust-gated, secret-redacted on
ingest, JSONL on disk, no surprises.

D48 is the smallest safe scaffold — a pure read-only preview
function (`memory::distill_preview`) that reports what _would_
happen without writing. No IPC verb, no UI, no mutation. Future
slices wire approval-gated apply, and (later) LLM-driven
summarization.

## Why distill

D37 caps the memory store at `MAX_ENTRIES = 100` entries,
`MAX_BYTES_PER_ENTRY = 1 KiB`, `MAX_BYTES_TOTAL = 64 KiB`. D42
caps the per-send injection budget at `MEMORY_CONTEXT_BYTE_CAP =
4 KiB` (newest-first picker). Over time three accumulation modes
show up:

1. **Duplicates.** The user remembers the same fact more than
   once. Exact or case-insensitive matches do nothing useful but
   cost capacity.
2. **Near-duplicates.** "the verify script is `scripts/verify.sh`"
   and "verify lives at scripts/verify.sh". Semantically identical.
3. **Stale.** "we're using react 18" after the project moved to
   react 19. The newer entry is correct; the older one misleads
   if it lands in context after the newer is forgotten.

Distillation addresses all three:

- **Dedupe** keeps one of each duplicate set.
- **Summarize** (v2) merges near-duplicates by asking a local LLM
  to produce one entry from many.
- **Age-out** (v2) flags entries older than a configurable window
  for the user to review.

## Properties to preserve

Every property D37 and D42 hold today must continue to hold post-
distillation:

- **Trust gate.** Every mutating verb is gated on a trusted open
  project. Distillation's apply step inherits this. The preview
  step (D48) does NOT mutate and is still gated for read symmetry
  with `memory.index` / `memory.search`.
- **Redactor at the boundary.** Secret-pattern bytes never reach
  disk. Distillation reads ALREADY-REDACTED text; its output is
  the only path that could re-introduce raw bytes, so the LLM
  variant (v2) re-runs the redactor on every produced entry
  before commit.
- **Symlink defenses.** The `.plume/memory/entries.jsonl` resolver
  refuses planted symlinks. Distillation uses the same resolver.
- **Process-wide mutex.** Same lock as `remember` / `forget` /
  `search`. The preview step takes the lock so a concurrent
  `remember` can't shift the entry set mid-scan; the apply step
  holds the lock for the read-merge-write cycle.
- **JSONL stability.** Distillation rewrites the WHOLE file
  atomically (temp file → rename) like `forget` does today.
  Partial writes can never leave a corrupt JSONL.

## v1: rule-based, no LLM

The minimum-viable shape. Operator-driven; no model in the loop.

### Triggers

- Manual via a future "Distill" button in the Memory panel.
- Optional: an unread `distillSuggestion` count on
  `MemoryIndex` that nudges the user when there's at least one
  exact-duplicate group OR the store is at >= 75 % capacity. (Not
  in D48.)

### Operations

| Op             | What it does                                                                                                            |
| -------------- | ----------------------------------------------------------------------------------------------------------------------- |
| `dedupeExact`  | Find groups where `normalized(text)` is identical. Keep the newest; remove the rest. `normalized` = trim, collapse internal whitespace, lowercase. |
| `dedupeNearTBD`| (Out of scope for v1.) Levenshtein-distance threshold over normalized text. Conservative threshold to avoid false-positives. |
| `ageOutTBD`    | (Out of scope for v1.) Flag entries older than a configurable age. User reviews each.                                   |

### Wire shape (proposed; not in D48)

```rust
// IPC verbs (future)
memory.distillPreview() -> DistillPreview
memory.distillApply(payload: { groupIds: string[] }) -> DistillApplyResponse

struct DistillPreview {
    /// One group per duplicate set. Each group has 2+ entries.
    duplicate_groups: Vec<DuplicateGroup>,
    /// Total entries in the store, for the UI to show "would
    /// compact from N to M".
    total_entries: usize,
    /// Sum of `group.removable_count` across all groups.
    would_remove: usize,
}

struct DuplicateGroup {
    /// Opaque id the apply step round-trips. Stable across calls
    /// while the store hasn't changed.
    id: String,
    /// The entries in this group, newest first. By default the
    /// first entry survives apply; the rest are removed.
    entries: Vec<MemoryEntry>,
    /// Convenience: `entries.len() - 1`.
    removable_count: usize,
}
```

### Apply semantics (proposed; not in D48)

- The frontend passes the set of `groupIds` the user confirmed.
- Backend re-runs the preview INSIDE the mutex, intersects with
  the requested ids, removes the non-survivor entries from each
  intersected group, and rewrites the JSONL atomically.
- Concurrent `remember` / `forget` between preview and apply
  invalidate the affected groups (the preview is re-computed; an
  unmatched group id is a no-op, not an error).
- Result reports `{ removedEntryCount, remainingEntryCount }`.
- No "undo" verb in v1. The user can `remember` the lost text
  manually; the LLM v2 will add a pre-apply snapshot.

## v2: LLM-driven summary

Compact near-duplicates and overlapping facts into one entry per
cluster. Local-model only, behind the same trust gate.

### Open questions

- **Which model.** The MLX-LM supervisor (D40+) hosts the chat
  model. We can either reuse it (one model loaded at a time, the
  user picks whether distillation interrupts chat) or spawn a
  second smaller model just for distillation. The first is simpler
  for v2; the second is more polished.
- **Cluster detection.** Embedding similarity vs LLM-as-a-clusterer.
  Embeddings need a separate model and a vector store; LLM-as-
  clusterer just adds latency. Start with LLM-as-clusterer.
- **Prompt template.** Reserved; will live in `prompts/distill.rs`
  with the same redactor + cap discipline as `prompts::assemble`.
- **Approval.** The user sees the proposed distilled entry and
  the source entries; each cluster requires an explicit OK
  before commit. No silent merge.
- **Audit trail.** A `.plume/memory/distill-log.jsonl` records
  every apply: timestamp, removed-entry ids, produced-entry id,
  rule (`dedupeExact` / `llm`). Append-only, never read by the
  hot path.

### Properties to enforce

- LLM output passes through the redactor before commit. If the
  produced text contains a `[REDACTED:*]` marker the apply
  refuses — the LLM hallucinated content from a redacted source.
- LLM output respects `MAX_BYTES_PER_ENTRY`. Truncation policy:
  reject and surface "produced entry too long" rather than silent
  truncation.
- No internet access. The supervisor only routes to local
  servers.
- Cancel-aware. A long-running LLM call respects the existing
  chat-cancel pattern.

## D48 scaffold

What this slice ships:

- **`memory::distill_preview(root) -> Result<DistillPreview,
  MemoryStoreError>`** — pure read function. Resolves entries the
  same way `read_index` does (process-wide mutex, symlink-safe
  path resolver), normalizes each entry's text, groups by exact
  normalized match, returns groups of 2+.
- **`DistillPreview` / `DuplicateGroup`** — Rust types only. NOT
  exposed via any IPC verb. The shape is informational and
  mirrors the proposed wire shape so a future v1 IPC slice just
  wraps it.
- **Tests** in `memory_tests.rs` covering: empty store, no
  duplicates, exact match, case-insensitive match, whitespace-
  normalized match, multi-group, symlink refusal.

What this slice deliberately does NOT ship:

- No IPC verb. The function is reachable from Rust only; no
  `commands::memory::*` handler, no `memory.*` wire shape, no
  TypeScript types in `src/lib/api/memory.ts`.
- No `apply` path. No mutation, no log, no JSONL rewrite.
- No UI. The Memory panel is untouched.
- No LLM. v2 is roadmap.
- No background scheduler. Future "auto-suggest" is also roadmap.
- No "near-duplicate" detection. v1 dedupe is exact-after-
  normalization only.

The scaffold compiles and is exercised by tests, but the
production binary cannot reach it. That's the right shape for an
unfinished feature: the contract is testable, the surface is
unreachable, and the next slice wires apply without touching
the design.

## Risks to think through before v1 lands

- **Information loss.** Even exact duplicates may carry intent
  (the user remembered it twice → it's important). v1's apply
  is opt-in per group; auto-apply is explicitly out of scope.
- **Order sensitivity.** The newest-first picker (D42) means
  removing the oldest of a duplicate group is observationally
  equivalent — context content stays the same. Removing the
  newest would shift behavior. The default-survivor rule (keep
  the newest) is non-negotiable.
- **Capacity churn.** A user who distills and then runs into the
  capacity cap again experiences whiplash. v2 will eventually
  combine distill + age-out so the cap is rarely hit.
- **Redactor false-positives in distillation output.** A future
  LLM might produce text that looks like a secret (random hex
  string). The redactor will mask it; the entry stays useful
  because the user's mental model already accounts for
  `[REDACTED:*]` markers from D37 remember.

## Open

- Should `distillPreview` be cached? The store is small (64 KiB
  cap) and reads are cheap; recomputing on demand is fine for v1.
- Should the duplicate-group id encode the survivor entry's id,
  or be opaque? Opaque is safer for forward compatibility (the
  apply step can change which entry survives without breaking
  saved group ids); pin opaque.
- Should the UI show the rejected (non-survivor) entries before
  apply? Yes — full transparency. Renders as a list per group.
