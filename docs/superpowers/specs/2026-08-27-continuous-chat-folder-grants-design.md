# Continuous Chat And Folder Grants

**Date:** 2026-08-27

**Status:** Approved product direction; implementation is commissioned only
through the phased campaign in `docs/ROADMAP.md`

**Base:** `main@c05752a01e473cfc82b535173ecf945c0ef909cd`

## Goal

Make Plume feel like one persistent local AI teammate instead of a collection
of disposable chats and project containers. The user should be able to keep
talking, let Plume compact old context safely, approve useful long-term
learning, and grant access to whichever folders a task needs without creating
or switching Projects.

The user-facing product becomes chat-first:

- one durable Home conversation is the default surface;
- context limits do not force a new chat;
- folders are attached permissions, not top-level product objects;
- one coding run may write inside one explicitly approved folder root;
- additional attached folders are read-only unless the user deliberately
  promotes one for a later run;
- memory is reviewable, scoped, correctable, and provenance-bearing; and
- action authority remains bounded even though conversation continuity feels
  effortless.

This is an architectural programme, not one implementation slice. Each phase
below must produce independently testable software and preserve the current
feature inventory's exact `shipped`, `partial`, `scaffolded`, `researched`, and
`candidate` labels.

## Product decision

### Remove Projects from the consumer model

Plume will not ask users to create, open, or switch a Project merely to talk to
the assistant or continue prior work. The normal product vocabulary becomes:

- **Chat** — the persistent conversation;
- **Folders** — places the user has explicitly allowed Plume to inspect;
- **Working folder** — the one folder a bounded coding run may modify;
- **Reference folders** — additional attached folders that the run may inspect
  but not modify;
- **Memory** — approved durable facts and preferences; and
- **Runs** — bounded attempts to complete a concrete task.

The Rust backend still maintains exact scope and trust. Removing the Projects
surface does not remove canonical roots, path gates, hardlink checks,
redaction, approval records, or source ownership.

### One conversation does not mean one unbounded prompt

The full conversation is durable history. The provider receives only a
bounded projection made from:

1. canonical system and safety instructions;
2. currently valid folder grants and run permissions;
3. approved memory selected for this turn;
4. the newest valid compaction checkpoint;
5. complete recent turns kept verbatim; and
6. exact explicitly attached sources resolved for this turn.

Compaction is a model-context optimization. It is not deletion, memory,
permission, or proof that a past claim remains true.

## User experience

### Launch and ordinary conversation

Opening Plume lands in Home chat. If no durable Home conversation exists,
Plume creates it in app-private storage. Relaunching returns to the same
conversation and restores its full visible chronology.

The user may start a temporary fresh conversation or branch/rewind for
experimentation, but Plume never requires that merely because the active model
is approaching its context limit. History, branch, and rewind controls remain
available through progressive disclosure rather than dominating the default
sidebar.

### Attaching a folder

The user can say “work in Plume,” choose **Add folder**, or drop a folder onto
the conversation. Plume opens the native folder chooser or resolves an
already-known grant, then shows a plain-language review:

> Plume can read this folder. A coding run will ask before making it the
> working folder.

The conversation displays a small folder chip only while the folder is
relevant. Folder attachment must not create a new chat, move the user to a
workspace, or silently inject the whole repository into the prompt.

Attaching another folder adds a reference scope to the same conversation. It
does not replace the first folder or mutate the current run's authority.

### Starting a coding run

When an instruction requires action, Plume presents one concise run preview:

- the intended outcome;
- the proposed working folder;
- any read-only reference folders;
- the files or commands already known to be required;
- the iteration, time, output, and memory budgets; and
- the approval policy.

Only one folder is writable in one run. Additional folders remain read-only.
Changing the working folder ends or pauses the current lease and requires a
new visible approval. Cross-folder changes are separate runs with separate
patches and checkpoints.

### Quiet compaction

When the rendered model context approaches its tested budget, Plume creates a
checkpoint and continues automatically. The transcript shows one quiet event:

