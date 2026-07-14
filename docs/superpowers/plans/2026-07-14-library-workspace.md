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
- [ ] Move existing exact projection behavior without changing results; keep compatibility re-export only if needed during the same commit.
- [ ] Rename visible navigation and `ProjectWorkspaceView` from `knowledge` to `library`; old persisted UI preference values fall back safely.
- [ ] Re-run projection/App/drawer/sidebar tests.
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
- Modify: `src-tauri/src/sessions/validation.rs`
- Modify: `src/lib/api/memory.ts`
- Modify: `src/lib/api/chat.ts`

**Interfaces:**
- App-private store path is resolved from Tauri app data; no command accepts a root.
- Commands: `memory_user_index`, `memory_user_remember`, `memory_user_update`, `memory_user_forget`, `memory_user_search`.
- Reuse `MemoryEntry`, redaction, 100-entry/1-KiB-entry/64-KiB-store caps; user entries have no project-topic links.
- Add `ContextSourceRef::{ kind: 'userMemoryEntry', entryId }` and matching exact manifest/preview variants.
- Both local and project session shelves may contain user-memory refs; resolution always uses the app-private store and never enables ambient selection.

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

- [ ] Write failing store tests for CRUD/search/redaction/caps/symlink/hardlink/id validation/relaunch and physical separation from every project `.plume` store.
- [ ] Run `cd src-tauri && cargo test memory::user_store_tests -- --nocapture`; expected RED is the absent user store/module.
- [ ] Write failing command/wire tests for no-project availability, exact camelCase types, and no caller-controlled path.
- [ ] Write failing prompt/session tests proving explicit local/project use, deleted/stale rejection, no ambient injection, and project `memoryEntry` behavior unchanged.
- [ ] Implement the store by extracting shared validated entry mechanics only where it reduces duplication without weakening project path checks.
- [ ] Run `cd src-tauri && cargo test memory::user_store_tests -- --nocapture`, `cd src-tauri && cargo test commands::memory -- --nocapture`, and `cd src-tauri && cargo test prompts::explicit_context_tests -- --nocapture`.
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
- [ ] Key every request generation by exact scope/project identity and clear visible data synchronously on change.
- [ ] Load the app-private user store in both no-project and project shells; never substitute project entries into User memory.
- [ ] Keep errors source-local so one failed store does not blank healthy topics or memory.
- [ ] Run `npm run test -- src/features/library/useLibraryData.test.tsx`; confirm GREEN.
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

- [ ] Run the focused test before implementation; expected RED is missing `LibraryPanel`, tree sections, and `library` route.

- [ ] Add failing component tests for tree categories/counts, scope label, selection, search semantics, empty/loading/error states, and keyboard navigation.
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
- [ ] Render human summary/title first; paths, ids, hashes, byte counts, and redaction counts live under Details.
- [ ] Connections lists exact backlinks/links and states explicitly that connections organize information and do not choose chat context.
- [ ] Do not render a graph or imply semantic similarity.
- [ ] Commit: `feat: add library details and connections`.

### Task 6: Exact click/drag handoff

**Files:**
- Modify: `src/features/library/LibraryPanel.tsx`
- Modify: `src/features/library/LibraryDetail.tsx`
- Reuse: `src/features/chat/ContextDragAction.tsx`
- Test: `src/features/library/LibraryPanel.test.tsx`

- [ ] Add tests proving canonical topics and memory entries emit only their exact refs, duplicates emphasize, full/unavailable results are visible, core topic files have no action, and local/project boundaries hold.
- [ ] Reuse the current context shelf handoff and Plume-only drag MIME; never put content in drag data.
- [ ] Keep **Use in chat** as the keyboard/screen-reader path.
- [ ] Add user-memory click/drag tests proving the new `userMemoryEntry` kind works in both local and project chats without auto-selection.
- [ ] Commit: `feat: connect library to chat`.

### Task 7: Docs, packaged smoke, and publication

- [ ] Update `docs/FEATURE_INVENTORY.md`, `docs/UI_STYLE.md`, `docs/IPC_CONTRACT.md` if needed, README/docs spine wording, and smoke steps from Knowledge to Library.
- [ ] Packaged smoke: project A/B separation, no-project view, partial store failure, capped topics, search, backlink detail, click/drag, duplicate/full shelf, and no automatic context selection.
- [ ] Run complete verification and exact-head review before merge.
