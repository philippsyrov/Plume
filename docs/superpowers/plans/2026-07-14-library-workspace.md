# Library Workspace Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace Knowledge with an Obsidian-informed, Plume-restrained Library where people can understand user memory, project memory, topics, backlinks, and provenance without changing prompt authority.

**Architecture:** Reuse current project-memory/topic APIs and exact projection logic, and add a real app-private user-memory store with an explicit `userMemoryEntry` context kind. Split Knowledge into a scope-aware Library controller, source tree, searchable index, detail canvas, and optional Connections inspector. Project data stays project-only; user memory is local app data and enters prompts only through explicit shelf placement.

**Tech Stack:** TypeScript, React 19, existing memory IPC, Vitest, Testing Library, CSS grid, typed context shelf.

## Global Constraints

- Memory-topic links and backlinks remain organization metadata only.
- No semantic retrieval, graph view, automatic prompt selection, editing migration, or cross-project aggregation.
- User memory is never ambient prompt context in this campaign. Only an explicitly attached `userMemoryEntry` is resolved.
- Scope is always visible; project switches clear old data before loading new data.
- Click/drag emits opaque existing `ContextSourceRef` values only.
- Use Obsidian for information architecture, not a pixel/theme/plugin copy.

---

### Task 1: Rename and projection model

**Files:**
- Rename: `src/features/knowledge/` to `src/features/library/` in focused moves.
- Create: `src/features/library/projection.ts`
- Create: `src/features/library/projection.test.ts`
- Modify: `src/App.tsx`
- Modify: `src/features/project-shell/UnifiedSidebar.tsx`
- Modify: `src/features/project-shell/ToolDrawer.tsx`

**Interfaces:**
- `LibrarySection = 'overview' | 'user-memory' | 'project-memory' | 'topics' | 'connections'`.
- `LibrarySelection` is a tagged object carrying section plus exact entry/topic identity.
- Visible route value becomes `library`; legacy `knowledge` preference values normalize to `library` once.

- [ ] Add failing projection tests for Overview, User memory, Project memory, Topics, Connections, canonical backlinks, stale/unresolved links, search, and stable human titles.
- [ ] Run `npm run test -- src/features/library/projection.test.ts src/App.test.tsx src/features/project-shell/ToolDrawer.test.tsx src/features/project-shell/UnifiedSidebar.test.tsx`; expected RED is missing `library` modules/route.
- [ ] Move existing exact projection behavior without changing results; keep compatibility re-export only if needed during the same commit.
- [ ] Rename visible navigation and `ProjectWorkspaceView` from `knowledge` to `library`; old persisted UI preference values fall back safely.
- [ ] Re-run the same command; expected GREEN includes the five Library sections and no visible `Knowledge` navigation label.
- [ ] Commit: `refactor: rename knowledge to library`.

### Task 2: App-private user memory store and explicit context kind

**Files:**
- Create: `src-tauri/src/memory/user_store.rs`
- Create: `src-tauri/src/memory/user_store_tests.rs`
- Modify: `src-tauri/src/memory/mod.rs`
- Modify: `src-tauri/src/commands/memory.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/capabilities/default.json`
- Modify: `src-tauri/src/prompts/explicit_context.rs`
- Modify: `src-tauri/src/prompts/explicit_context_tests.rs`
- Modify: `src-tauri/src/prompts/assemble.rs`
- Modify: `src-tauri/src/prompts/assemble_tests.rs`
- Modify: `src-tauri/src/commands/chat/context.rs`
- Modify: `src-tauri/src/commands/chat/context_tests.rs`
- Modify: `src-tauri/src/commands/chat/send.rs`
- Modify: `src-tauri/src/commands/chat/send_tests.rs`
- Modify: `src-tauri/src/sessions/validation.rs`
- Modify: `src-tauri/src/sessions/context_tests.rs`
- Modify: `src/lib/api/memory.ts`
- Modify: `src/lib/api/chat.ts`
- Modify: `src/features/chat/useChat.ts`
- Modify: `src/features/chat/useChat.test.tsx`

**Interfaces:**
- App-private store path is resolved from Tauri app data; no command accepts a root.
- Commands: `memory_user_index`, `memory_user_remember`, `memory_user_update`, `memory_user_forget`, `memory_user_search`.
- Reuse `MemoryEntry`, redaction, 100-entry/1-KiB-entry/64-KiB-store caps; user entries have no project-topic links.
- Add `ContextSourceRef::{ kind: 'userMemoryEntry', entryId }` and matching exact manifest/preview variants.
- Both local and project session shelves may contain user-memory refs; resolution always uses the app-private store and never enables ambient selection.
- `ExplicitContextStores<'a> { project_root: Option<&'a Path>, user_memory_dir: &'a Path, local_browser_evidence_dir: Option<&'a Path> }` replaces the single-root resolver input; each source kind selects only its owning store.
- `assemble` and preview receive `ExplicitContextStores` from the chat command after Tauri resolves app data and the optional trusted project. No prompt module calls Tauri or accepts a caller path.
- `useChat` retains `userMemoryEntry` refs for local chat and sends them with the exact session owner; it still rejects project-only refs without project context.