> Earlier conversation condensed · Review

Review shows the checkpoint summary, the transcript boundary, the model and
prompt version that produced it, and a **Rebuild from history** action. It does
not show internal token bookkeeping in the normal transcript.

If compaction cannot produce a valid checkpoint before overflow, Plume stops
before sending an invalid request and explains that the context needs review.
It never silently drops recent user turns, tool results, safety boundaries, or
accepted source manifests.

### Reviewable learning

Plume may propose a durable memory after a clear user correction, preference,
or repeated stable decision:

> Remember that you prefer focused tests before the full verifier?

The user can **Remember**, **Edit**, **Not now**, or **Never suggest this**.
They also choose the scope:

- **About you** — app-private and available without a folder;
- **For this folder** — stored through that folder's trusted memory boundary;
  or
- **This conversation only** — retained in transcript/compaction state but not
  promoted to durable memory.

An approved memory retains its source turn ids, creation time, revision, and
scope. The user can correct or forget it. Rejected proposals do not become
hidden memories.

Approved **About you** entries may become bounded ambient context only after a
separate acceptance slice proves exact manifest reporting, deterministic caps,
and correction/forget behaviour. Approved folder memory may become ambient
only while that exact folder grant is valid. Until those gates ship, memory
remains explicitly attached as it is today.

## Architecture

### Ownership model

New consumer conversations live in the backend-owned app-data session store.
A conversation is not owned by a folder. This allows one chronology to work
across zero, one, or several folder grants.

Folder-owned data remains physically scoped:

- project/folder memory stays under the canonical folder's guarded `.plume`
  store;
- approval ledgers stay tied to the exact canonical folder root;
- patches and checkpoints stay tied to the one writable root for their run;
- Browser evidence remains owned by its persisted conversation and preserves
  its existing app-private versus folder-sensitive provenance; and
- fixed model installations and app-private user memory remain in app data.

The current local/project session databases are not destructively merged.
Existing local sessions remain ordinary history. Existing project sessions
remain readable through a compatibility path after their folder is granted.
A later explicit import may copy a legacy conversation into app-private
history while preserving its original owner metadata; no migration deletes or
rewrites the source database.

### Folder grants

The frontend receives an opaque backend-minted grant id and display metadata,
never a reusable trusted source body. Conceptually:

```text
FolderGrant {
  grantId,
  displayName,
  canonicalIdentityDigest,
  access: read,
  trustRevision,
  createdAtMs,
  revokedAtMs?
}
```

The canonical path remains Rust-private after the native selection/trust flow.
Every operation re-resolves `grantId`, reopens the canonical root without
following aliases, and rechecks trust, path containment, size, binary,
hardlink, redaction, and capability-specific limits.

A folder grant permits bounded reads only. It does not itself permit writes,
commands, model startup, Browser actions, or arbitrary tool invocation.

Grant lifecycle rules:

- a revoked or missing grant fails closed;
- moving or replacing the underlying folder invalidates its canonical
  identity until the user approves it again;
- a grant is available to the user, not silently transferable to remote page
  content or a model-authored reference;
- compaction may mention a folder display name but cannot recreate a grant;
- deletion of a chat does not delete folder data or a grant; and
- revoking a grant does not delete the chat or its historical accepted-source
  manifests.

### Run leases

Each actionable task receives a short-lived Rust-owned lease:

```text
RunLease {
  runId,
  conversationId,
  writableGrantId,
  readableGrantIds[],
  fileAllowlist,
  commandAllowlist,
  approvalPolicy,
  iterationCap,
  deadlineMs,
  outputBudgetBytes,
  createdAtMs,
  expiresAtMs
}
```

The exact wire shape is defined by the slice implementation plan, but these
invariants are binding:

- exactly one writable grant;
- zero or more distinct read-only grants;
- no caller-supplied filesystem roots;
- explicit normalized argv allowlists rather than shell strings;
- filesystem containment for every spawned process, established before the
  process starts and independent of its argv;
