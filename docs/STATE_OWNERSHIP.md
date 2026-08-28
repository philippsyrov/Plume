# Typed State Ownership

This document answers one question: for each piece of conversation, context,
memory, folder, and run state, which Rust type owns it and where does it live?

It describes **ownership, not implementation status**.
[`FEATURE_INVENTORY.md`](FEATURE_INVENTORY.md) remains the only
repository-wide implementation-status ledger, and its vocabulary
(`shipped`, `partial`, `scaffold`, `researched`, `blocked`, `retired`) is not
used here. A row in the table below says who owns a record, never whether the
surface that uses it is reachable.

Companion documents: [`ARCHITECTURE.md`](ARCHITECTURE.md) for the process and
storage picture, [`SAFETY.md`](SAFETY.md) for the enforcement boundaries, and
[`IPC_CONTRACT.md`](IPC_CONTRACT.md) for wire shapes.

## Owned state today

Every `file:line` below was read against this head. The line points at the
declaration of the named type.

| State | Owning Rust type | `file:line` | Persistence |
| --- | --- | --- | --- |
| Session list row | `SessionSummary` | `src-tauri/src/sessions/mod.rs:106` | SQLite `chat_sessions` |
| Session with transcript | `SessionRecord` | `src-tauri/src/sessions/mod.rs:120` | SQLite `chat_sessions` + `chat_messages` |
| Transcript entry | `TranscriptEntry` | `src-tauri/src/sessions/mod.rs:138` | SQLite `chat_messages` |
| Session ownership scope | `SessionOwnerScope` / `SessionOwnerRef` / `ResolvedSessionOwner` | `src-tauri/src/sessions/owner.rs:10`, `:16`, `:22` | In memory; selects which database directory is opened |
| Session database location | `local_sessions_dir()` / `project_sessions_dir()` | `src-tauri/src/sessions/mod.rs:253`, `:262` | `<app-data>/sessions/state.sqlite` versus `<project>/.plume/sessions/state.sqlite` |
| Context shelf (mutable) | `ContextSourceRef` | `src-tauri/src/prompts/explicit_context.rs:31` | `chat_sessions.context_sources_json` |
| Accepted-turn manifest (immutable) | `ContextSourceManifestItem` | `src-tauri/src/prompts/explicit_context.rs:62` | `chat_messages.context_manifest_json` |
| App-private user memory | `UserMemoryEntry` | `src-tauri/src/memory/user_store.rs:44` | `<app-data>/memory/entries.jsonl` |
| Project memory entry | `MemoryEntry` | `src-tauri/src/memory/types.rs:12` | `<project>/.plume/memory/entries.jsonl` |
| Memory links and backlinks | `MemoryEntry.links` | `src-tauri/src/memory/types.rs:28` | Same `entries.jsonl` row as its entry |
| Project topics | `TopicFile` / `MemoryTopics` | `src-tauri/src/memory/topics.rs:65`, `:102` | Markdown under `<project>/.plume/memory/` |
| Project trust | `TrustStore` | `src-tauri/src/project/trust.rs:37` | `<app-data>/trusted-projects.json` |
| Command argv approval | `ApprovalRecord` | `src-tauri/src/agent/ledger.rs:62` | `<project>/.plume/approvals.json` |
| Agent autonomy configuration | `AgentConfig` | `src-tauri/src/agent/mod.rs:81` | In memory only (`AppState.agent_config`); reset to the default on project open and close |
| Patch checkpoint | `Checkpoint` / `Manifest` | `src-tauri/src/patch/checkpoint.rs:38`, `:53` | `<project>/.plume/checkpoints/<id>/` |
| Browser text evidence | `BrowserEvidenceRecord` | `src-tauri/src/browser/evidence.rs:46` | `<project>/.plume/browser-evidence/` |
| Browser screenshot evidence | `CapturedBrowserScreenshot` / `StoredBrowserScreenshot` | `src-tauri/src/browser/screenshot_evidence.rs:38`, `:74` | `<project>/.plume/browser-evidence/screenshots/` |
| App-private Browser evidence owner | `LocalEvidenceOwner` | `src-tauri/src/browser/local_evidence.rs:51` | `<app-data>/browser-sessions/<sessionId>/` |
| Browser workspace | `BrowserWorkspaceRecord` / `BrowserTabRecord` / `BrowserHistoryRecord` | `src-tauri/src/sessions/browser_workspace.rs:89`, `:78`, `:70` | SQLite `browser_workspaces` / `browser_tabs` / `browser_history` |
| Research artifact bundle | `ArtifactBundleRecord` | `src-tauri/src/research/bundle.rs:101` | `<app-data>/research-artifacts/<sessionId>/` or `<project>/.plume/research-artifacts/<sessionId>/` |
| Research run lease | `ResearchRunRegistry` / `ResearchRunLease` | `src-tauri/src/research/run_registry.rs:24`, `:98` | In memory (`AppState.research_runs`) |
| Chat stream cancellation | `ChatStreamRegistry` | `src-tauri/src/chat/stream.rs:24` | In memory (`AppState.chat_streams`) |

