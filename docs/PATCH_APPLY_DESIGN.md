# Patch Apply Design

Status: roadmap. Not implemented. This file designs the next slice on
top of the D15 / D16 read-only stack so the first writing verb in Plume
has a contract before it has code.

## Why this doc exists

D15 (`mode: 'proposeDiff'`) lets the model emit a unified diff in a
fenced block; the chat panel renders it with per-line coloring and a
disabled Apply button. D16 (`patch.validate`) parses the same diff,
enforces project-root path safety on every diff-side path, and returns
either `{ ok: true; touches; hunks }` or `{ ok: false; errors }`. The
Apply button stays disabled even when validation passes — see
`docs/SAFETY.md § Patch validation` and the D16 subsection underneath
it. Neither slice touches disk.

Pressing Apply has to write files, and writing files in an agent
context has more decisions than "open and `write_all`". This doc
records those decisions so the implementation slice (D21+) lands a
shape, not an argument. The actual wire types still live in
`docs/IPC_CONTRACT.md § patch` once they ship.

## Wire shape

Three reserved verbs. The shapes below are TypeScript-flavored for the
same reason `docs/IPC_CONTRACT.md` is — they are the on-the-wire
shape, not Rust internals. The placeholder shapes in
`docs/IPC_CONTRACT.md § patch` (`patch.apply(diff: string) -> {
applied; checkpoint }`, `patch.checkpoint() -> string`,
`patch.revert(checkpoint: string) -> void`) get refined into the
envelope-wrapped, in-band-error versions below when the verbs land.

```ts
patch.apply(payload: { diff: string })
  -> PatchApplyOk | PatchApplyErr

interface PatchApplyOk {
  applied: true;
  checkpoint: string;            // see § Checkpoint storage; always present on success
  touched: PatchAppliedFile[];
}

interface PatchAppliedFile {
  path: string;                  // project-relative, forward-slash, matches PatchTouch.path
  changeType: 'modify' | 'create' | 'delete' | 'rename';
  renamedFrom?: string;
  bytesWritten: number;          // post-apply file size on disk; 0 for delete
}

interface PatchApplyErr {
  applied: false;
  reason: PatchApplyFailure;     // see § Failure modes
  details?: PatchFailureDetail[];// per-file, when the failure is per-file (preImageMismatch)
}

interface PatchFailureDetail {
  path: string;
  hunkIndex?: number;            // 1-based hunk within the file
  message: string;
}

type PatchApplyFailure =
  | 'validationFailed'           // re-validation rejected the diff
  | 'preImageMismatch'           // pre-image hunk did not match disk
  | 'checkpointFailed'           // could not record a pre-apply checkpoint
  | 'writeFailed'                // disk write failed mid-apply
  | 'scopeUnsupported'           // diff includes a change type the slice does not support
  | 'untrusted';                 // belt-and-braces; trust gate normally raises IpcError
```

```ts
patch.checkpoint(payload: {}) -> { checkpoint: string }
```

```ts
patch.revert(payload: { checkpoint: string })
  -> PatchRevertOk | PatchRevertErr

interface PatchRevertOk {
  reverted: true;
  restored: PatchRestoredFile[];
}

interface PatchRestoredFile {
  path: string;
  changeType: 'modify' | 'create' | 'delete' | 'rename';
}

interface PatchRevertErr {
  reverted: false;
  reason: PatchRevertFailure;
  details?: PatchFailureDetail[];
}

type PatchRevertFailure =
  | 'unknownCheckpoint'          // id not in store
  | 'drift'                      // file changed since apply; needs explicit override
  | 'writeFailed'
  | 'untrusted';
```

### Payload: diff only, no client-supplied validation result

`patch.apply`'s payload carries `diff` only. The backend re-runs
`patch.validate` server-side before any write. The frontend's
`patch.validate` result is a UI helper, not a security artifact —
trusting it would let a future bug or a swapped renderer ship a diff
that the validator never saw. Validating twice is cheap (the parser is
hand-rolled, no I/O) and removes a class of trust-the-client mistakes.

### Error model — typed `IpcError` vs in-band `ok: false`

Same split as `patch.validate`:

- The `Promise` rejects with `IpcError` only for envelope failures
  (`Version`), trust gating (`NeedsApproval`), or a genuine internal
  failure (`Internal`). The first two are the same conditions
  `patch.validate` rejects on today.