- patch writes reuse validation, checkpoint, atomic apply, and drift-checked
  revert;
- Stop cancels model generation, queued tool work, and active commands;
- expiry or revocation prevents the next action even if a model already
  proposed it; and
- the visible run trace records requests, approvals, bounded results,
  cancellations, failures, and patch identities.

An approved argv is not containment. A verifier Plume spawns inherits Plume's
own filesystem access, so `npm test` running an arbitrary project's test script
can write anywhere the user can — outside the lease, outside every grant, and
without any further approval. Working directory, allowlists, and the approval
preview all describe what Plume *asked* for; none of them binds what the child
process may then do.

A run may therefore only execute a command through one of:

- an OS-enforced sandbox that confines the child to the writable root plus the
  read-only grants, applied by the parent at spawn time; or
- a narrowly purpose-built verifier that Plume implements and whose filesystem
  reach is fixed by its own code rather than by the command it is given.

This fails closed. Where neither is available on the platform, the run does not
execute the command: it reports that verification could not be contained and
leaves the patch for the user to test outside Plume. Falling back to an
uncontained spawn is never the answer, and the trace records the refusal.

### Conversation projection and compaction

The durable transcript remains the source record. Compaction adds an immutable
checkpoint event; it does not replace earlier entries. A checkpoint records:

- its owning conversation;
- the inclusive history boundary it summarizes;
- the first complete recent turn kept verbatim;
- structured goal, constraints, progress, decisions, unresolved work, and
  critical facts, each carrying its provenance: the source turn ids it was
  derived from and, when it restates a durable memory entry, that entry id and
  revision;
- referenced accepted-source manifest ids rather than copied trusted bodies;
- model/runtime identity and compaction prompt version;
- token estimates before and after;
- creation time and superseded checkpoint id, when any; and
- validation status.

Compaction boundaries must preserve whole user turns and model/tool pairs. A
tool result can never appear without its request. Active streaming turns,
pending approvals, partially applied patches, and unsettled command results
are not compacted.

The projection builder always reconstructs current authority from structured
backend state. It never relies on summary prose for:

- system or safety instructions;
- folder grants or trust;
- writable roots;
- file or command allowlists;
- source acceptance;
- memory scope;
- approval status; or
- current model capability.

Repeated compaction summarizes the previous valid checkpoint plus later
complete turns. Rebuild discards derived checkpoints from the active
projection and regenerates them from retained history; it never rewrites the
history itself.

Provenance is re-resolved on every projection, not trusted from the last one.
Without that step compaction quietly defeats forget: a fact copied into a
checkpoint outlives the memory entry it came from, and the next compaction
summarizes the checkpoint rather than the source, so each generation launders
the fact further from anything the user can inspect or revoke.

So before a checkpoint is used, every fact it carries is re-checked against
current state. A fact is dropped from the projection when its source memory
entry has been forgotten, when that entry's revision has moved on, or when its
source turns are no longer in retained history. A checkpoint that loses facts
this way is marked stale and rebuilt from history rather than re-summarized;
dropping a fact is never a silent edit of the stored checkpoint, because the
checkpoint stays immutable. A fact with no resolvable provenance is not
eligible for the projection at all.

### General-purpose artefacts

Plume is a general-purpose local harness, not only a coding editor. The user
asks; the harness does the work — and the work is as often a document, a deck,
or a spreadsheet as it is a patch.

The authority model carries over unchanged. One writable folder per run,
reference folders read-only, bounded reads, visible approval, a run trace. None
of that assumes source code.

**The write primitive does not carry over.** Patch apply works because code is
line-oriented text: a unified diff can be validated against a pre-image,
checkpointed, applied atomically, and drift-checked on revert. A `.pptx` or
`.docx` is a zip of XML. A diff over it is meaningless, and so is restoring a
pre-image line. Widening the patch path to cover binary artefacts would give up
exactly the properties that make it safe.

So there are two write primitives under one approval gate:

