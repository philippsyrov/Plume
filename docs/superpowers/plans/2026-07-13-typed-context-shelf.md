# Typed Explicit Context Shelf Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a durable, explicit, typed per-project-session context shelf for project files/selections, exact memory entries, and curated topic files, with all-or-nothing backend resolution and immutable exact per-turn manifests.

**Architecture:** Add one shared tagged source-reference contract and one resolved-manifest contract across TypeScript and Rust. Resolve references only in Rust through a focused `prompts::explicit_context` module, persist the ordered shelf and accepted turn manifests as validated JSON in session schema v4, and let the existing serialized session-save queue own shelf persistence. Replace the one-shot attachment chip with a sticky shelf; Knowledge and the inspector add references through the hoisted `ChatApi`.

**Tech Stack:** Rust, serde, rusqlite, Tauri 2 commands, TypeScript, React 19, Vitest, Testing Library.

## Global Constraints

- Read `AGENTS.md` first; `docs/PLUME_PROJECT_SPEC.md` remains product truth.
- New frontend code sends references only; no frontend-supplied prompt text.
- The shelf is project-session-only, ordered, sticky, capped at 16 references, and deduplicated by first insertion identity.
- Backend send resolution is all-or-nothing and capped at 256 KiB total explicit content.
- Memory-topic links remain organization metadata only and never select context.
- Legacy singular `attachment` stays wire-compatible; new frontend code uses `contextSources` only.
- Preserve trust, path, symlink, hardlink, secret-name, binary, redaction, and owning-store caps.
- No semantic retrieval, agent-selected context, browser, computer-use, or broad tool execution ships here.

---

### Task 1: Shared Rust source contracts and resolver

**Files:**
- Create: `src-tauri/src/prompts/explicit_context.rs`
- Modify: `src-tauri/src/prompts/mod.rs`
- Modify: `src-tauri/src/prompts/assemble.rs`
- Test: `src-tauri/src/prompts/explicit_context_tests.rs`
- Test: `src-tauri/src/prompts/assemble_tests.rs`

**Interfaces:**
- Produces: `ContextSourceRef`, `ContextSourceManifestItem`, `ContextSourcePreviewItem`, `resolve_for_preview`, and `resolve_for_send`.
- Produces: `AssembledPrompt.explicit_context: Vec<ContextSourceManifestItem>`.
- Consumes: existing prompt file reader/redactor and memory/topic stores.

- [ ] **Step 1: Write failing identity, ordering, and cap tests**

Add tests that construct mixed references and assert first-position dedupe, range-sensitive file identity, stable request order, rejection above 16 references, and rejection above the 256-KiB aggregate. Include a link-only memory fixture and assert it produces no explicit sources.

```rust
assert_eq!(dedupe_refs(&[file("a.rs"), memory(ID), file("a.rs")])?,
           vec![file("a.rs"), memory(ID)]);
assert!(validate_refs(&vec![memory(ID); 17]).is_err());
assert!(resolve_for_send(root, &refs_over_budget()).is_err());
```

- [ ] **Step 2: Run the focused tests and confirm RED**

Run: `cd src-tauri && cargo test prompts::explicit_context_tests -- --nocapture`

Expected: compile failure because the module and contracts do not exist.

- [ ] **Step 3: Implement tagged contracts and deterministic validation**

Define camelCase serde enums matching the design, `MAX_EXPLICIT_CONTEXT_SOURCES = 16`, `EXPLICIT_CONTEXT_BYTE_CAP = 256 * 1024`, stable identity keys, first-position dedupe, and typed preview outcomes. Keep prompt content private in an internal resolved struct.

- [ ] **Step 4: Add exact owning-store resolution tests**

Cover whole-file and line-range post-redaction bytes, missing/blocked files, exact memory id and deletion, strict `topics/<flat-name>.md`, missing/oversize/symlink/hardlink topics, and preview/send manifest equality for unchanged storage.

```rust
let preview = resolve_for_preview(root, &refs);
let sent = resolve_for_send(root, &refs).unwrap();
assert_eq!(ready_manifests(preview), sent.manifest);
assert_eq!(sent.manifest.iter().map(kind), ["projectFile", "memoryEntry", "topicFile"]);
```

- [ ] **Step 5: Implement resolution and bounded prompt rendering**

Reuse `prompts::read::read_for_prompt` for files, add exact-id lookup in the memory store without exposing raw input over IPC, and add exact canonical topic read under the existing 8-KiB cap. Render one inert, labelled system block preserving request order. Reject before modifying messages if any source fails.

