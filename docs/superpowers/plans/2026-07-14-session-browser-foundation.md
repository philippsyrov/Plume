# Session Browser Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Browser workspaces durable, bounded, scope-safe children of persisted chat sessions, including private casual-chat evidence and clean deletion/migration behavior.

**Architecture:** Bump the session database to schema v5 and add normalized `browser_workspaces`, `browser_tabs`, and `browser_history` relations with foreign-key cascade from `chat_sessions`. Expose validated load/save/reset commands keyed only by scope and opaque session id. Keep live WKWebViews out of this slice; this PR establishes the durable contract consumed by the integrated Browser.

**Tech Stack:** Rust, rusqlite, serde, Tauri commands, TypeScript IPC wrappers, cargo tests, Vitest.

## Global Constraints

- Preserve physical separation: local DB in app data, project DB under the trusted project's `.plume/sessions`.
- Existing v1-v4 databases migrate additively; transcript corruption and Browser corruption remain separate outcomes.
- Maximum five tabs; maximum 20 admitted history rows per tab; ids are backend-minted opaque values.
- Persist only validated top-level HTTP(S) URLs. Strip unsafe query/fragment data and set `manualReopenRequired`.
- Never persist credentials, cookies, form values, DOM state, scroll state, or JavaScript state.
- Fork/rewind children start with an empty Browser workspace. Delete cascades Browser descriptors and session-owned local evidence.

---

### Task 1: Schema v5 and Browser domain contracts

**Files:**
- Modify: `src-tauri/src/sessions/schema.rs`
- Create: `src-tauri/src/sessions/browser_workspace.rs`
- Create: `src-tauri/src/sessions/browser_workspace_tests.rs`
- Modify: `src-tauri/src/sessions/mod.rs`

**Interfaces:**
- `BrowserLayoutMode::{Split, Expanded}`.
- `BrowserWorkspaceRecord { session_id, scope, layout_mode, split_width_px, active_tab_id, tabs, recovery }`.
- `BrowserTabRecord { id, position, current_history_index, manual_reopen_required, restoration_status }`.
- `BrowserHistoryRecord { position, url, recorded_at_ms }`.
- `load_browser_workspace`, `replace_browser_workspace`, `reset_browser_workspace` operate under the existing per-database mutex.

The migration must use normalized rows rather than opaque JSON:

```sql
CREATE TABLE browser_workspaces (
  session_id TEXT PRIMARY KEY REFERENCES chat_sessions(id) ON DELETE CASCADE,
  layout_mode TEXT NOT NULL,
  split_width_px INTEGER NOT NULL,
  active_tab_id TEXT,
  updated_at_ms INTEGER NOT NULL
);
CREATE TABLE browser_tabs (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES browser_workspaces(session_id) ON DELETE CASCADE,
  position INTEGER NOT NULL,
  current_history_index INTEGER NOT NULL,
  manual_reopen_required INTEGER NOT NULL,
  UNIQUE(session_id, position)
);
CREATE TABLE browser_history (
  tab_id TEXT NOT NULL REFERENCES browser_tabs(id) ON DELETE CASCADE,
  position INTEGER NOT NULL,
  url TEXT NOT NULL,
  recorded_at_ms INTEGER NOT NULL,
  PRIMARY KEY(tab_id, position)
);
```

- [ ] Write a real v4 fixture migration test asserting transcript/shelf/fork metadata survives and no Browser rows appear.
- [ ] Write fresh-schema tests pinning foreign keys, uniqueness, ordering, and `ON DELETE CASCADE` across workspace, tabs, and history.
- [ ] Run `cd src-tauri && cargo test sessions::browser_workspace_tests -- --nocapture`; confirm RED.
- [ ] Expected RED: unresolved `browser_workspace` module / absent schema tables; a test that unexpectedly passes is incorrectly exercising v4 behavior.
- [ ] Bump `SCHEMA_VERSION` to 5 and add `migrate_v4_to_v5` to every legacy chain.
- [ ] Define `BrowserWorkspaceRecord`, `BrowserTabRecord`, `BrowserHistoryRecord`, `BrowserLayoutMode`, and restoration status with camelCase serde.
- [ ] Implement backend-minted `bw_`/`bt_` opaque ids, bounded split width validation, and deterministic tab/history ordering.
- [ ] Re-run the focused suite; confirm migration and fresh schema GREEN.
- [ ] Commit: `feat: add session browser schema`.