- **Patch apply** — text files, unchanged. Validate, checkpoint, apply
  atomically, drift-checked revert.
- **Guarded artefact write** — whole-file replacement for binary or generated
  documents. The run proposes a complete file; the user sees what it is, where
  it would land, and how large it is; approval writes it atomically inside the
  one writable root, keeping the previous bytes as the checkpoint so revert
  restores the file rather than a line range.

Revert is drift-checked, exactly as patch revert is. The checkpoint records the
bytes Plume wrote as well as the bytes it replaced, and revert refuses when the
file on disk no longer matches what Plume left there — otherwise restoring the
old version silently destroys whatever the user or another tool changed since.

A generated artefact usually has no previous bytes, so the checkpoint records
that the target was absent. Reverting then removes the file Plume created, and
only that file: it stays in place if it has changed since, and a directory
created solely to hold it is removed only while it is still empty.

Both stay inside the writable grant, both appear in the run trace, and neither
accepts a caller-supplied path — the target is resolved through the grant like
every other file reference. An artefact write is never implicit in a chat reply.

The consumer surface stays deliberately plain: the user states an outcome, and
Plume does the work with its reasoning, approvals, and results visible but not
demanding. No mode grid, no tool palette, no editor chrome around a task that
does not need one.

### Durable storage policy

Full history is only honestly unbounded if the disk is. Retention promises
nothing without a policy for the moment the store reaches its limit, and the
tempting failure — silently trimming the oldest turns — would break the one
guarantee this design rests on.

Each durable store therefore carries a documented cap: a byte budget for the
app-private conversation store and a per-conversation transcript budget, both
recorded in the contract rather than left to the implementation.

Behaviour at the cap fails closed and stays visible:

- Plume warns while approaching the cap, in ordinary language, with the numbers
  it is measuring.
- At the cap it **refuses further appends** to that store. It never deletes,
  trims, or rolls over a transcript to make room.
- A refusal explains what is full and offers the recovery paths: review the
  conversation, export it, or explicitly delete conversations the user chooses.
- Deletion remains an explicit user action on a named conversation. Compaction
  is not a recovery path, because it adds a checkpoint rather than reclaiming
  history.
- Reads, review, and export keep working at the cap; only new writes stop.

Refusing to append is a visible failure the user can act on. Silent deletion is
an invisible one they cannot.

### Memory and learning

Memory proposals are separate typed records, not assistant prose parsed from
the transcript. A proposal records candidate content, proposed scope, source
turn ids, reason, and proposal status. Only an explicit user action creates or
updates a durable memory entry.

The first learning slice is deterministic and narrow. It may propose only:

- explicit preferences stated by the user;
- corrections the user asks Plume to retain;
- stable workflow choices repeated in the same form; or
- a direct “remember this” request.

It does not infer identity, sensitive traits, secrets, credentials, medical or
financial conclusions, relationship facts, or speculative intent. It does not
write memory in the background or generate topics automatically.

Every prompt reports the exact memory entries accepted. Forget immediately
removes an entry from future projection. Correct creates a new revision and
marks the old one superseded without erasing provenance.

### UI composition

The consumer shell keeps chat central. The default sidebar contains compact
access to conversation history, Library, Models, and Settings. Files, Browser,
run trace, diffs, and diagnostics appear contextually when the conversation
uses them.

Remove or retire from the normal flow:

- Open Project as a prerequisite;
- local-chat versus project-chat creation choices;
- a permanent Projects collection;
- project-owned session switching as the primary navigation model; and
- UI copy implying that selecting a folder automatically grants execution.

Preserve:

- accessible names and keyboard routes;
- visible trust summaries and approval/cancel controls;
- exact source details behind progressive disclosure;
- Continue, Rewind, branch, archive, and history recovery;
- Library scope indicators; and
- model/runtime and run evidence when the user opens details.

## Error handling

Errors use ordinary language first and typed backend causes underneath.

- **Folder unavailable:** Keep the chat; mark the folder chip unavailable and
  offer **Choose again**.
