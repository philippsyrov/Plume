# Visible Context Drop Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let users drag an eligible memory, curated topic, or current inspector file/selection into project chat through a temporary, obvious drop tray that reuses the shipped typed context shelf.

**Architecture:** Pure helpers serialize and validate one opaque `ContextSourceRef` under a Plume-only MIME type. A reusable drag action writes that payload, while a reusable drop surface owns transient visibility, hover depth, parsing, and result announcements. `TrustedView` keeps session/navigation ownership and passes the existing `addContextSource` handoff into Knowledge and Files; the existing Context shelf remains the only stored destination and receives a one-shot visual emphasis key.

**Tech Stack:** React 19, TypeScript, HTML Drag and Drop API, Vitest, Testing Library, existing Plume CSS tokens.

## Global Constraints

- Frontend-only: add no Rust command, prompt path, source kind, dependency, or authority.
- Reuse `ContextSourceRef`, `AddContextSourceResult`, `addContextSource`, and exact backend preview/send manifests.
- Custom drag payloads contain opaque references only; never file bytes, memory text, or topic content.
- Existing **Use in chat** buttons remain the keyboard/screen-reader path.
- Do not expose the destination in local chat or while project chat is streaming.
- Memory-topic links remain organization metadata only.
- Motion is subtle and disabled by `prefers-reduced-motion: reduce`.

---

### Task 1: Opaque drag payload contract

**Files:**
- Create: `src/features/chat/contextDragPayload.ts`
- Create: `src/features/chat/contextDragPayload.test.ts`

**Interfaces:**
- Produces: `PLUME_CONTEXT_MIME`, `writeContextDrag(event, source)`, `readContextDrop(event)`.
- Consumes: `ContextSourceRef` from `src/lib/api/chat.ts`.

- [ ] **Step 1: Write failing round-trip and rejection tests**

Cover all three variants plus foreign MIME, malformed JSON, unknown kinds, memory ids outside `m_` + 32 hex, non-flat topics, absolute/parent/NUL/oversize project paths, half-ranges, non-integer/non-positive/reversed ranges.

```ts
expect(roundTrip({ kind: 'memoryEntry', entryId: `m_${'a'.repeat(32)}` }))
  .toEqual({ kind: 'memoryEntry', entryId: `m_${'a'.repeat(32)}` });
expect(parse({ kind: 'topicFile', name: '../topic.md' })).toBeNull();
expect(parse({ kind: 'projectFile', relPath: 'src/App.tsx', startLine: 8, endLine: 4 }))
  .toBeNull();
```

- [ ] **Step 2: Run the focused test and confirm RED**

Run: `npm run test -- src/features/chat/contextDragPayload.test.ts`

Expected: FAIL because `contextDragPayload.ts` does not exist.

- [ ] **Step 3: Implement the minimal payload module**

Use `application/x-plume-context-source+json`. `writeContextDrag` calls `dataTransfer.setData`, sets `effectAllowed = 'copy'`, and never writes `text/plain`. `readContextDrop` reads only the custom type, parses JSON inside `try/catch`, and returns a cloned validated tagged object.

```ts
export const PLUME_CONTEXT_MIME = 'application/x-plume-context-source+json';

export function writeContextDrag(
  dataTransfer: DataTransfer,
  source: ContextSourceRef,
): void {
  dataTransfer.setData(PLUME_CONTEXT_MIME, JSON.stringify(source));
  dataTransfer.effectAllowed = 'copy';
}

export function readContextDrop(dataTransfer: DataTransfer): ContextSourceRef | null {
  if (!Array.from(dataTransfer.types).includes(PLUME_CONTEXT_MIME)) return null;
  try {
    return validatedSource(JSON.parse(dataTransfer.getData(PLUME_CONTEXT_MIME)));
  } catch {
    return null;
  }
}
```

- [ ] **Step 4: Run the focused test and confirm GREEN**

Run: `npm run test -- src/features/chat/contextDragPayload.test.ts`

Expected: all payload tests pass.

### Task 2: Reusable drag action and temporary drop surface

**Files:**
- Create: `src/features/chat/ContextDragAction.tsx`
- Create: `src/features/chat/ContextDragAction.test.tsx`
- Create: `src/features/chat/ContextDropSurface.tsx`
- Create: `src/features/chat/ContextDropSurface.test.tsx`

**Interfaces:**
- `ContextDragAction({ source, onActivate, onDragActiveChange, children, className? })` renders one normal button that is also a dedicated drag surface.
- `ContextDropSurface({ onDropSource, disabled, children })` passes `{ onDragActiveChange }` to its render-prop child and owns the temporary tray/live result.

- [ ] **Step 1: Write failing action tests**

Assert click calls `onActivate(source)`, drag start writes only the custom MIME payload and calls `onDragActiveChange(true)`, drag end calls `false`, and the button has `title="Drag to project chat"` while retaining its normal accessible label.

- [ ] **Step 2: Run action tests and confirm RED**

Run: `npm run test -- src/features/chat/ContextDragAction.test.tsx`

Expected: FAIL because the component does not exist.

- [ ] **Step 3: Implement `ContextDragAction` minimally**