### Task 2: Safe URL admission and restoration records

**Files:**
- Modify: `src-tauri/src/browser/policy.rs`
- Create: `src-tauri/src/browser/restoration.rs`
- Create: `src-tauri/src/browser/restoration_tests.rs`
- Modify: `src-tauri/src/browser/mod.rs`

**Interfaces:**
- `RestorableUrl { value: String, manual_reopen_required: bool }`.
- `admit_restorable_url(raw: &str) -> Result<RestorableUrl, BrowserUrlError>`.
- `append_history(history, current_index, admitted) -> (bounded_history, new_index)`.

Pin the unsafe-value behavior directly:

```rust
let admitted = admit_restorable_url("https://example.com/path?token=sk-secret#x")?;
assert_eq!(admitted.value, "https://example.com/path");
assert!(admitted.manual_reopen_required);
assert!(!format!("{admitted:?}").contains("sk-secret"));
```

- [ ] Add failing table tests for public HTTP(S), loopback, credentials, unsupported schemes, NUL/oversize URLs, secret-shaped query/fragment values, and ordinary query strings.
- [ ] Run `cd src-tauri && cargo test browser::restoration_tests -- --nocapture`; confirm RED.
- [ ] Expected RED: missing `admit_restorable_url` and history helper.
- [ ] Implement `admit_restorable_url`: reuse network validation, preserve safe URL values, and reduce unsafe query/fragment values to origin/path with `manual_reopen_required = true`.
- [ ] Ensure the sanitizer never logs or returns the rejected sensitive original.
- [ ] Add a 20-row append helper that removes forward history after navigation and trims oldest rows deterministically.
- [ ] Re-run policy/restoration tests; confirm GREEN.
- [ ] Commit: `feat: validate browser restoration urls`.

### Task 3: Atomic workspace store operations

**Files:**
- Modify: `src-tauri/src/sessions/browser_workspace.rs`
- Modify: `src-tauri/src/sessions/browser_workspace_tests.rs`
- Modify: `src-tauri/src/sessions/mod.rs`
- Modify: `src-tauri/src/sessions/branch.rs`

**Interfaces:**
- `BrowserWorkspaceLoad::{Missing, Ready(BrowserWorkspaceRecord), ResetCorrupt { reason }}`.
- `replace_browser_workspace` replaces all workspace/tab/history rows in one immediate transaction.
- Fork/rewind call no Browser copy helper; cascade ownership stays on the new child id only after its first save.

- [ ] Add failing tests for create/load/save/relaunch, five-tab rejection, duplicate/order corruption, 20-history trimming, scope mismatch `NotFound`, and corrupt-Browser reset without transcript loss.
- [ ] Add deletion/fork/rewind tests: delete cascades; children have no workspace; parent remains unchanged.
- [ ] Run `cd src-tauri && cargo test sessions::browser_workspace_tests -- --nocapture`; expected RED is missing store functions/schema mapping.
- [ ] Implement store functions under the existing per-database serialized lock and immediate transactions.
- [ ] Return a typed `BrowserWorkspaceLoad::{Ready, ResetCorrupt}` rather than mapping Browser validation failure to `SessionStoreError::Corrupt` for the transcript.
- [ ] Re-run all `sessions` tests; confirm GREEN.
- [ ] Run `cd src-tauri && cargo test sessions::browser_workspace_tests -- --nocapture` first; expected GREEN includes v4 migration, corrupt reset, cascade, caps, and scope mismatch.
- [ ] Commit: `feat: persist bounded browser workspaces`.

### Task 4: Private casual-chat evidence store

**Files:**
- Create: `src-tauri/src/browser/local_evidence.rs`
- Create: `src-tauri/src/browser/local_evidence_tests.rs`
- Modify: `src-tauri/src/browser/mod.rs`
- Modify: `src-tauri/src/commands/sessions.rs`