- **Trust changed:** Stop before the operation and request a fresh grant.
- **Wrong writable folder:** Do not redirect the write. Explain which folder
  the current run can change.
- **Reference-folder write requested:** Offer a separate run with that folder
  as writable; never silently promote it.
- **Compaction invalid:** Keep full history, retain the last valid checkpoint,
  and stop before overflow if no safe projection fits.
- **Memory conflict:** Show the existing and proposed fact together; require
  Keep existing, Replace, Merge, or Cancel.
- **Run interrupted:** Preserve the trace and patch/checkpoint state; Continue
  starts from the last settled boundary rather than replaying an uncertain
  action.
- **Legacy project chat unavailable:** Keep its index entry hidden until its
  exact folder is granted; never search arbitrary disk locations.

## Security and privacy invariants

1. Chat continuity never grants filesystem, command, Browser, or host
   authority.
2. The frontend never supplies a trusted root after grant creation.
3. One run has exactly one writable folder root.
4. Reference folders are read-only and cannot receive patches, commands, or
   generated exports.
5. All writes continue through approved patch or purpose-built guarded IPC.
   Binary and generated artefacts use the guarded whole-file write, never a
   widened patch path.
6. No arbitrary shell string or broad `tools.invoke` is introduced.
7. Compaction prose cannot create trust, approvals, memory, or source
   acceptance.
8. Full history is retained until an explicit user deletion and remains
   physically bounded by the documented storage policy above: at the cap Plume
   refuses new appends and never trims or deletes a transcript to make room.
9. App-private user memory and folder memory remain separate stores and exact
   prompt-manifest entries.
10. Remote Browser content cannot attach folders, approve actions, promote
    memory, or invoke application commands.
11. No model or runtime downloads happen silently.
12. Cross-folder work is split into independently approved writable runs.
13. Compaction cannot resurrect a corrected or forgotten fact: checkpoint facts
    carry provenance, and provenance is re-resolved on every projection.
14. No command runs outside an OS-enforced sandbox or a purpose-built verifier;
    where neither is available the run refuses to execute it.

## Phased delivery

### Phase 0 — Contracts and evaluation fixtures

Specify the new ownership terms, compaction record, grant lifecycle, migration
fixtures, projection invariants, and acceptance corpus before changing the
consumer shell. Keep current behaviour reachable while tests are introduced.

**Exit:** Deterministic tests can distinguish durable history, projected
context, memory, grants, and run authority; no product behaviour changes yet.

### Phase 1 — Durable Home conversation

Add an app-private Home identity, relaunch restoration, and unified history
entrypoint. Preserve local/project session compatibility and current trust
behaviour. Do not remove Projects UI yet.

**Exit:** A user can relaunch repeatedly into the same Home conversation
without attaching a folder or creating a new chat.

### Phase 2 — Transparent compaction

Add immutable compaction checkpoints, provider-neutral projection, complete
recent-turn retention, manual review/rebuild, repeated-compaction tests, and
safe overflow failure.

**Exit:** A long deterministic conversation survives multiple compactions
while retaining goals, constraints, decisions, unsettled-work boundaries, and
exact safety state.

### Phase 3 — Reviewable learning

Add typed memory proposals, explicit approval/edit/reject, provenance,
correction, forget, and app-private versus folder scope. Ambient injection
remains gated until its separate exact-manifest acceptance tests pass.

**Exit:** Plume can learn a user-approved stable preference and later prove
exactly why, where, and when it was used.

### Phase 4 — Read-only folder grants

Replace caller-visible roots in new flows with opaque grant ids. Allow Home
chat to attach and read from multiple folders through existing path,
hardlink, binary, size, redaction, and exact-manifest gates.

**Exit:** One conversation can answer from two explicitly granted folders
without either becoming writable or leaking into the other.

### Phase 5 — Chat-first shell and legacy migration

Remove Projects from the normal consumer navigation, make folder attachment
contextual, unify history discovery, and provide a non-destructive legacy
project-chat compatibility/import path.