```tsx
<button
  type="button"
  draggable
  className={className}
  title="Drag to project chat"
  onClick={() => void onActivate(source)}
  onDragStart={(event) => {
    writeContextDrag(event.dataTransfer, source);
    onDragActiveChange(true);
  }}
  onDragEnd={() => onDragActiveChange(false)}
>
  {children}
</button>
```

- [ ] **Step 4: Write failing drop-surface tests**

Assert the tray is absent at rest, appears after `onDragActiveChange(true)`, changes to **Release to add to chat** on nested drag enter without flicker, calls `preventDefault` only for the Plume MIME, returns the parsed ref once on drop, clears after cancel/drop, announces full/unavailable copy, and stays absent when disabled.

- [ ] **Step 5: Run drop-surface tests and confirm RED**

Run: `npm run test -- src/features/chat/ContextDropSurface.test.tsx`

Expected: FAIL because the surface does not exist.

- [ ] **Step 6: Implement the drop surface**

Keep `dragActive`, `overDepth`, and `notice` in component state. Render children through:

```ts
type ContextDragControls = {
  onDragActiveChange: (active: boolean) => void;
};
```

The tray is a `div` with `aria-hidden="true"`; the result is a separate `role="status" aria-live="polite"`. On `full` and `unavailable`, use the exact approved copy. `added`/`duplicate` need no source-view notice because navigation reveals the shelf.

- [ ] **Step 7: Run both component suites and confirm GREEN**

Run: `npm run test -- src/features/chat/ContextDragAction.test.tsx src/features/chat/ContextDropSurface.test.tsx`

Expected: both suites pass.

### Task 3: Knowledge memory and topic drag sources

**Files:**
- Modify: `src/features/knowledge/KnowledgePanel.tsx`
- Modify: `src/features/knowledge/KnowledgeMemoryCard.tsx`
- Modify: `src/features/knowledge/KnowledgePanel.test.tsx`

**Interfaces:**
- Knowledge receives optional `onContextDragActiveChange(active)` alongside the existing `onUseInChat`.
- Memory and canonical-topic **Use in chat** buttons become `ContextDragAction` instances with the exact existing refs.

- [ ] **Step 1: Add failing Knowledge tests**

Assert the memory action writes `{kind:'memoryEntry', entryId}` and the topic action writes `{kind:'topicFile', name}` to the Plume MIME, both toggle the drag-active callback, and clicking still calls the existing handoff. Assert core topic files still have no action.

- [ ] **Step 2: Run the focused suite and confirm RED**

Run: `npm run test -- src/features/knowledge/KnowledgePanel.test.tsx`

Expected: FAIL because the actions are not draggable and no drag callback exists.

- [ ] **Step 3: Replace only the existing actions**

Thread the callback through `KnowledgeContent`, `MemoryContent`, `TopicFile`, and `KnowledgeMemoryCard`. Do not make article bodies draggable; text selection remains normal.

- [ ] **Step 4: Re-run and confirm GREEN**

Run: `npm run test -- src/features/knowledge/KnowledgePanel.test.tsx`

Expected: Knowledge tests pass and the old full-shelf click behavior remains green.

### Task 4: Files view source, cross-view handoff, and shelf emphasis

**Files:**
- Modify: `src/features/file-tree/FileBrowser.tsx`
- Modify: `src/features/file-tree/FileBrowser.test.tsx`
- Modify: `src/App.tsx`
- Modify: `src/App.test.tsx`
- Modify: `src/features/sessions/usePersistedChat.ts`
- Modify: `src/features/sessions/usePersistedChat.test.tsx`
- Modify: `src/features/chat/ChatPanel.tsx`
- Modify: `src/features/chat/ContextShelf.tsx`
- Modify: `src/features/chat/ContextShelf.test.tsx`
- Modify: `src/features/chat/ChatPanel.test.tsx`

**Interfaces:**
- `FileInspector` gains optional `onUseInChat(source)` and `onContextDragActiveChange(active)`.
- `usePersistedChat` exposes `surfaceIdentity(): { scope: SessionScope; sessionId: string | null }`, backed by its existing render-mirrored refs.
- `ChatPanel` gains `emphasizedContextKey?: string | null`.
- `ContextShelf` gains the same optional key and applies it only to the exact `contextSourceKey(source)` row.

- [ ] **Step 1: Add failing FileInspector tests**

For an eligible UTF-8 selection, assert **Use file in chat** emits a whole-file ref; with `currentLineRange`, assert **Use selection in chat** emits the snapshotted range. Assert binary/oversize/loading/error/empty selections expose no draggable action.

- [ ] **Step 2: Run the FileBrowser suite and confirm RED**

Run: `npm run test -- src/features/file-tree/FileBrowser.test.tsx`

Expected: FAIL because FileInspector has no context action.

- [ ] **Step 3: Implement the inspector action with existing eligibility rules**

Reuse `describeAttachCandidate(selection, currentLineRange, null)` rather than duplicating size/encoding logic. Build the exact project-file ref from the eligible candidate and render `ContextDragAction` beside `InspectorHeader`.

