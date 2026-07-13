# Chat Session Persistence Design

**Date:** 2026-07-10

**Status:** D63 persistence and sidebar are shipped; follow-ups extend the same spine

## Goal

Replace Plume's window-local placeholder chats with durable local-chat and
project-chat sessions. A user can create, reopen, rename, archive, and delete
chats, and completed transcripts return after Plume relaunches. Local chats and
project chats remain separate by construction.

This work establishes the session spine needed by later session search,
compaction, verification history, tool traces, skills, and agent-loop work. It
does not redesign the current white, shaded UI.

## Delivery Boundary

The work is deliberately split into two reviewable slices:

- **D63A - persistence spine:** SQLite stores, typed IPC, validation, and Rust
  tests. No visible UI change.
- **D63B - session UI wiring:** replace fake sidebar rows and remount-based chat
  resets with real persisted sessions. Keep the existing visual system.

D63A must merge before D63B starts. The current D62 UI work must be verified
and committed separately before either slice begins.

## Storage Model

Use SQLite from the first persistence slice. Session search and FTS are already
part of Plume's roadmap, so introducing a temporary JSON transcript format
would create a migration without buying meaningful simplicity.

The same schema is used in two physically separate databases:

- **Local chats:** `<tauri app data>/sessions/state.sqlite`
- **Project chats:** `<trusted project>/.plume/sessions/state.sqlite`

The split is load-bearing:

- Local-chat commands never accept or infer a project root.
- Project-chat commands resolve only through the currently open trusted
  project.
- Opening or closing a project cannot change which database backs local chat.
- A project transcript never appears in the local-chat list, even if a caller
  submits a mismatched session id.

SQLite access lives in Rust. The frontend never receives a database path and
never opens the database directly.

### Dependency

D63A adds `rusqlite` with bundled SQLite so packaged Plume builds do not depend
on a user's separately installed SQLite library. Selecting and fetching the
compatible crate version must follow the repository's dependency approval
rules; no global install is permitted.

### Schema Version 1

The original schema used `PRAGMA user_version = 1`; the current schema is v3.
Enable foreign keys for every connection, and
create these tables inside one initialization transaction:

```sql
CREATE TABLE chat_sessions (
  id TEXT PRIMARY KEY NOT NULL,
  title TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL,
  updated_at_ms INTEGER NOT NULL,
  archived_at_ms INTEGER,
  forked_from_session_id TEXT,
  forked_through_entry_id TEXT
);

CREATE TABLE chat_messages (
  id TEXT PRIMARY KEY NOT NULL,
  session_id TEXT NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
  ordinal INTEGER NOT NULL,
  kind TEXT NOT NULL,
  role TEXT,
  content TEXT NOT NULL,
  model_used TEXT,
  duration_ms INTEGER,
  attachment_rel_path TEXT,
  attachment_start_line INTEGER,
  attachment_end_line INTEGER,
  stats_json TEXT,
  sent_in_mode TEXT,
  created_at_ms INTEGER NOT NULL,
  UNIQUE(session_id, ordinal)
);

CREATE INDEX chat_sessions_updated_idx
  ON chat_sessions(archived_at_ms, updated_at_ms DESC);
```

The lineage columns intentionally have no foreign key, so deleting a source
does not erase provenance. Continuing a thread validates and copies all
persisted rows with fresh ids in one IMMEDIATE transaction. Ephemeral draft,
selection, provider, stream, tool, and process state are not copied.
`forkedThroughEntryId` records the last copied source-message id at that moment;
it is deliberately opaque provenance rather than a resolvable foreign key.
Later source replacement or deletion can remove that id while the child keeps
the lineage strings and copied transcript as durable evidence.

`kind` preserves the visible transcript distinction between `message`,
`cancelled`, and `error`. Streaming placeholders are never persisted. `role`
is required for `message` rows and null for cancelled/error rows. `stats_json`
contains only the existing bounded `ChatStats` shape; arbitrary frontend state
is not accepted.

Opaque session and message ids are minted in Rust and validated before lookup.
They are not paths and must not contain user-controlled path fragments.

## Typed IPC

Add a new `sessions` command module and matching TypeScript wrapper. The wire
uses `scope: 'local' | 'project'`; no command accepts a filesystem root.

