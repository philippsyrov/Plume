# Library Workspace Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace Knowledge with an Obsidian-informed, Plume-restrained Library where people can understand user memory, project memory, topics, backlinks, and provenance without changing prompt authority.

**Architecture:** Reuse current memory/topic APIs and exact projection logic. Split the existing Knowledge panel into a scope-aware Library controller, source tree, searchable index, detail canvas, and optional Connections inspector. Project data stays project-only; user-memory capability is surfaced only if repo truth provides a local store, otherwise the view explains that it is planned rather than faking data.

**Tech Stack:** TypeScript, React 19, existing memory IPC, Vitest, Testing Library, CSS grid, typed context shelf.

## Global Constraints

- Memory-topic links and backlinks remain organization metadata only.
- No semantic retrieval, graph view, automatic prompt selection, editing migration, or cross-project aggregation.
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

- [ ] Add failing projection tests for Overview, User memory, Project memory, Topics, Connections, canonical backlinks, stale/unresolved links, search, and stable human titles.
- [ ] Move existing exact projection behavior without changing results; keep compatibility re-export only if needed during the same commit.
- [ ] Rename visible navigation and `ProjectWorkspaceView` from `knowledge` to `library`; old persisted UI preference values fall back safely.
- [ ] Re-run projection/App/drawer/sidebar tests.

### Task 2: Scope-safe Library data controller

**Files:**
- Create: `src/features/library/useLibraryData.ts`
- Create: `src/features/library/useLibraryData.test.tsx`
- Consume: `src/lib/api/memory.ts`
- Modify backend/API only if a current local user-memory store already exists and needs a read-only list verb.

- [ ] Write failing tests for project load, independent source failures/retries, project A to B switch, unmount, revision refresh, capped topics, and no-project state.
- [ ] Key every request generation by exact scope/project identity and clear visible data synchronously on change.
- [ ] Keep User memory honest: load verified local entries if supported; otherwise return a typed unavailable state with planned copy.
- [ ] Keep errors source-local so one failed store does not blank healthy topics or memory.

### Task 3: Source tree and searchable index

**Files:**
- Create: `src/features/library/LibraryPanel.tsx`
- Create: `src/features/library/LibraryPanel.test.tsx`
- Create: `src/features/library/LibraryTree.tsx`
- Create: `src/features/library/LibraryIndex.tsx`
- Create: `src/features/library/librarySelection.ts`
- Modify: `src/styles/layout/knowledge.css` into `src/styles/layout/library.css`
- Modify: `src/styles/layout.css`

- [ ] Add failing component tests for tree categories/counts, scope label, selection, search semantics, empty/loading/error states, and keyboard navigation.
- [ ] Implement compact tree + list with stable selection ids and responsive collapse at narrow widths.
- [ ] Search across the currently loaded selected scope and label the search boundary plainly.
- [ ] Avoid dashboard cards; use calm rows, readable hierarchy, and one selected state.

### Task 4: Detail canvas and Connections inspector

**Files:**
- Create: `src/features/library/LibraryDetail.tsx`
- Create: `src/features/library/LibraryConnections.tsx`
- Create: `src/features/library/LibraryDetail.test.tsx`
- Adapt: `src/features/knowledge/KnowledgeMemoryCard.tsx`

- [ ] Add failing tests for memory/topic reading, backlinks, stale/unresolved labels, timestamps/redactions, and Details provenance.
- [ ] Render human summary/title first; paths, ids, hashes, byte counts, and redaction counts live under Details.
- [ ] Connections lists exact backlinks/links and states explicitly that connections organize information and do not choose chat context.
- [ ] Do not render a graph or imply semantic similarity.

### Task 5: Exact click/drag handoff

**Files:**
- Modify: `src/features/library/LibraryPanel.tsx`
- Modify: `src/features/library/LibraryDetail.tsx`
- Reuse: `src/features/chat/ContextDragAction.tsx`
- Test: `src/features/library/LibraryPanel.test.tsx`

- [ ] Add tests proving canonical topics and memory entries emit only their exact refs, duplicates emphasize, full/unavailable results are visible, core topic files have no action, and local/project boundaries hold.
- [ ] Reuse the current context shelf handoff and Plume-only drag MIME; never put content in drag data.
- [ ] Keep **Use in chat** as the keyboard/screen-reader path.

### Task 6: Docs, packaged smoke, and publication

- [ ] Update `docs/FEATURE_INVENTORY.md`, `docs/UI_STYLE.md`, `docs/IPC_CONTRACT.md` if needed, README/docs spine wording, and smoke steps from Knowledge to Library.
- [ ] Packaged smoke: project A/B separation, no-project view, partial store failure, capped topics, search, backlink detail, click/drag, duplicate/full shelf, and no automatic context selection.
- [ ] Run complete verification and exact-head review before merge.