- [ ] **Step 4: Add failing surface-identity and shell handoff tests**

First pin that `surfaceIdentity()` reports the current ref-backed scope/id immediately after a completed scope/session transition. Then update the App Knowledge mock to capture props. Assert a dropped/activated source calls project scope open, reads the resulting identity, calls `chat.addContextSource` only when that identity is project-scoped, navigates only for `added|duplicate`, leaves the source view for neither `full` nor `unavailable`, and wraps both Knowledge and Files in `ContextDropSurface`.

- [ ] **Step 5: Add failing shelf-emphasis tests**

Assert only the matching item gets `plume-context-shelf-item-emphasized`, duplicate emphasis targets the existing row, and a different key does nothing. In ChatPanel, assert the prop reaches ContextShelf.

- [ ] **Step 6: Run the integration suites and confirm RED**

Run: `npm run test -- src/features/sessions/usePersistedChat.test.tsx src/App.test.tsx src/features/chat/ContextShelf.test.tsx src/features/chat/ChatPanel.test.tsx`

Expected: FAIL on missing drop-surface/handoff/emphasis wiring.

- [ ] **Step 7: Implement the TrustedView handoff**

Add `surfaceIdentity()` to `usePersistedChat` by reading `activeScopeRef.current` and `activeIdsRef.current[scope]`; this is read-only and adds no persistence behavior. Rename the existing Knowledge-only callback to `useContextInChat`, await `openScope('project')`, require `surfaceIdentity().scope === 'project'`, then call `addContextSource` synchronously before another transition can interleave. On `added|duplicate`, set `emphasizedContextKey`, navigate to project chat, and clear the key after 900 ms with effect cleanup.

Wrap only Files and Knowledge center views in `ContextDropSurface`, passing `disabled={persisted.chat.status === 'streaming'}`. The local-chat branch remains untouched.

- [ ] **Step 8: Implement exact shelf emphasis and run GREEN suites**

Run: `npm run test -- src/features/file-tree/FileBrowser.test.tsx src/features/sessions/usePersistedChat.test.tsx src/App.test.tsx src/features/chat/ContextShelf.test.tsx src/features/chat/ChatPanel.test.tsx`

Expected: all focused suites pass.

### Task 5: Styling, documentation, and verification

**Files:**
- Modify: `src/styles/layout/chat.css`
- Modify: `src/styles/layout/knowledge.css`
- Modify: `src/styles/layout/inspector.css`
- Create: `src/styles/layout/context-drop.css`
- Modify: `src/styles/layout.css`
- Modify: `docs/UI_STYLE.md`
- Modify: `docs/IPC_ROADMAP.md`
- Modify: `docs/SMOKE_TESTING.md`
- Modify: `docs/ROADMAP.md`
- Modify: `docs/FEATURE_INVENTORY.md`

**Interfaces:**
- CSS classes: `.plume-context-drop-surface`, `.plume-context-drop-tray`, `.plume-context-drop-tray-over`, `.plume-context-shelf-item-emphasized`.

- [ ] **Step 1: Add the restrained visual treatment**

Use existing paper/chrome/tint/line/radius/spacing tokens. The tray sits above the workspace bottom edge, has a dashed outline and generous hit area, and strengthens only on hover. Add no glow, shadow stack, confetti, or new color token.

```css
@media (prefers-reduced-motion: reduce) {
  .plume-context-drop-tray,
  .plume-context-shelf-item-emphasized {
    transition: none;
    animation: none;
  }
}
```

- [ ] **Step 2: Run typecheck and all focused frontend tests**

Run:

```bash
npm run typecheck
npm run test -- src/features/chat/contextDragPayload.test.ts src/features/chat/ContextDragAction.test.tsx src/features/chat/ContextDropSurface.test.tsx src/features/knowledge/KnowledgePanel.test.tsx src/features/file-tree/FileBrowser.test.tsx src/features/sessions/usePersistedChat.test.tsx src/App.test.tsx src/features/chat/ContextShelf.test.tsx src/features/chat/ChatPanel.test.tsx
```

Expected: typecheck and all focused suites pass.

- [ ] **Step 3: Update honest shipped-status docs**

Document the gesture as frontend placement over the existing typed shelf. Explicitly state that it adds no semantic retrieval, link authority, browser evidence type, external file import, or computer-use capability.

- [ ] **Step 4: Run full verification**

Run: `PLUME_FULL_VERIFY=1 ./scripts/verify.sh`

Expected: all gates pass with only the two existing documentation soft-cap warnings.

- [ ] **Step 5: Run packaged UI smoke**

Build/launch the packaged smoke app through the repository harness. Test memory, topic, duplicate, full/unavailable, inspector range, persistence after relaunch, visible result, and reduced-motion posture. Record exact observations in the PR description; do not claim OS-level external file drag.

- [ ] **Step 6: Commit the implementation**

```bash
git add src docs
git commit -m "feat: add visible context drop"
```

- [ ] **Step 7: Push and open a focused PR**

Push `codex/drag-drop-context`, open one PR covering the design plus implementation, wait for GitHub verify and gitleaks, then hand the exact head to the independent reviewer. Do not merge before exact-head review.