```text
sessions.list({ scope, includeArchived? })
  -> { sessions: SessionSummary[] }

sessions.create({ scope, title? })
  -> { session: SessionSummary }

sessions.load({ scope, sessionId })
  -> { session: SessionRecord }

sessions.rename({ scope, sessionId, title })
  -> { session: SessionSummary }

sessions.archive({ scope, sessionId, archived })
  -> { session: SessionSummary }

sessions.delete({ scope, sessionId })
  -> { ok: true }

sessions.saveTranscript({ scope, sessionId, entries })
  -> { session: SessionSummary }
```

`SessionSummary` contains `id`, `title`, `createdAtMs`, `updatedAtMs`, and
`archivedAtMs`. `SessionRecord` adds `entries` in the existing visible-chat
shape, excluding `streaming`.

`saveTranscript` validates the complete snapshot, then replaces that session's
message rows inside one SQLite transaction and updates `updated_at_ms`. This is
acceptable for the first slice because transcripts are bounded and saves occur
at turn boundaries, not per token. A later structured event log can supersede
snapshot persistence without changing session identity.

## Validation And Limits

All session commands use the existing IPC envelope and typed `IpcError`
patterns.

Initial hard limits:

- 200 non-deleted sessions per database.
- 500 persisted transcript entries per session.
- 256 KiB maximum content per entry.
- 8 MiB maximum serialized transcript per session.
- Session title: trimmed, 1-120 Unicode scalar values.
- Attachment path: existing project-relative validation; attachments are
  rejected for local scope.
- Line ranges must be both present or both absent and satisfy
  `1 <= startLine <= endLine`.

Project-scope commands require a currently open trusted project. Local-scope
commands remain available without a project and must not touch project state,
`AGENTS.md`, project memory, or project files.

Reject malformed persisted rows on load instead of silently coercing them.
Database initialization, migration, and writes must fail with typed errors; a
storage failure must not crash the Tauri process.

SQLite operations use a scoped Rust mutex per database path so two commands in
one process cannot interleave initialization or replacement writes. SQL values
are always bound parameters.

## Save Lifecycle

The frontend continues to own live token rendering. Persistence occurs only at
stable boundaries:

1. Creating a chat immediately persists an empty session.
2. After `chat.send` is accepted, save the transcript containing the new user
   turn but excluding the streaming placeholder.
3. Do not write on `chat/token`.
4. On `chat/done`, stopped, or error, save the terminal visible transcript.
5. Rename/archive/delete update the database first; React state changes only
   after success.

If Plume exits during generation, the accepted user turn remains visible after
restart without pretending the missing assistant response completed.

Only one session may stream in a window at a time in D63B, matching the current
`useChat` behavior. Switching sessions while a stream is active is blocked with
a visible explanation; it does not silently cancel or detach the stream.

## D63B UI Behavior

The existing unified sidebar becomes a renderer for persisted summaries:

- The **Chats** section lists non-archived local sessions ordered by
  `updatedAtMs DESC`.
- Each project row lists only sessions from that project's project database.
- The top-level New chat action creates and selects a local session.
- The plus button beside the active project creates and selects a project
  session.
- Clicking a row loads its transcript and updates the central chat surface.
- The row menu supports Rename, Archive, and Delete. Delete requires explicit
  confirmation because it is permanent.
- Archived chats are hidden from the normal list. An `Archived chats` action
  at the bottom of each scope opens a compact Plume-styled modal listing that
  scope's archived sessions with Unarchive and Delete actions. Search and a
  full history screen remain outside D63B.
- Empty lists show one quiet empty state and a new-chat action.

Simple chats never show Files, project attachments, project instructions,
project memory, or the project tool drawer. Project chats keep those project
capabilities. Existing model selection remains window-local in D63.

Do not use `window.prompt`, `window.confirm`, or native browser dialogs. Rename
and delete confirmation use Plume-styled inline/popover or modal controls with
stable accessible names and keyboard support.

## Frontend Boundaries

Do not add session persistence directly to `App.tsx` or enlarge `useChat.ts`
with database orchestration. Introduce focused units:

- `src/lib/api/sessions.ts` - wire types and IPC wrappers.
- `src/features/sessions/useSessions.ts` - list/create/select/mutate state.
- `src/features/sessions/usePersistedChat.ts` - bridge stable `useChat`
  transcript boundaries to `sessions.saveTranscript`.
- `src/features/sessions/SessionRow.tsx` - accessible row and menu.
- `src/features/sessions/SessionDialogs.tsx` - rename/delete UI.