```ts
export type UserMemorySourceRef = {
  kind: 'userMemoryEntry';
  entryId: string;
};
```

```rust
pub fn read_user_entry_for_prompt(
    app_data_dir: &Path,
    entry_id: &str,
) -> Result<MemoryEntry, MemoryStoreError>;
```

```rust
pub(crate) struct ExplicitContextStores<'a> {
    pub project_root: Option<&'a Path>,
    pub user_memory_dir: &'a Path,
    pub local_browser_evidence_dir: Option<&'a Path>,
}
```

- [ ] Write failing store tests for CRUD/search/redaction/caps/symlink/hardlink/id validation/relaunch and physical separation from every project `.plume` store.
- [ ] Run `cd src-tauri && cargo test memory::user_store_tests -- --nocapture`; expected RED is the absent user store/module.
- [ ] Write failing command/wire tests for no-project availability, exact camelCase types, and no caller-controlled path.
- [ ] Write failing prompt/session tests proving explicit local/project use, deleted/stale rejection, no ambient injection, and project `memoryEntry` behavior unchanged.
- [ ] In `sessions/context_tests.rs`, add exact save/load/relaunch tests for local `userMemoryEntry`, project `userMemoryEntry`, local rejection of project `memoryEntry`, malformed persisted user refs, and preservation through fork/rewind accepted-turn manifests with an empty child shelf.
- [ ] Add command tests asserting local `userMemoryEntry` preview/send succeeds without trust, project chat resolves the same user id plus project refs, local `memoryEntry` remains blocked, and omitted new fields preserve old wire compatibility.
- [ ] Add frontend tests proving `includeProjectContext=false` keeps `userMemoryEntry` and owned Browser refs but removes/rejects file/project-memory/topic refs.
- [ ] Run `cd src-tauri && cargo test sessions::context_tests -- --nocapture`, `cd src-tauri && cargo test commands::chat -- --nocapture`, and `npm run test -- src/features/chat/useChat.test.tsx`; expected RED is missing the new source variant/store threading and current local-source rejection.
- [ ] Implement the store by extracting shared validated entry mechanics only where it reduces duplication without weakening project path checks.
- [ ] Thread `ExplicitContextStores` through preview/send/assemble; do not infer app data from a project root.
- [ ] Run `cd src-tauri && cargo test sessions::context_tests -- --nocapture`, `cd src-tauri && cargo test memory::user_store_tests -- --nocapture`, `cd src-tauri && cargo test commands::memory -- --nocapture`, `cd src-tauri && cargo test prompts::explicit_context_tests -- --nocapture`, `cd src-tauri && cargo test commands::chat -- --nocapture`, and `npm run test -- src/features/chat/useChat.test.tsx`; expected GREEN covers save/load/relaunch plus the complete store-to-prompt path.
- [ ] Commit: `feat: add private user memory`.

### Task 3: Scope-safe Library data controller

**Files:**
- Create: `src/features/library/useLibraryData.ts`
- Create: `src/features/library/useLibraryData.test.tsx`
- Consume: `src/lib/api/memory.ts`

**Interfaces:**
- `useLibraryData({ projectIdentity }): LibraryData` loads user memory always and project memory/topics only for a trusted project.
- Each source returns independent `loading | ready | unavailable | error` state with its own retry.

```ts
export type LibraryData = {
  userMemory: LibrarySourceState<MemoryIndex>;
  projectMemory: LibrarySourceState<MemoryIndex>;
  topics: LibrarySourceState<MemoryTopics>;
  retryUserMemory(): void;
  retryProjectMemory(): void;
  retryTopics(): void;
};
```

- [ ] Write failing tests for project load, independent source failures/retries, project A to B switch, unmount, revision refresh, capped topics, and no-project state.
- [ ] Run `npm run test -- src/features/library/useLibraryData.test.tsx`; expected RED is the absent Library controller/user-memory source.
- [ ] Key every request generation by exact scope/project identity and clear visible data synchronously on change.
- [ ] Load the app-private user store in both no-project and project shells; never substitute project entries into User memory.
- [ ] Keep errors source-local so one failed store does not blank healthy topics or memory.
- [ ] Re-run `npm run test -- src/features/library/useLibraryData.test.tsx`; expected GREEN.
- [ ] Commit: `feat: load scoped library data`.

### Task 4: Source tree and searchable index