- [ ] **Step 6: Integrate explicit context into `assemble`**

Extend `assemble(project_root, messages, attachment, context_sources, mode)`; reject simultaneous legacy attachment plus new refs; normalize a legacy attachment to one file ref. Exclude explicit memory ids from ambient memory selection. Return the exact manifest.

- [ ] **Step 7: Run focused backend tests and format**

Run:

```bash
cd src-tauri
cargo fmt --all -- --check
cargo test prompts::explicit_context_tests -- --nocapture
cargo test prompts::assemble_tests -- --nocapture
```

Expected: all focused tests pass.

### Task 2: IPC preview/send exact-manifest contract

**Files:**
- Modify: `src-tauri/src/commands/chat.rs`
- Modify: `src-tauri/src/commands/chat/context.rs`
- Modify: `src-tauri/src/commands/chat/send.rs`
- Modify: `src-tauri/src/commands/chat/validate.rs`
- Test: `src-tauri/src/commands/chat/context_tests.rs`
- Test: `src-tauri/src/commands/chat/send_tests.rs`
- Modify: `src/lib/api/chat.ts`

**Interfaces:**
- Consumes: Task 1 resolver contracts.
- Produces: `ChatContextRequest.context_sources`, `ChatContextResponse.context_sources`, `ChatSendPayload.context_sources`, and `ChatSendStartedResponse.context_sources`.

- [ ] **Step 1: Write failing wire-shape and validation tests**

Pin camelCase serialization for all three refs/manifests/outcomes. Assert both legacy attachment and `contextSources` reject, local/no-trust explicit refs reject, duplicate refs normalize, and omitted fields preserve old request compatibility.

- [ ] **Step 2: Run focused command tests and confirm RED**

Run: `cd src-tauri && cargo test commands::chat -- --nocapture`

Expected: failures for missing fields/types.

- [ ] **Step 3: Extend Rust and TypeScript IPC types**

Add the design's exact tagged unions. Keep `attachment` and its existing response summary. Add `contextSources` arrays with serde defaults so old payloads deserialize unchanged.

- [ ] **Step 4: Wire preview outcomes and exact send manifests**

`chat.context` resolves every requested source and returns all outcomes. `chat.send` resolves all sources before stream registration and returns the resolver manifest. Verify a stale memory/topic/file returns no registered stream and no provider call.

- [ ] **Step 5: Run focused command tests and TypeScript typecheck**

Run:

```bash
cd src-tauri
cargo test commands::chat -- --nocapture
cd ..
npm run typecheck
```

Expected: command tests and typecheck pass.

### Task 3: Session schema v4 and atomic shelf/provenance persistence

**Files:**
- Modify: `src-tauri/src/sessions/schema.rs`
- Modify: `src-tauri/src/sessions/mod.rs`
- Modify: `src-tauri/src/sessions/validation.rs`
- Modify: `src-tauri/src/sessions/branch.rs`
- Modify: `src-tauri/src/commands/sessions.rs`
- Test: `src-tauri/src/sessions/tests.rs`
- Test: `src-tauri/src/sessions/fork_tests.rs`
- Test: `src-tauri/src/sessions/rollback_tests.rs`
- Modify: `src/lib/api/sessions.ts`

**Interfaces:**
- Consumes: Task 1 reference/manifest serde types.
- Produces: `SessionRecord.context_sources` and `TranscriptEntry::Message.context_sources`.
- Produces: `sessions.saveTranscript({ entries, contextSources })` atomic replacement.

- [ ] **Step 1: Write failing v3 migration and fresh-v4 tests**

Build an actual v3 database, reopen it, and assert `user_version = 4`, old session shelf is empty, and old attachment turns still load. Pin both new columns in a fresh schema.

- [ ] **Step 2: Write failing store-boundary tests**

Cover ordered save/load/relaunch, malformed JSON, unknown kinds, invalid ids/paths/topic names, source cap, manifest on assistant/cancelled/error, local non-empty shelf/turn-manifest rejection, session separation, and delete cascade.

- [ ] **Step 3: Implement schema migration and validated JSON mapping**

Add nullable columns, serde encode/decode helpers, and semantic validation. Empty vectors serialize as `NULL` to keep old/simple rows compact. Treat malformed persisted JSON as corruption, never empty fallback.

- [ ] **Step 4: Make save atomic across transcript and shelf**

Extend the existing immediate transaction so the session shelf update and transcript replacement commit together. Include the shelf in all load/save responses.

- [ ] **Step 5: Pin fork/rewind semantics**