**Exit:** Fresh users never encounter a Project concept; existing users retain
access to prior chats and folder data without silent movement or deletion.

### Phase 6 — Writable run leases

Introduce one-writable-root run leases, read-only reference folders, visible
approval preview, budgets, expiry, cancellation, traces, and guarded verifier
execution. Reuse the current patch validate/apply/revert path.

**Exit:** A bounded run can read, propose, apply with approval, test, stop, and
revert inside one folder while proving all other grants stayed read-only.

### Phase 7 — Multi-iteration coding loop

Connect the existing controller scaffold to the lease-backed file, patch, and
approved-command adapters. Keep iteration caps, output caps, settled-action
boundaries, visible failures, and human Stop.

**Exit:** Plume completes a representative read/edit/test/fix task through at
least one failed test and correction without escaping the approved root or
repeating an uncertain action.

### Phase 8 — Local-model task matrix

After the execution pipeline is stable, evaluate Qwen3.8-27B first against the
real Plume task fixtures. Treat Muse Glimmer as a challenger against the exact
selected Qwen 3.x 30–37B-class checkpoint. Keep Qwen3.8-Flash-Next outside the
practical catalogue because its current 4-bit MLX weight footprint leaves
insufficient headroom on the target machine. Keep GLM-5.3 on the watchlist
until weights, runtime support, licensing, and measured Plume evidence exist.

Any MLX-LM/MLX-VLM runtime update is its own reviewed slice with pinned hashes,
packaging tests, cancellation tests, model-template/tool-call fixtures, and
packaged memory evidence. No model is downloaded without explicit approval.

**Exit:** The catalogue and capability tiers reflect measured task completion,
latency, memory pressure, cancellation, vision, and structured-action evidence
rather than vendor benchmark claims.

### Phase 9 — Later capabilities

Only after the guarded loop ships may Plume add one allowlisted skill/tool path,
then separately consider agent Browser actions, scheduled work, or sandboxed
computer-use emission. Host-wide macOS control, blanket multi-folder writes,
silent background learning, and unrestricted plugin execution remain outside
this programme.

## Verification programme

Every behaviour phase begins with a failing focused test and ends with:

1. focused frontend/Rust tests;
2. migration, stale-response, cancellation, and failure-path tests;
3. `npm run verify:docs` when documentation changes;
4. `PLUME_FULL_VERIFY=1 ./scripts/verify.sh` with no failures;
5. packaged-app smoke for navigation, native-folder, model, Browser, or run UI;
6. exact-head findings-only review;
7. feature inventory and domain-map updates matching the exact head;
8. one focused branch and reviewable PR; and
9. GitHub verify and gitleaks before a commissioned merge.

Campaign-level acceptance additionally requires:

- three or more repeated compaction cycles with no lost standing constraint;
- correction and forget removing stale memory from the next projection,
  including from facts already carried inside a compaction checkpoint;
- a durable store at its cap refusing appends with review, export, and delete
  offered, and no transcript trimmed;
- every executed command provably contained to the writable root plus the
  read-only grants;
- zero cross-folder reads without a current grant;
- zero writes to reference folders;
- Stop reaching a settled terminal state during model, command, and verifier
  work;
- relaunch recovery from every settled phase boundary;
- legacy sessions remaining recoverable; and
- measured local-model records tied to hardware, runtime, fixture, and commit.

## Explicit non-goals

- A single all-files or home-directory grant.
- Multiple writable roots in one run.
- Automatic project discovery across the disk.
- Silent transcript deletion or opaque provider-owned compaction as the only
  retained context.
- Treating compaction as durable learning.
- Unreviewed memory extraction or automatic sensitive-trait inference.
- Semantic retrieval before its own evaluated authority design.
- Arbitrary shell/tool/plugin execution.
- Autonomous Browser or macOS control as an implied consequence of chat
  persistence.
- Packaging Qwen3.8-Flash-Next as a default local model.