**Files:**
- Create: `src/features/library/LibraryPanel.tsx`
- Create: `src/features/library/LibraryPanel.test.tsx`
- Create: `src/features/library/LibraryTree.tsx`
- Create: `src/features/library/LibraryIndex.tsx`
- Create: `src/features/library/librarySelection.ts`
- Modify: `src/styles/layout/knowledge.css` into `src/styles/layout/library.css`
- Modify: `src/styles/layout.css`

**Interfaces:**
- `LibraryPanel({ projectIdentity, onUseInChat, onContextDragActiveChange })`.
- `LibraryTree` emits a `LibrarySection`; `LibraryIndex` emits a `LibrarySelection`.

- [ ] Add failing component tests for tree categories/counts, scope label, selection, search semantics, empty/loading/error states, and keyboard navigation.
- [ ] Run `npm run test -- src/features/library/LibraryPanel.test.tsx`; expected RED is missing `LibraryPanel`, tree sections, and `library` route.
- [ ] Implement compact tree + list with stable selection ids and responsive collapse at narrow widths.
- [ ] Search across the currently loaded selected scope and label the search boundary plainly.
- [ ] Avoid dashboard cards; use calm rows, readable hierarchy, and one selected state.
- [ ] Run `npm run test -- src/features/library/LibraryPanel.test.tsx`; confirm GREEN.
- [ ] Commit: `feat: build library navigation`.

### Task 5: Detail canvas and Connections inspector

**Files:**
- Create: `src/features/library/LibraryDetail.tsx`
- Create: `src/features/library/LibraryConnections.tsx`
- Create: `src/features/library/LibraryDetail.test.tsx`
- Adapt: `src/features/knowledge/KnowledgeMemoryCard.tsx`

**Interfaces:**
- `LibraryDetail({ selection, data })` renders one selected object.
- `LibraryConnections({ selection, projection })` renders exact stored links/backlinks only.

```ts
expect(screen.getByText(/organize information/i)).toBeVisible();
expect(screen.queryByText(/automatically added to chat/i)).not.toBeInTheDocument();
```

- [ ] Add failing tests for memory/topic reading, backlinks, stale/unresolved labels, timestamps/redactions, and Details provenance.
- [ ] Run `npm run test -- src/features/library/LibraryDetail.test.tsx`; expected RED is missing detail/connections components.
- [ ] Render human summary/title first; paths, ids, hashes, byte counts, and redaction counts live under Details.
- [ ] Connections lists exact backlinks/links and states explicitly that connections organize information and do not choose chat context.
- [ ] Do not render a graph or imply semantic similarity.
- [ ] Re-run `npm run test -- src/features/library/LibraryDetail.test.tsx`; expected GREEN includes metadata-only copy and exact backlinks.
- [ ] Commit: `feat: add library details and connections`.

### Task 6: Exact click/drag handoff

**Files:**
- Modify: `src/features/library/LibraryPanel.tsx`
- Modify: `src/features/library/LibraryDetail.tsx`
- Reuse: `src/features/chat/ContextDragAction.tsx`
- Test: `src/features/library/LibraryPanel.test.tsx`

- [ ] Add tests proving canonical topics and memory entries emit only their exact refs, duplicates emphasize, full/unavailable results are visible, core topic files have no action, and local/project boundaries hold.
- [ ] Run `npm run test -- src/features/library/LibraryPanel.test.tsx`; expected RED is missing user-memory handoff and renamed Library wiring.
- [ ] Reuse the current context shelf handoff and Plume-only drag MIME; never put content in drag data.
- [ ] Keep **Use in chat** as the keyboard/screen-reader path.
- [ ] Add user-memory click/drag tests proving the new `userMemoryEntry` kind works in both local and project chats without auto-selection.
- [ ] Re-run `npm run test -- src/features/library/LibraryPanel.test.tsx src/features/chat/contextDragPayload.test.ts`; expected GREEN.
- [ ] Commit: `feat: connect library to chat`.

### Task 7: Docs, packaged smoke, and publication

- [ ] Update `docs/FEATURE_INVENTORY.md`, `docs/UI_STYLE.md`, `docs/IPC_CONTRACT.md` if needed, README/docs spine wording, and smoke steps from Knowledge to Library.
- [ ] Packaged smoke: project A/B separation, no-project view, partial store failure, capped topics, search, backlink detail, click/drag, duplicate/full shelf, and no automatic context selection.
- [ ] Run `cd src-tauri && cargo test`, `npm run test -- src/features/library src/features/chat/useChat.test.tsx`, `npm run typecheck`, and `PLUME_FULL_VERIFY=1 ./scripts/verify.sh`; require zero failures.
- [ ] Publish, wait for GitHub verify/gitleaks, complete packaged smoke, and obtain findings-only exact-head review before merge.