Add tests proving child `context_sources` is empty while retained user turns preserve `context_manifest_json`. Extend branch row copy accordingly; do not copy the parent's current shelf.

- [ ] **Step 6: Run session tests**

Run: `cd src-tauri && cargo test sessions -- --nocapture`

Expected: all session, fork, rollback, migration, and command tests pass.

### Task 4: Frontend shelf state and persistence bridge

**Files:**
- Create: `src/features/chat/contextSources.ts`
- Modify: `src/features/chat/useChat.ts`
- Modify: `src/features/sessions/transcript.ts`
- Modify: `src/features/sessions/usePersistedChat.ts`
- Test: `src/features/chat/contextSources.test.ts`
- Test: `src/features/chat/useChat.test.tsx`
- Test: `src/features/sessions/transcript.test.ts`
- Test: `src/features/sessions/usePersistedChat.test.tsx`

**Interfaces:**
- Produces: `contextSourceKey`, `addContextSource`, `removeContextSource`, and shelf-aware `restore`.
- Consumes: Tasks 2 and 3 TypeScript contracts.

- [ ] **Step 1: Write failing pure identity/order tests**

Assert identical files dedupe, different line ranges coexist, memory/topic opaque identities dedupe, first insertion wins, remove is exact, and the 17th distinct source returns `full`.

- [ ] **Step 2: Implement pure shelf operations**

Keep the helper free of React and IPC. Never inspect memory/topic text or links.

- [ ] **Step 3: Write failing `useChat` acceptance tests**

Assert send captures an immutable shelf snapshot, passes it to IPC, blocks shelf mutation while streaming, marks explicit-source user rows non-persistable before accept, stamps the exact backend manifest on accept, and removes pending state without a manifest on reject.

- [ ] **Step 4: Implement shelf-aware `useChat`**

Expose ordered refs and mutation methods on `ChatApi`. Preserve sticky refs after accept/reject. Extend `restore(entries, refs)` and `clear()` so transcript clear does not silently clear the session shelf.

- [ ] **Step 5: Write failing persistence queue tests**

Assert shelf-only change creates/saves a session, ordered refs ride on every queued save, selecting restores transcript+shelf together, stale load cannot overwrite the newer surface, project shelves never save to local scope, and turn manifests round-trip.

- [ ] **Step 6: Implement shelf-aware transcript/session bridge**

Compare refs structurally in the boundary detector, capture shelf snapshots with the same active scope/id as transcript snapshots, and pass refs to `saveSessionTranscript`. Update mappers for accepted manifests and legacy attachment fallback.

- [ ] **Step 7: Run focused frontend state tests**

Run:

```bash
npx vitest run src/features/chat/contextSources.test.ts src/features/chat/useChat.test.tsx src/features/sessions/transcript.test.ts src/features/sessions/usePersistedChat.test.tsx
npm run typecheck
```

Expected: all focused tests and typecheck pass.

### Task 5: Visible Context shelf and file/selection action

**Files:**
- Create: `src/features/chat/ContextShelf.tsx`
- Create: `src/features/chat/ContextShelf.test.tsx`
- Modify: `src/features/chat/AttachBar.tsx`
- Modify: `src/features/chat/ChatPanel.tsx`
- Modify: `src/features/chat/useChatContextPreview.ts`
- Modify: `src/features/chat/ContextPreview.tsx`
- Modify: `src/features/chat/ChatEntryRow.tsx`
- Modify: `src/styles/chat.css`
- Test: `src/features/chat/ChatPanel.test.tsx`
- Test: `src/features/chat/ChatEntryRow.test.tsx`

**Interfaces:**
- Consumes: Task 4 `ChatApi`, Task 2 preview outcomes.
- Produces: accessible shelf UI, blocked-state recovery, and exact per-turn provenance rendering.

- [ ] **Step 1: Write failing shelf component tests**

Cover ordered ready/checking/blocked items, typed labels, remove, retry, disabled mutation while streaming, local-chat absence, and no hidden mutation controls.

- [ ] **Step 2: Implement `ContextShelf` and preview hook**

Replace singular attachment preview inputs with the full ref array and generation-guard every request. Render one item outcome per ref; retain the ref's label while checking or blocked.

- [ ] **Step 3: Convert the file action from one-shot chip to shelf add**

Keep existing eligibility and selection-range rules. Rename copy to **Use file in chat** / **Use selection in chat**. Duplicate is a harmless no-op; full shelf shows a visible error. Do not clear after send.

- [ ] **Step 4: Render immutable accepted turn manifests**