- Everything that is a normal, expected, user-explicable outcome of an
  apply attempt — re-validation rejected the diff, pre-image mismatch,
  out-of-disk, checkpoint creation failed — comes back in-band as
  `PatchApplyErr` with a typed `reason`. The frontend renders the
  failure under the diff (next to the existing validation pill); the
  promise resolves cleanly so the chat panel does not have to do
  parallel try / catch + result-discrimination plumbing.

`BadArgument` and `PathEscape` from the existing `IpcError` enum
**do not** appear on `patch.apply`'s rejection path — they collapse
into `validationFailed` on the in-band shape. The frontend already
handles `IpcError` rejections for `patch.validate`; surfacing the same
class of errors via two different mechanisms across two related verbs
is the kind of inconsistency that becomes a bug when the UI is rewired.

## Pre-image verification

`patch.apply` MUST re-read every file the diff modifies / deletes and
compare its current bytes against the diff's pre-image hunks before
writing. D16 explicitly skips this (it doesn't write, so a stale
pre-image is not dangerous; see `docs/SAFETY.md § Patch validation`
D16 subsection). For `patch.apply` the situation inverts: applying a
diff whose pre-image doesn't match disk means the model is reasoning
about a version of the file that no longer exists, and the post-image
the user sees may delete or mangle the user's intervening edits.

The check is hunk-level, not whole-file. For each file the validator
classifies as `modify`, `delete`, or `rename`:

1. Open the file once through the canonical path (FD-based; see
   `docs/SAFETY.md § Avoid TOCTOU`).
2. For each hunk, compare the lines under the hunk's `-` / context
   slots against the file at the hunk's stated line range.
3. If any hunk disagrees, record the mismatch under that file's
   `details` and continue checking the remaining hunks.

For `create`-typed touches the pre-image is empty by definition; the
check is "the file does not currently exist on disk."

### Atomic rejection on any mismatch

If any hunk in any touched file fails pre-image verification, the
whole apply rejects with `reason: 'preImageMismatch'` and a `details`
list naming every file-and-hunk that failed. No file is written. The
alternative — partially apply the files that match, leave the rest —
is rejected explicitly: the user signed off on one diff, not an
arbitrary subset of it, and a partial apply leaves the project in a
state neither the model nor the user described.

### Why not soft drift, like `git apply --3way`

Future slice question. The first implementation does not try to
recover; mismatch is mismatch. A merge fallback adds a meaningful
amount of code (three-way merge logic, conflict marker injection, a
"resolve in editor" UX) for a case that, in chat workflows, is usually
solved by re-prompting the model with the current file content. We
revisit if real usage shows pre-image drift is common enough to be
friction.

### Symlink-resolve races

Pre-image reads run through the same FD-based pattern the file
sandbox documents. Concretely: canonicalize the path once, `open` to
get an FD, do the pre-image compare against that FD, then write
through the same FD (or unlink-and-replace; see § Atomicity). Calling
`fs::read` on a `Path` and then `fs::write` on the same `Path` would
allow a symlink swap between the two operations. The helper at
`safety::path::ensure_inside` is part of the canonicalize step, not a
substitute for the FD discipline.

## Atomicity

**All-or-nothing across the whole patch.** Not per-hunk, not per-file.
If any pre-image check fails, no file is touched. If any write fails
after the first write has landed, the in-flight apply rolls back via
the checkpoint taken before any write.

The reason is the user model. The user reviewed one diff and pressed
Apply once. "Half of it landed, the other half didn't, here's a toast"
is worse than "nothing landed, here's what's wrong" — the user can
re-prompt the model with the current state and get a coherent next
diff. A partial-success state is a state the model never proposed
and the user never approved.

### Per-file write strategy

For each touched file:

- `modify`: write the post-image to a sibling tempfile in the same
  directory (`<file>.plume-<random>.tmp`), `fsync` it, then atomic
  rename over the original. Same-directory rename keeps the operation
  atomic on POSIX even across filesystem oddities (`renameat` semantics
  apply per-directory).