**Interfaces:**
- `LocalEvidenceOwner { session_id }` is resolved from the local session database, never a caller path.
- `stage_local_evidence_delete`, `restore_local_evidence_delete`, and `finish_local_evidence_delete` implement the two-phase tombstone protocol.
- Existing `BrowserEvidenceSummary` / `BrowserScreenshotSummary` wire shapes stay shared; stored records gain an internal owner.

- [ ] Add failing tests for text/screenshot records beneath app data, session ownership, redaction, symlink/hardlink refusal, hashes, size/count caps, unknown ids, and recursive session deletion.
- [ ] Run `cd src-tauri && cargo test browser::local_evidence_tests -- --nocapture`; expected RED is the absent local evidence module/store.
- [ ] Reuse current evidence record formats and redaction/image validation; do not create a second prompt-manifest shape.
- [ ] Store under an app-private session directory resolved by backend scope, never a caller path.
- [ ] Implement two-phase local deletion: atomically rename the evidence directory to a tombstone, delete the session transaction, restore the directory if the DB delete fails, then remove the inaccessible tombstone. A failed final cleanup may leave only a bounded orphan tombstone, never evidence reachable from another session.
- [ ] Prove a project-session delete never touches app-private evidence.
- [ ] Run `cd src-tauri && cargo test browser::local_evidence_tests -- --nocapture`, then `cd src-tauri && cargo test commands::sessions -- --nocapture`; confirm GREEN.
- [ ] Commit: `feat: add private local browser evidence`.

### Task 5: IPC and TypeScript foundation

**Files:**
- Create: `src-tauri/src/commands/browser_workspace.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/capabilities/default.json`
- Create: `src/lib/api/browserWorkspace.ts`
- Create: `src/lib/api/browserWorkspace.test.ts`

**Interfaces:**
- `SessionIdentity = { scope: SessionScope; sessionId: string }` is exported from `src/lib/api/sessions.ts` and reused by Browser/chat plans.
- `browserWorkspaceLoad({ identity }) -> { workspace, recoveryNotice }`.
- `browserWorkspaceSave({ identity, workspace }) -> { workspace }`.
- `browserWorkspaceReset({ identity }) -> { workspace }`.

```ts
export type SessionIdentity = {
  scope: 'local' | 'project';
  sessionId: string;
};

export type BrowserWorkspaceLoadResponse = {
  workspace: BrowserWorkspace | null;
  recoveryNotice: 'browserStateReset' | null;
};
```

- [ ] Add failing command tests for version, caller allowlist, scope/session mismatch, local/project trust, malformed payloads, caps, reset, and exact camelCase wire shape.
- [ ] Run `cd src-tauri && cargo test commands::browser_workspace -- --nocapture` and `npm run test -- src/lib/api/browserWorkspace.test.ts`; expected RED is missing Rust commands and TypeScript wrappers.
- [ ] Add `browser_workspace_load`, `browser_workspace_save`, and `browser_workspace_reset`; accept no filesystem roots.
- [ ] Register commands through the shared registry/capability and pin parity tests.
- [ ] Add TypeScript tagged types and thin wrappers; test exact command names/payloads.
- [ ] Run `cd src-tauri && cargo test commands::browser_workspace -- --nocapture` and `npm run test -- src/lib/api/browserWorkspace.test.ts`; confirm GREEN.
- [ ] Commit: `feat: expose session browser persistence`.

### Task 6: Docs and full gate

- [ ] Update `docs/IPC_CONTRACT.md`, `docs/SAFETY.md`, `docs/ARCHITECTURE.md`, `docs/FEATURE_INVENTORY.md`, and `docs/SMOKE_TESTING.md` with foundation-only status.
- [ ] State plainly that the existing global Browser UI remains until PR 2 consumes this foundation.
- [ ] Run `cd src-tauri && cargo test`, `npm run typecheck`, `npm run test -- src/lib/api/browserWorkspace.test.ts`, and `PLUME_FULL_VERIFY=1 ./scripts/verify.sh`; expected final result is zero failures with only documented soft warnings.
- [ ] Publish one focused PR and complete exact-head review/merge before PR 2.