Render one compact provenance row under the user turn in manifest order. Continue rendering legacy attachment metadata for old rows.

- [ ] **Step 5: Run focused chat UI tests**

Run:

```bash
npx vitest run src/features/chat/ContextShelf.test.tsx src/features/chat/ChatPanel.test.tsx src/features/chat/ChatEntryRow.test.tsx src/features/chat/useChatContextPreview.test.tsx
npm run typecheck
```

Expected: focused UI tests and typecheck pass.

### Task 6: Knowledge **Use in chat** actions and shell wiring

**Files:**
- Modify: `src/features/knowledge/KnowledgePanel.tsx`
- Modify: `src/features/knowledge/KnowledgeMemoryCard.tsx`
- Modify: `src/features/knowledge/KnowledgeTopicCard.tsx`
- Modify: `src/features/knowledge/KnowledgePanel.test.tsx`
- Modify: `src/features/project-shell/ProjectWorkspace.tsx`
- Modify: `src/App.tsx`
- Test: `src/App.test.tsx`

**Interfaces:**
- Consumes: Task 4 hoisted `ChatApi` and navigation callback.
- Produces: manual opaque-reference placement from Knowledge into the active project chat.

- [ ] **Step 1: Write failing Knowledge action tests**

Assert memory uses only `{kind:'memoryEntry', entryId}`, canonical topic uses only `{kind:'topicFile', name}`, links do not add anything, success/duplicate navigates to Chat, and full/unavailable errors stay visible without navigation.

- [ ] **Step 2: Implement card actions**

Add minimal accessible buttons. Never pass memory text, topic content, backlink lists, or stale-link refs into the shelf API.

- [ ] **Step 3: Wire the hoisted chat and view navigation**

Pass the active project `ChatApi` into Knowledge and reuse the existing `ProjectWorkspaceView` setter. Keep local chat and non-project Knowledge paths impossible by type/branch.

- [ ] **Step 4: Run Knowledge and shell tests**

Run:

```bash
npx vitest run src/features/knowledge/KnowledgePanel.test.tsx src/App.test.tsx
npm run typecheck
```

Expected: all focused tests and typecheck pass.

### Task 7: Documentation and integration verification

**Files:**
- Modify: `docs/IPC_CONTRACT.md`
- Modify: `docs/PROMPT_CONTEXT.md`
- Modify: `docs/MEMORY.md`
- Modify: `docs/SESSIONS.md`
- Modify: `docs/SMOKE_TESTING.md`
- Modify: `docs/ROADMAP.md`
- Modify: `AGENTS.md`

**Interfaces:**
- Consumes: proven behavior from Tasks 1–6.
- Produces: honest shipped/candidate status and packaged smoke script.

- [ ] **Step 1: Update contracts and status from code truth**

Document exact wire shapes, limits, compatibility, persistence, fork/rewind behavior, all-or-nothing send, and manifest semantics. State explicitly that links remain metadata and drag/drop/browser/semantic retrieval are not shipped.

- [ ] **Step 2: Run all focused suites**

Run:

```bash
cd src-tauri
cargo fmt --all -- --check
cargo test prompts:: -- --nocapture
cargo test commands::chat -- --nocapture
cargo test sessions -- --nocapture
cd ..
npm run typecheck
npx vitest run src/features/chat src/features/sessions src/features/knowledge src/App.test.tsx
```

Expected: zero failures.

- [ ] **Step 3: Run the full verifier**

Run: `PLUME_FULL_VERIFY=1 ./scripts/verify.sh`

Expected: zero failures; only already-documented soft-cap warnings may remain.

- [ ] **Step 4: Run packaged UI smoke**

Build/launch the packaged app through the repository smoke workflow. In a disposable trusted project, add a line selection, one memory entry, and one topic; verify order, removal, relaunch restoration, accepted-turn provenance, then delete a backing source and verify blocked recovery/no send.

- [ ] **Step 5: Perform exact-head review and fix genuine findings**

Review the full diff plus surrounding code for scope leaks, stale async state, path/trust bypass, manifest mismatch, fork/rewind corruption, and missing tests. Apply one coherent fix round, rerun affected focused tests, full verifier, and the relevant smoke step.

- [ ] **Step 6: Commit, push, open PR, and confirm CI**

Run:

```bash
git add AGENTS.md docs src src-tauri
git commit -m "feat: add typed context shelf"
git push -u origin codex/typed-context-shelf
```

Open a ready PR with exact verification evidence. Confirm GitHub verify and gitleaks on the exact head before merge recommendation.
