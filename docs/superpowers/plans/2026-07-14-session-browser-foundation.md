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

- [ ] Write a real v4 fixture migration test asserting transcript/shelf/fork metadata survives and no Browser rows appear.
- [ ] Write fresh-schema tests pinning foreign keys, uniqueness, ordering, and `ON DELETE CASCADE` across workspace, tabs, and history.
- [ ] Run `cd src-tauri && cargo test sessions::browser_workspace_tests -- --nocapture`; confirm RED.
- [ ] Bump `SCHEMA_VERSION` to 5 and add `migrate_v4_to_v5` to every legacy chain.
- [ ] Define `BrowserWorkspaceRecord`, `BrowserTabRecord`, `BrowserHistoryRecord`, `BrowserLayoutMode`, and restoration status with camelCase serde.
- [ ] Implement backend-minted `bw_`/`bt_` opaque ids, bounded split width validation, and deterministic tab/history ordering.
- [ ] Re-run the focused suite; confirm migration and fresh schema GREEN.

### Task 2: Safe URL admission and restoration records

**Files:**
- Modify: `src-tauri/src/browser/policy.rs`
- Create: `src-tauri/src/browser/restoration.rs`
- Create: `src-tauri/src/browser/restoration_tests.rs`
- Modify: `src-tauri/src/browser/mod.rs`

- [ ] Add failing table tests for public HTTP(S), loopback, credentials, unsupported schemes, NUL/oversize URLs, secret-shaped query/fragment values, and ordinary query strings.
- [ ] Run `cd src-tauri && cargo test browser::restoration_tests -- --nocapture`; confirm RED.
- [ ] Implement `admit_restorable_url`: reuse network validation, preserve safe URL values, and reduce unsafe query/fragment values to origin/path with `manual_reopen_required = true`.
- [ ] Ensure the sanitizer never logs or returns the rejected sensitive original.
- [ ] Add a 20-row append helper that removes forward history after navigation and trims oldest rows deterministically.
- [ ] Re-run policy/restoration tests; confirm GREEN.

### Task 3: Atomic workspace store operations

**Files:**
- Modify: `src-tauri/src/sessions/browser_workspace.rs`
- Modify: `src-tauri/src/sessions/browser_workspace_tests.rs`
- Modify: `src-tauri/src/sessions/mod.rs`
- Modify: `src-tauri/src/sessions/branch.rs`

- [ ] Add failing tests for create/load/save/relaunch, five-tab rejection, duplicate/order corruption, 20-history trimming, scope mismatch `NotFound`, and corrupt-Browser reset without transcript loss.
- [ ] Add deletion/fork/rewind tests: delete cascades; children have no workspace; parent remains unchanged.
- [ ] Implement store functions under the existing per-database serialized lock and immediate transactions.
- [ ] Return a typed `BrowserWorkspaceLoad::{Ready, ResetCorrupt}` rather than mapping Browser validation failure to `SessionStoreError::Corrupt` for the transcript.
- [ ] Re-run all `sessions` tests; confirm GREEN.

### Task 4: Private casual-chat evidence store

**Files:**
- Create: `src-tauri/src/browser/local_evidence.rs`
- Create: `src-tauri/src/browser/local_evidence_tests.rs`
- Modify: `src-tauri/src/browser/mod.rs`
- Modify: `src-tauri/src/commands/sessions.rs`

- [ ] Add failing tests for text/screenshot records beneath app data, session ownership, redaction, symlink/hardlink refusal, hashes, size/count caps, unknown ids, and recursive session deletion.
- [ ] Reuse current evidence record formats and redaction/image validation; do not create a second prompt-manifest shape.
- [ ] Store under an app-private session directory resolved by backend scope, never a caller path.
- [ ] Implement two-phase local deletion: atomically rename the evidence directory to a tombstone, delete the session transaction, restore the directory if the DB delete fails, then remove the inaccessible tombstone. A failed final cleanup may leave only a bounded orphan tombstone, never evidence reachable from another session.
- [ ] Prove a project-session delete never touches app-private evidence.
- [ ] Run `cd src-tauri && cargo test browser::local_evidence_tests -- --nocapture`, then `cd src-tauri && cargo test commands::sessions -- --nocapture`; confirm GREEN.

### Task 5: IPC and TypeScript foundation

**Files:**
- Create: `src-tauri/src/commands/browser_workspace.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/capabilities/default.json`
- Create: `src/lib/api/browserWorkspace.ts`
- Create: `src/lib/api/browserWorkspace.test.ts`

- [ ] Add failing command tests for version, caller allowlist, scope/session mismatch, local/project trust, malformed payloads, caps, reset, and exact camelCase wire shape.
- [ ] Add `browser_workspace_load`, `browser_workspace_save`, and `browser_workspace_reset`; accept no filesystem roots.
- [ ] Register commands through the shared registry/capability and pin parity tests.
- [ ] Add TypeScript tagged types and thin wrappers; test exact command names/payloads.
- [ ] Run `cd src-tauri && cargo test commands::browser_workspace -- --nocapture` and `npm run test -- src/lib/api/browserWorkspace.test.ts`; confirm GREEN.

### Task 6: Docs and full gate

- [ ] Update `docs/IPC_CONTRACT.md`, `docs/SAFETY.md`, `docs/ARCHITECTURE.md`, `docs/FEATURE_INVENTORY.md`, and `docs/SMOKE_TESTING.md` with foundation-only status.
- [ ] State plainly that the existing global Browser UI remains until PR 2 consumes this foundation.
- [ ] Run `cargo test`, `npm run typecheck`, focused frontend tests, and `PLUME_FULL_VERIFY=1 ./scripts/verify.sh`.
- [ ] Publish one focused PR and complete exact-head review/merge before PR 2.