Two rows are easy to misread:

- The **research run lease** is scoped to bounded research notes. It is not a
  general run lease and does not carry a writable root, a file allowlist, or a
  command allowlist.
- The **chat stream registry** is a cancellation map keyed by stream id. It
  grants nothing and does not bound an action.

## Specified but not implemented

The four records below appear in
[`docs/superpowers/specs/2026-08-27-continuous-chat-folder-grants-design.md`](superpowers/specs/2026-08-27-continuous-chat-folder-grants-design.md).
None of them exists in the tree. A grep over `src-tauri/src` at this head finds
no `*Grant*` type, no `*Proposal*` type, no compaction or summarization record,
and no session-store column for a summary — `src-tauri/src/sessions/schema.rs`
carries every migration through v6 and adds none.

There is no half-built version, no unreachable version, and no type to extend.
Each will be introduced whole by the phase named beside it.

| Specified record | Introduced by | Intended ownership |
| --- | --- | --- |
| `CompactionCheckpoint` | Phase 2 — Transparent compaction | An immutable derived summary owned by one conversation, added beside retained history rather than replacing it |
| `MemoryProposal` | Phase 3 — Reviewable learning | A typed candidate memory with provenance, which only an explicit user action can turn into a durable entry |
| `FolderGrant` | Phase 4 — Read-only folder grants | An opaque backend-minted read permission over one canonical folder root |
| `RunLease` | Phase 6 — Writable run leases | A short-lived bound on one actionable task: one writable grant, zero or more read-only grants, and explicit allowlists |

## Invariants

These hold for the owned state today and constrain the four records above.

1. **Derived prose confers no authority.** Compaction summaries, display
   names, titles, and any other generated text are model-context material.
   They never create or restore trust, folder access, approvals, source
   acceptance, memory scope, or model capability. Authority is reconstructed
   from structured backend state on every turn.
2. **A folder grant permits bounded reads only.** Read access to a folder is
   not permission to write, run a command, start a model, act in the Browser,
   or invoke a tool. Each of those has its own gate and its own record.
3. **A run has exactly one writable root.** Additional attached folders are
   read-only for the life of that run. Changing the writable root ends the run
   and requires a fresh visible approval; cross-folder work is separate runs
   with separate patches and checkpoints.
4. **The frontend never supplies a trusted root after grant creation.** The
   canonical path stays Rust-private once the native selection and trust flow
   finish. Callers pass opaque ids, and the backend re-resolves and re-checks
   trust and containment on every operation. This is the existing rule for
   context refs, stated once for grants.
5. **App-private and folder memory remain physically separate stores.**
   `<app-data>/memory/entries.jsonl` and `<project>/.plume/memory/entries.jsonl`
   are never merged, cross-read, or aggregated, and each appears as its own
   exact entry in the prompt manifest.

## Keeping this document true

Change an owning type, a storage location, or one of the invariants, and
update the matching row here in the same slice. If a record moves from the
second table to the first, delete its row from the second table rather than
leaving both — a record is owned or it is specified, never both.