- `create`: write the new file to the same sibling tempfile pattern,
  rename into place. Reject if the destination exists at apply time
  (the pre-image check for `create` already confirmed absence; this
  is a belt-and-braces guard against a race the FD discipline can't
  fully close on `create` since there's no FD to compare against).
- `delete`: rename the existing file out to `.plume/checkpoints/<id>/`
  rather than unlinking in place. If the apply later rolls back, the
  delete is undone by renaming back. The checkpoint store IS the
  delete's pre-image storage.
- `rename`: combine — rename old path to new path; checkpoint records
  the old path so revert can swap back. If the rename also includes
  hunks (a rename-with-edits), the rename happens first, then the
  modify path runs against the new location.

### Cross-file ordering and rollback

Writes run sequentially in `touched`-array order. If the Nth write
fails:

1. Rollback every previous write by restoring from the checkpoint
   (rename the saved pre-image back over the touched path, undo
   creates by deleting the new file, undo deletes by restoring the
   saved copy).
2. Surface `reason: 'writeFailed'` with a `details` entry naming the
   file that failed and the OS error.
3. Leave the checkpoint in place — the user might want to inspect it,
   and GC handles eventual cleanup (see § Checkpoint storage).

There is no "best effort" rollback. If the rollback itself fails (disk
finally filled, fs went read-only mid-operation), Plume surfaces a
critical failure with both the original write error and the rollback
error; the checkpoint directory contents are the user's recovery path.
This is the worst case the design tolerates without escalating to an
OS-level transactional fs (which Plume does not assume).

### Interaction with the FD-based safety pattern

`docs/SAFETY.md § Avoid TOCTOU` requires that safety-sensitive ops
either operate on an FD obtained from the canonicalized path or run
canonicalize + op atomically with no async point in between. The
tempfile + rename pattern fits because:

- Canonicalize the target's parent directory once.
- Open the tempfile inside that canonical parent (the tempfile is
  inside the project root by construction).
- `renameat` from canonical parent to canonical parent is a single
  syscall — no window for a symlink swap to redirect the write.

For `create` against `link/new.rs` where `link` is symlinked out, the
validator already rejects in D16 (see the D16 subsection in
SAFETY.md). `patch.apply`'s re-validation catches the same case
before any write attempt.

## Checkpoint storage

**Filesystem checkpoints for D21. Git checkpoints deferred.**

The checkpoint stores enough pre-image state to reverse the apply. Two
real options:

1. **Filesystem checkpoint** — `.plume/checkpoints/<id>/` directory of
   pre-image copies (and a small manifest naming what was created,
   so revert knows to delete those rather than restore).
2. **Git checkpoint** — `git stash create` or a real commit on an
   internal `plume/checkpoints/<id>` ref.

Trade-offs:

| Aspect                    | Filesystem                                         | Git                                                  |
| ------------------------- | -------------------------------------------------- | ---------------------------------------------------- |
| Works on non-git projects | yes                                                | no (git is not a hard project requirement)           |
| Side effects in the repo  | none (`.plume/` is project-local, gitignored)      | adds refs / stash entries; user-visible in `git log` |
| Storage cost              | byte copy of every touched file                    | git compresses; cheap for tiny diffs                 |
| Cleanup                   | rm directory                                       | `git update-ref -d` + gc                             |
| Inspection                | plain files                                        | `git show`                                           |
| Implementation cost       | small (file copy + manifest)                       | larger (libgit2 or shelling git, error model bigger) |

D21 ships filesystem only. Git-based checkpoints are appealing but
they require Plume to take a non-trivial action inside the user's
repo, which is itself the kind of action the approval gate exists to
mediate. A user who opens a non-git directory must still get apply.

A future slice can layer git-checkpoints as an opt-in tier, ideally
keyed off `agentMode === 'agent-loop'` (which already implies a
pre-run git checkpoint per `docs/SAFETY.md § \`agent-loop\` always
requires`) or off an explicit preference.

### Storage layout

```
<project>/.plume/checkpoints/<checkpointId>/
  manifest.toml          # one entry per touched path: change_type, old_path, new_path
  files/
    src/foo.rs           # pre-image copy of <project>/src/foo.rs
    src/old_name.rs      # pre-image copy of a rename's old path
```

`<checkpointId>` is the same opaque id the response carries:
ULID-shaped, generated at checkpoint creation, never reused. The
prefix on disk is the literal string — no further nesting — so a
human eyeballing `.plume/checkpoints/` can see what's there.

`manifest.toml` is the source of truth for revert; the `files/` tree
is a content addressable copy keyed by project-relative path. The
manifest records the `change_type` for each entry so revert knows
that a `create`-typed entry has no `files/` copy (only the post-apply
path needs to be deleted) and that a `delete`-typed entry needs the
saved copy renamed back into place.

### Where the checkpoint id surfaces

`PatchApplyOk.checkpoint` always carries the id; the field is
non-optional on success. The frontend stores it on the assistant turn
so the Revert button can call `patch.revert({ checkpoint })` later.
`patch.checkpoint` (standalone) returns `{ checkpoint }` for callers
that want to snapshot without applying — reserved for `agent-loop`'s
pre-run snapshot.

### GC policy

Two retention modes, applied at apply time and on session close:

- **Soft cap.** Keep the most recent N checkpoints per project
  (`N = 20` to start). Older directories get rm'd best-effort. The cap
  is per-project, not global, so a single high-velocity project
  doesn't evict another project's history.
- **Hard cap by age.** Anything older than 30 days, regardless of N,
  is eligible for deletion. The user can disable via a setting (not
  shipped yet; just keep the knob in mind so future cleanup work
  doesn't repaint the policy).

Cleanup is opportunistic: scan + prune on every apply, never blocking
the apply itself. A failed prune (disk error, permissions) logs and
moves on; the apply still succeeds.

Checkpoints are NOT auto-deleted on revert. A user who reverts may
want to revert the revert; keeping the directory means we don't have
to design "redo." When the revert path also takes a checkpoint (it
should, see § Revert flow), the redo is just another revert of a
different id.

## Revert flow

`patch.revert(payload: { checkpoint })`:

1. Read `.plume/checkpoints/<id>/manifest.toml`. Unknown id rejects
   with `reason: 'unknownCheckpoint'`.
2. For each entry in the manifest, compare current disk content to the
   post-apply state we expect: i.e. the diff's *post-image*. If any
   file's current content does not match what apply left there, treat
   as drift and reject with `reason: 'drift'` and per-file `details`
   naming which files differ. The user has edited since apply; a
   silent revert would destroy their work.
3. On agreement, perform the inverse of apply: restore manifest
   entries from `files/`, delete the new files for `create` entries,
   restore the saved file for `delete` entries, rename back for
   `rename` entries.
4. Take a fresh checkpoint of the post-apply state BEFORE the revert
   writes anything, so the user can "redo" by reverting the new
   checkpoint. The response carries the post-revert state's files
   list; the new checkpoint id is internally retained but not
   currently surfaced — future shape may grow a
   `redoCheckpoint?: string` field.

### Idempotency

`patch.revert` is **not idempotent.** Calling it twice with the same
id reverts once and then rejects the second call with
`reason: 'unknownCheckpoint'` (if cleanup ran) or `reason: 'drift'`
(if it hasn't — the post-apply state we expect no longer matches
disk, because the first revert already changed disk). Either failure
is a clean rejection, never a silent no-op. The frontend renders the
failure the same way it renders the apply-failure surface.

### Drift override

Out of scope for D21. The first slice rejects on drift, period. A
later slice can layer an explicit `override: 'discardLocalEdits'`
flag, but that needs its own approval prompt — a revert that nukes
user changes is exactly the kind of thing the approval gate exists
for, even more than apply itself.

## UI contract

The patch surface is one diff in one assistant turn. Apply / Revert
state lives on that turn, not on a global widget.

### Success

- Apply button transitions to `Applied` (disabled).
- The validation pill's existing slot grows a second line:
  `checkpoint <id>` in pencil, with the id rendered short
  (`01HXY…` — first 5 chars, hover for full).
- A new `Revert` button appears next to (now-disabled) Apply.
- The D14 Copy button continues to function — copying the diff after
  apply is a legitimate "save it as a note" operation.

### Failure

The validation pill is the single render surface for state changes on
the diff. Apply failure renders there:

- `validationFailed`: same UI as D16's invalid pill, with the typed
  reason in the message.
- `preImageMismatch`: pill in `--bad`,
  `apply rejected — <N> file(s) drifted since the diff was
  generated`. Hover surfaces the file list from `details`.
- `checkpointFailed` / `writeFailed`: pill in `--bad` with the OS
  message. No toast layer is added for this; the validation-pill slot
  is the established render surface for diff-state changes on a turn.

The Apply button does not disable on a failure; the user may try
again after re-prompting the model. The pill itself is the source of
truth for whether the last attempt succeeded.

### Mid-apply

Small diffs apply in well under a second; the implementation should
NOT introduce a streaming surface (no `patch.progress` event, no
spinner with steps). The Apply button disables for the duration of
the in-flight `patch.apply` call (a single Promise round-trip) and
re-enables on response. A future "thousand-file refactor" mode might
need progress, but D21 explicitly does not.

### Revert UI

`Revert` button on a successfully-applied turn:

- Disabled when the validation pill shows any drift or unknown-
  checkpoint indication (cheap pre-check Plume can run by stat-ing
  one touched file; expensive full pre-check happens server-side).
- On click, calls `patch.revert({ checkpoint })`. On success, the
  Applied state on the turn flips back to a `Reverted` state with a
  pencil note `<id> reverted`. On failure, replaces the validation
  pill content with the revert error.

The Revert state intentionally does NOT delete the diff from the
transcript — the user wants to remember they tried the change.

## Approval gate

Today's session is locked to `agentMode: 'chat'` (D7) or
`'propose-diff'` (D15) with `approvalPolicy: 'ask-each'` and no
file allowlist — see `docs/SAFETY.md § Agent stages`. Apply is the
first writing verb Plume ships.

**Pressing the Apply button itself is the approval. No separate
confirm dialog.**

Argument: the propose-diff surface IS the approval surface. The user
reviewed a fully-rendered diff with per-line coloring, saw the
`valid diff · N files · M hunks` pill confirming what's about to be
touched, and chose to press Apply. A second `"Are you sure?"` dialog
on top of that:

- Adds friction that does not reduce mistakes — the user just clicked
  Apply intentionally; clicking Confirm doesn't introduce new info.
- Trains the user to dismiss confirm dialogs reflexively, which
  weakens the approval signal everywhere else.
- Duplicates the review the user just did on the diff itself.

What DOES gate apply:

- The diff is in a trusted-project chat turn (existing trust gate;
  same as `patch.validate`).
- `patch.apply` re-runs `patch.validate` server-side and rejects on
  any validation failure — the Apply click did not bypass the
  path-safety / fence checks (a malicious renderer can't smuggle a
  different diff past).
- A checkpoint must succeed before any write; checkpoint failure
  rejects the whole apply.

### Once `scoped-edit` mode lands

`scoped-edit` would let the apply happen against a per-task
`fileAllowlist` with `approvalPolicy: 'ask-on-write'`. In that mode
the gate shifts: subsequent applies inside the allowlist need no
re-prompt; an apply against a file NOT in the allowlist still
prompts. Today's chat-only session has no allowlist concept and an
empty allowlist is the safe floor — every apply is per-click.

This is described as a future state only. `scoped-edit` is not
shipped, the allowlist plumbing is not built, and `patch.apply` in
D21 always runs in the per-click model. When `scoped-edit` lands the
apply gate will need exactly one new check (`approvalPolicy ===
'ask-on-write' && fileAllowlist.includes(every touched path)`); the
default-to-prompt behavior stays.

## Failure modes

These are the rejection conditions `patch.apply` enforces. The
in-band `reason` keeps them machine-stable. Items marked `IpcError`
go out as a typed reject, not as `PatchApplyErr`.

| Condition                                    | Surface                                           |
| -------------------------------------------- | ------------------------------------------------- |
| No trusted project open                      | `IpcError::NeedsApproval`                         |
| IPC envelope version mismatch                | `IpcError::Version`                               |
| Re-validation fails (any error from D16)     | `reason: 'validationFailed'` + `details`          |
| Diff includes a `delete` or `rename` (D21)   | `reason: 'scopeUnsupported'` (see § D21 scope)    |
| Any file's pre-image disagrees with disk     | `reason: 'preImageMismatch'` + per-file `details` |
| `create` target already exists at apply time | `reason: 'preImageMismatch'` (treated as drift)   |
| Checkpoint write failed (disk full, perms)   | `reason: 'checkpointFailed'`                      |
| Any file write failed; rollback succeeded    | `reason: 'writeFailed'` + the failing file        |
| Any file write failed; rollback ALSO failed  | `IpcError::Internal` with both errors             |

Disk-full and write-permission failures specifically fall into
`writeFailed`, not `IpcError::Internal`. The model and the user can
react to "disk full"; "internal" is for genuinely unexpected states
(panics caught at the handler boundary, mutex poisoning, etc.).

`untrusted` exists on `PatchApplyFailure` only as belt-and-braces in
case a future code path needs to surface a trust failure in-band; the
default `NeedsApproval` reject is the actual gate.

## First implementation slice (D21 scope)

Cut so the slice is one PR shaped like every other D-slice.

**Ships in D21:**

- `patch.apply(payload: { diff })` IPC verb.
- Server-side re-validation via the existing `validate_patch`.
- Pre-image verification (hunk-level, atomic reject on mismatch).
- All-or-nothing apply with rollback on mid-apply write failure.
- Filesystem-backed `.plume/checkpoints/<id>/` storage with
  `manifest.toml` + `files/`.
- `patch.checkpoint(payload: {})` standalone verb that calls the same
  checkpoint primitive (no diff, no write — returns
  `{ checkpoint }`). Useful for tests and the future `agent-loop`
  pre-run snapshot.
- `patch.revert(payload: { checkpoint })` IPC verb. Drift-detect.
  Reject on drift (no override flag yet).
- Chat panel wiring: Apply button calls `patch.apply`, transitions to
  Applied state with checkpoint id, surfaces a Revert button. Failure
  rendered in the validation-pill slot.

**Scope-cut deliberately deferred to D22+:**

- `create` / `delete` / `rename` change types. **D21 ships
  modify-only.** The validator already classifies; the apply path
  rejects non-modify with `reason: 'scopeUnsupported'`. Reasoning:
  modify is the dominant case from chat-mode diffs, and ship-pressure
  on the first writing verb matters more than completeness. Adding
  the other three change types in a follow-up slice lets the
  filesystem checkpoint design be exercised on the simplest case
  first.
- Git-based checkpoints. Filesystem checkpoints work everywhere; git
  is a follow-up tier.
- `scoped-edit` integration. The whole `agentMode` axis is not wired
  through IPC yet (`session.setMode` is roadmap per
  `docs/IPC_ROADMAP.md § Session mode and policy`). Apply is
  per-click until then.
- Drift override on revert.
- Three-way merge / soft drift recovery on apply.
- Multi-checkpoint redo UI. The internal redo-checkpoint is recorded
  but not surfaced.
- A configurable GC policy. The 20-checkpoints / 30-days defaults
  ship hardcoded; preferences UI is later.

## Open questions

These are real questions that an implementation slice needs an answer
to before it lands. Recording them here so reviewer and integrator
catch any deferred decisions.

1. **Final-line-newline.** What does Plume do when the diff's
   post-image lacks a trailing newline and the original file had one
   (or vice versa)? Unified diffs use `\ No newline at end of file`
   to mark this; the D16 parser does not currently track it. D21
   needs to either round-trip the marker (preferred) or normalize and
   document the normalization.
2. **Mode bits on `create`.** New files created via `patch.apply`:
   inherit parent directory perms, hardcoded `0o644`, or read mode
   from a `new file mode 100644` git header when present? Git diffs
   include the bit; non-git diffs don't.
3. **Encoding.** Plume's pre-image read assumes UTF-8 for compare
   purposes. A file that is valid UTF-8 with a BOM, or a file in a
   non-UTF-8 encoding, currently has no defined behavior. Probable
   answer: reject with `preImageMismatch` if the file isn't a clean
   round-trip through UTF-8, since the model emitted UTF-8 lines and
   we can't safely write a "mixed" file. Worth confirming.
4. **Permissions for `.plume/checkpoints/`.** On a multi-user
   workstation, the directory should be `0o700`. Plume runs as the
   user today; not urgent, but the implementation should not create
   the checkpoint dir world-readable.
5. **Symlinked `.plume/`.** A user (or a hostile repo) could replace
   `.plume/` with a symlink to outside the project. The checkpoint
   write would then escape. The fix is the same FD-based
   canonicalization the rest of the safety design uses, applied to
   `.plume/checkpoints/` on first use per session.
6. **Concurrent applies.** Two chat turns in the same window, both
   with Apply buttons. Pressing both before either resolves: is the
   second rejected, queued, or interleaved? Probable answer:
   per-project mutex on the apply path; second press rejects with a
   transient `reason` (not yet listed above — add `'busy'` if this
   becomes a real risk).
7. **Editor open during apply.** The read-only inspector might have
   the about-to-be-modified file open. After apply, the inspector
   should re-read; today there's no IPC for "file changed under you."
   D21 can punt and require the user to re-open the file; a later
   slice can add a `fs.invalidate` event.