`ChatPanel` remains responsible for chat presentation. `useChat` remains
responsible for streaming orchestration. Session persistence consumes their
public state rather than duplicating model-stream logic.

## Rust Boundaries

Keep storage, command validation, and Tauri registration separate:

- `src-tauri/src/sessions/mod.rs` - public store types and operations.
- `src-tauri/src/sessions/schema.rs` - connection initialization and v1 schema.
- `src-tauri/src/sessions/validation.rs` - ids, titles, entries, and caps.
- `src-tauri/src/sessions/tests.rs` - store-level tests.
- `src-tauri/src/commands/sessions.rs` - trust/scope resolution and IPC handlers.
- `src-tauri/src/main.rs` - command registration only.

Do not put SQL or filesystem-root selection in frontend code. Do not route
sessions through the provider trait; sessions belong to the Plume application
runtime, not a model provider.

## Testing

### D63A Rust tests

Cover at minimum:

- Local and project stores use different roots and cannot cross-load ids.
- Project commands reject without a trusted open project.
- Create/list ordering by latest update.
- Rename trimming and title bounds.
- Archive hide/include/unarchive behavior.
- Delete cascades messages and is idempotency-defined explicitly: first delete
  succeeds, unknown/already-deleted id returns `NotFound`.
- Transcript snapshot round-trip for message, cancelled, and error entries.
- Attachment metadata round-trip for project scope.
- Local scope rejects attachment metadata.
- Streaming entry kind, system/tool roles, malformed ranges, unknown mode,
  oversize entries, excessive entry count, and excessive transcript size are
  rejected.
- Replacement is atomic when validation or insertion fails.
- Foreign keys and schema version are active on a reopened database.
- Symlinked project `.plume` or sessions directory is refused using the same
  defensive posture as memory/checkpoints.

### D63B frontend tests

Cover at minimum:

- Local and project lists render separately.
- New local chat calls create with `scope: 'local'`.
- Project plus calls create with `scope: 'project'`.
- Selecting a row loads the correct transcript.
- Rename/archive/delete update the row only after successful IPC.
- Failed mutations remain visible and announce the error.
- Per-token updates do not call `saveTranscript`.
- Accepted user turn and each terminal outcome do call `saveTranscript`.
- Session switching is blocked during streaming.
- Simple chat renders no Files or project attachment affordance.
- Project chat retains Files/tool-drawer access.
- Relaunch fixture selects and restores the most recently updated transcript
  in the active scope without mixing scopes.

Run focused tests throughout, then finish each slice with:

```bash
npm run test
npx tsc --noEmit
./scripts/verify.sh
```

Run the packaged app smoke harness after D63B and verify create, rename,
archive, delete, project separation, relaunch restoration, and active-stream
switch blocking through the visible UI. Do not start or download a local model
for this smoke; mock/disabled chat state is sufficient for persistence UI, and
the user's laptop should remain cool.

## Documentation Updates

D63A updates `docs/IPC_CONTRACT.md`, `docs/IPC_ROADMAP.md`,
`docs/ARCHITECTURE.md`, and the project status in `AGENTS.md` only after the
implementation and tests prove the final wire shape.

D63B updates `docs/MANUAL_TESTING.md`, `docs/SMOKE_TESTING.md`,
`docs/AGENT_OPERABILITY.md`, and `docs/UI_STYLE.md` for the visible workflow.

Documentation must describe landed behavior, not planned behavior. Do not
inflate the cargo/frontend test counts without running the relevant suites.

## Explicit Non-Goals

- Session search or SQLite FTS tables.
- Automatic transcript compaction.
- Memory distillation from sessions.
- Persisted model selection.
- Multiple simultaneous streams.
- Terminal, command execution, verifier output, or tool traces.
- Skills, MCP, plugins, browser/computer-use, subagents, or scheduled work.
- Cloud sync or cross-device history.
- Importing old window-local chats; none are durable today.
- Restyling the project shell beyond session controls required by D63B.

## Acceptance

D63 is complete only when:

1. A local chat survives app relaunch and appears only under Chats.
2. A project chat survives app relaunch and appears only beneath its project.
3. Rename, archive, unarchive, and delete are durable.
4. No token-by-token database writes occur.
5. A project switch cannot expose another project's transcript.
6. Simple chat cannot attach or access project files/context.
7. Storage failures are visible and recoverable without crashing Plume.
8. D63A and D63B each pass their focused tests and `./scripts/verify.sh`.
9. D63B's packaged-app workflow is visually checked without running a model.
