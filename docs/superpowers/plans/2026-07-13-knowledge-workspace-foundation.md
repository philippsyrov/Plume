# Knowledge Workspace Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship D133, a trusted-project, read-only Knowledge workspace that browses curated topics, derives exact-ref backlinks, keeps unlinked and stale-linked memories visible, and provides honest lexical memory search.

**Architecture:** Reuse the shipped `memory.index` and `memory.topics` IPC reads. A pure frontend projection turns their bounded results into topic/backlink/unlinked/stale views; a dedicated hook owns independent load, retry, revision refresh, and stale-response suppression; a workspace panel renders the projection without adding mutation or prompt-selection authority. No new Rust command, graph dependency, or persistence schema is needed.

**Tech Stack:** React 19, strict TypeScript, Vitest, Testing Library, existing Tauri IPC wrappers, existing CSS tokens.

## Global Constraints

- Start from merged `origin/main@ca2954ff7b69a57a4ea680ffc9fcc8b0fbe37438` or later.
- `AGENTS.md` is authoritative; MLX-LM remains the happy path and Ollama remains compatibility.
- Memory links remain organization metadata only. Opening a linked topic must not alter prompt context, retrieval, chat state, or agent authority.
- The workspace is read-only. Remember, edit, forget, relink, and distill remain in Project Settings.
- Use only `memory.index` and `memory.topics`; add no IPC, Rust, database, dependency, network call, or background task.
- Exact canonical topic refs are the backlink key. Never fuzzy-match filenames or resolve a stale ref to another file.
- Core/topic content remains the backend-capped content already returned by `memory.topics`.
- Sort memory entries newest-first by `createdMs`, then by `id` for deterministic ties. Preserve core-file order and sort topic files by `name`.
- Project switch remains isolated by `ProjectView key={meta.root}`. Within one mount, request generations must prevent older reads from repainting newer Retry/revision results.
- One source failing must not hide the other. Each failed source owns its own visible error and Retry action.
- Lexical search is case-insensitive substring search over loaded memory text only. Label it `Search memories`; never call it semantic search or retrieval.
- Do not add `Use in chat`, drag/drop, context shelf, embeddings, dreaming, automatic topics, or mutation controls in D133.
- New user-facing copy must follow the existing calm monochrome workspace language.

**Approved design:** `docs/superpowers/specs/2026-07-12-roadmap-navigation-design.md` section `First Product Track After The Spine`.

---

### Task 1: Build The Pure Knowledge Projection With TDD

**Files:**

- Create: `src/features/knowledge/projection.ts`
- Create: `src/features/knowledge/projection.test.ts`

**Interfaces:**

- Consumes: `MemoryIndex`, `MemoryEntry`, `MemoryTopics`, and `MemoryTopicFile` from `src/lib/api/memory.ts`.
- Produces:

```typescript
export type KnowledgeMemory = {
  entry: MemoryEntry;
  staleLinks: string[];
};

export type KnowledgeTopic = {
  file: MemoryTopicFile;
  backlinks: KnowledgeMemory[];
};

export type KnowledgeProjection = {
  entries: KnowledgeMemory[];
  topics: KnowledgeTopic[];
  unlinked: KnowledgeMemory[];
  staleLinked: KnowledgeMemory[];
};

export function buildKnowledgeProjection(
  index: MemoryIndex,
  topicData: MemoryTopics,
): KnowledgeProjection;

export function filterKnowledgeMemories(
  memories: KnowledgeMemory[],
  query: string,
): KnowledgeMemory[];
```

- [ ] **Step 1: Write failing projection tests**

Create fixtures with two live topic refs, one missing ref, one unlinked entry,
and tied timestamps. Pin these behaviors:

```typescript
it('derives backlinks only from exact live refs and keeps stale refs visible', () => {
  const projection = buildKnowledgeProjection(
    memoryIndex([
      entry('m_b', 20, ['topics/alpha.md', 'topics/removed.md']),
      entry('m_a', 20, ['topics/beta.md']),
      entry('m_old', 10, []),
    ]),
    memoryTopics(['topics/alpha.md', 'topics/beta.md']),
  );

  expect(projection.entries.map(({ entry }) => entry.id)).toEqual(['m_a', 'm_b', 'm_old']);
  expect(projection.topics[0]?.backlinks.map(({ entry }) => entry.id)).toEqual(['m_b']);
  expect(projection.staleLinked[0]?.staleLinks).toEqual(['topics/removed.md']);
  expect(projection.unlinked.map(({ entry }) => entry.id)).toEqual(['m_old']);
});

it('uses exact refs rather than basename or fuzzy matches', () => {
  const projection = buildKnowledgeProjection(
    memoryIndex([entry('m_1', 1, ['alpha.md'])]),
    memoryTopics(['topics/alpha.md']),
  );
  expect(projection.topics[0]?.backlinks).toEqual([]);
  expect(projection.staleLinked[0]?.staleLinks).toEqual(['alpha.md']);
});

it('filters memory text with honest case-insensitive substring matching', () => {
  const projection = buildKnowledgeProjection(
    memoryIndex([entry('m_1', 2, [], 'Prefer Rust'), entry('m_2', 1, [], 'Use TypeScript')]),
    memoryTopics([]),
  );
  expect(filterKnowledgeMemories(projection.entries, ' RUST ').map(({ entry }) => entry.id))
    .toEqual(['m_1']);
});
```

Also pin: missing core files are excluded, existing core files precede sorted
`topics/*.md`, an empty/whitespace query returns all entries, link arrays are
not mutated, and a memory with both live and stale links appears in both its
live backlink list and `staleLinked`.

- [ ] **Step 2: Run the projection tests and capture red**

Run:

```bash
./scripts/dev-env.sh npx --no-install vitest run src/features/knowledge/projection.test.ts
```

Expected: FAIL because `projection.ts` does not exist.

- [ ] **Step 3: Implement the minimal pure projection**

Use this logic exactly:

```typescript
import type {
  MemoryEntry,
  MemoryIndex,
  MemoryTopicFile,
  MemoryTopics,
} from '../../lib/api/memory';

export type KnowledgeMemory = { entry: MemoryEntry; staleLinks: string[] };
export type KnowledgeTopic = { file: MemoryTopicFile; backlinks: KnowledgeMemory[] };
export type KnowledgeProjection = {
  entries: KnowledgeMemory[];
  topics: KnowledgeTopic[];
  unlinked: KnowledgeMemory[];
  staleLinked: KnowledgeMemory[];
};

function compareEntries(left: MemoryEntry, right: MemoryEntry): number {
  return right.createdMs - left.createdMs || left.id.localeCompare(right.id);
}

export function buildKnowledgeProjection(
  index: MemoryIndex,
  topicData: MemoryTopics,
): KnowledgeProjection {
  const files = [
    ...topicData.core.filter((file) => file.exists),
    ...topicData.topics.filter((file) => file.exists).sort((a, b) => a.name.localeCompare(b.name)),
  ];
  const liveRefs = new Set(files.map((file) => file.name));
  const entries = [...index.entries]
    .sort(compareEntries)
    .map((entry) => ({
      entry,
      staleLinks: entry.links.filter((link) => !liveRefs.has(link)),
    }));
  const topics = files.map((file) => ({
    file,
    backlinks: entries.filter(({ entry }) => entry.links.includes(file.name)),
  }));
  return {
    entries,
    topics,
    unlinked: entries.filter(({ entry }) => entry.links.length === 0),
    staleLinked: entries.filter(({ staleLinks }) => staleLinks.length > 0),
  };
}

export function filterKnowledgeMemories(
  memories: KnowledgeMemory[],
  query: string,
): KnowledgeMemory[] {
  const needle = query.trim().toLocaleLowerCase();
  if (needle === '') return memories;
  return memories.filter(({ entry }) => entry.text.toLocaleLowerCase().includes(needle));
}
```

Do not add graph traversal, scoring, stemming, tokenization, or backend search.

- [ ] **Step 4: Run focused tests and typecheck**

```bash
./scripts/dev-env.sh npx --no-install vitest run src/features/knowledge/projection.test.ts
./scripts/dev-env.sh npm run typecheck
git diff --check
```

Expected: projection tests and typecheck pass; diff check is silent.

- [ ] **Step 5: Commit the projection**

```bash
git add src/features/knowledge/projection.ts src/features/knowledge/projection.test.ts
git commit -m "feat: project memory topic backlinks"
```

---

### Task 2: Load Memory And Topics Independently With Stale Guards

**Files:**

- Create: `src/features/knowledge/useKnowledgeData.ts`
- Create: `src/features/knowledge/useKnowledgeData.test.tsx`

**Interfaces:**

- Consumes: `getMemoryIndex`, `getMemoryTopics`, `useMemoryRevision`, and IPC error helpers.
- Produces:

```typescript
export type KnowledgeSourceState<T> =
  | { kind: 'loading' }
  | { kind: 'ready'; data: T }
  | { kind: 'error'; message: string };

export type KnowledgeData = {
  memory: KnowledgeSourceState<MemoryIndex>;
  topics: KnowledgeSourceState<MemoryTopics>;
  retryMemory: () => void;
  retryTopics: () => void;
  refreshAll: () => void;
};

export function useKnowledgeData(): KnowledgeData;
```

- [ ] **Step 1: Write failing hook tests**

Mock `getMemoryIndex`, `getMemoryTopics`, and the memory API module. Use deferred
promises to pin request ordering.

```typescript
it('keeps topics usable when memory fails and retries only memory', async () => {
  mocks.getMemoryIndex.mockRejectedValueOnce(new Error('entries unreadable'));
  mocks.getMemoryTopics.mockResolvedValue(topicsFixture());
  const { result } = renderHook(() => useKnowledgeData());
  await waitFor(() => expect(result.current.memory.kind).toBe('error'));
  expect(result.current.topics.kind).toBe('ready');
  mocks.getMemoryIndex.mockResolvedValueOnce(indexFixture());
  act(() => result.current.retryMemory());
  await waitFor(() => expect(result.current.memory.kind).toBe('ready'));
  expect(mocks.getMemoryTopics).toHaveBeenCalledTimes(1);
});

it('ignores an older memory response after Retry resolves', async () => {
  const first = deferred<MemoryIndex>();
  const second = deferred<MemoryIndex>();
  mocks.getMemoryIndex.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
  const { result } = renderHook(() => useKnowledgeData());
  act(() => result.current.retryMemory());
  second.resolve(indexFixture('new'));
  await waitFor(() => expect(readyMemory(result.current).entries[0]?.id).toBe('new'));
  first.resolve(indexFixture('old'));
  await waitFor(() => expect(readyMemory(result.current).entries[0]?.id).toBe('new'));
});
```

Also test the mirror topic failure/retry, unmount suppression, `refreshAll`
calling both sources, `NeedsApproval` copy, and one `bumpMemoryRevision()`
causing both reads to refresh without a remount.

- [ ] **Step 2: Run the hook tests and capture red**

```bash
./scripts/dev-env.sh npx --no-install vitest run src/features/knowledge/useKnowledgeData.test.tsx
```

Expected: FAIL because the hook does not exist.

- [ ] **Step 3: Implement independent request generations**

Use one monotonically increasing ref per source plus a mounted ref. The core
loader shape is:

```typescript
const mounted = useRef(true);
const memoryRequest = useRef(0);
const topicRequest = useRef(0);

const loadMemory = useCallback(() => {
  const request = ++memoryRequest.current;
  setMemory({ kind: 'loading' });
  void getMemoryIndex().then(
    (data) => {
      if (mounted.current && request === memoryRequest.current) {
        setMemory({ kind: 'ready', data });
      }
    },
    (error: unknown) => {
      if (mounted.current && request === memoryRequest.current) {
        setMemory({ kind: 'error', message: knowledgeError(error, 'memory entries') });
      }
    },
  );
}, []);
```

Implement the same independently for topics. On mount and every
`useMemoryRevision()` change, call both loaders. Cleanup sets `mounted.current =
false` and increments both request refs. `retryMemory`, `retryTopics`, and
`refreshAll` call the stable loaders; they never share one combined Promise.

The defensive trust error copy is:

```typescript
function knowledgeError(error: unknown, source: string): string {
  if (isIpcError(error)) {
    return error.kind === 'NeedsApproval'
      ? `Trust the project to read ${source}.`
      : ipcErrorMessage(error);
  }
  return error instanceof Error ? error.message : String(error);
}
```

- [ ] **Step 4: Run focused tests and typecheck**

```bash
./scripts/dev-env.sh npx --no-install vitest run src/features/knowledge/useKnowledgeData.test.tsx
./scripts/dev-env.sh npm run typecheck
git diff --check
```

Expected: hook tests and typecheck pass; diff check is silent.

- [ ] **Step 5: Commit the source loader**

```bash
git add src/features/knowledge/useKnowledgeData.ts src/features/knowledge/useKnowledgeData.test.tsx
git commit -m "feat: load project knowledge independently"
```

---

### Task 3: Render The Read-Only Knowledge Workspace

**Files:**

- Create: `src/features/knowledge/KnowledgePanel.tsx`
- Create: `src/features/knowledge/KnowledgeMemoryCard.tsx`
- Create: `src/features/knowledge/KnowledgePanel.test.tsx`

**Interfaces:**

- Consumes: `useKnowledgeData`, `buildKnowledgeProjection`, and `filterKnowledgeMemories`.
- Produces: `KnowledgePanel`, a self-loading `<section aria-label="Project knowledge">`.

The selected view is local UI state only:

```typescript
type KnowledgeSelection =
  | { kind: 'all' }
  | { kind: 'unlinked' }
  | { kind: 'stale' }
  | { kind: 'topic'; ref: string };
```

- [ ] **Step 1: Write failing component tests**

Mock `useKnowledgeData` at the hook boundary. Cover:

1. Both sources ready: topic buttons, exact backlink count, topic content,
   unlinked count, stale-link count, memory id/date/redaction/link provenance.
2. Topic click renders only that topic's exact backlinks.
3. `Search memories` filters loaded memory text case-insensitively and labels
   the result as lexical search; clearing restores the selected view.
4. Topics error + memory ready: all/unlinked memories remain visible; the
   topic region shows its error and its own Retry.
5. Memory error + topics ready: topic navigation/content remain visible; the
   backlink region shows the memory error and its own Retry.
6. Empty sources: calm explicit empty states, never a blank panel.
7. Keyboard: focusing a topic button and pressing Enter selects it;
   `aria-current="page"` tracks the chosen view.
8. A selected topic removed by refresh resets to `All memories` rather than
   showing an impossible selection.

Example:

```typescript
it('keeps topics visible when memory entries fail', async () => {
  mocks.useKnowledgeData.mockReturnValue({
    memory: { kind: 'error', message: 'entries unreadable' },
    topics: { kind: 'ready', data: topicsFixture() },
    retryMemory: mocks.retryMemory,
    retryTopics: mocks.retryTopics,
    refreshAll: mocks.refreshAll,
  });
  render(<KnowledgePanel />);
  expect(screen.getByRole('button', { name: /topics\/alpha\.md/ })).toBeInTheDocument();
  await userEvent.click(screen.getByRole('button', { name: /topics\/alpha\.md/ }));
  expect(screen.getByText('Alpha topic body')).toBeInTheDocument();
  expect(screen.getByRole('alert')).toHaveTextContent('entries unreadable');
  await userEvent.click(screen.getByRole('button', { name: 'Retry memory entries' }));
  expect(mocks.retryMemory).toHaveBeenCalledOnce();
});
```

- [ ] **Step 2: Run panel tests and capture red**

```bash
./scripts/dev-env.sh npx --no-install vitest run src/features/knowledge/KnowledgePanel.test.tsx
```

Expected: FAIL because `KnowledgePanel.tsx` does not exist.

- [ ] **Step 3: Implement the panel as three focused regions**

The outer component owns selection/query and computes a projection only when
both sources are ready:

```typescript
export function KnowledgePanel() {
  const data = useKnowledgeData();
  const [selection, setSelection] = useState<KnowledgeSelection>({ kind: 'all' });
  const [query, setQuery] = useState('');
  const projection = useMemo(
    () => data.memory.kind === 'ready' && data.topics.kind === 'ready'
      ? buildKnowledgeProjection(data.memory.data, data.topics.data)
      : null,
    [data.memory, data.topics],
  );

  return (
    <section className="plume-knowledge" aria-label="Project knowledge">
      <KnowledgeHeader query={query} onQueryChange={setQuery} onRefresh={data.refreshAll} />
      <div className="plume-knowledge-grid">
        <KnowledgeNavigation
          topics={data.topics}
          projection={projection}
          selection={selection}
          onSelect={setSelection}
          onRetry={data.retryTopics}
        />
        <KnowledgeContent
          memory={data.memory}
          topics={data.topics}
          projection={projection}
          selection={selection}
          query={query}
          onRetryMemory={data.retryMemory}
        />
      </div>
    </section>
  );
}
```

Implement `KnowledgeHeader`, `KnowledgeNavigation`, and `KnowledgeContent` in
the same file; keep `KnowledgeMemoryCard` separate. Use native buttons and
`aria-current`, not custom keyboard handlers.

`KnowledgeMemoryCard` must render:

```tsx
<article className="plume-knowledge-memory" aria-label={`Memory ${entry.id}`}>
  <p>{entry.text}</p>
  <div className="plume-knowledge-memory-meta">
    <time dateTime={new Date(entry.createdMs).toISOString()}>{formattedDate}</time>
    <code>{entry.id}</code>
    {entry.redactionCount > 0 ? <span>{entry.redactionCount} redacted</span> : null}
  </div>
  <ul aria-label="Topic links">
    {entry.links.map((link) => (
      <li key={link} className={staleLinks.includes(link) ? 'is-stale' : undefined}>
        {link}{staleLinks.includes(link) ? ' · missing topic' : ''}
      </li>
    ))}
  </ul>
</article>
```

If topics are unavailable, pass an empty `staleLinks` array—unknown is not the
same as stale. Search results come only from `filterKnowledgeMemories`; render
the label `Lexical matches in loaded memory text`. Topic content uses `<pre>`
and preserves the backend `truncated` marker. No button in this task writes or
adds chat context.

- [ ] **Step 4: Run panel, projection, and hook tests**

```bash
./scripts/dev-env.sh npx --no-install vitest run src/features/knowledge
./scripts/dev-env.sh npm run typecheck
git diff --check
```

Expected: all Knowledge tests and typecheck pass.

- [ ] **Step 5: Commit the panel**

```bash
git add src/features/knowledge/KnowledgePanel.tsx src/features/knowledge/KnowledgeMemoryCard.tsx src/features/knowledge/KnowledgePanel.test.tsx
git commit -m "feat: browse project knowledge"
```

---

### Task 4: Integrate Knowledge Into The Trusted Workspace Shell

**Files:**

- Modify: `src/App.tsx`
- Modify: `src/App.test.tsx`
- Modify: `src/features/project-shell/UnifiedSidebar.tsx`
- Modify: `src/features/project-shell/UnifiedSidebar.test.tsx`
- Modify: `src/features/project-shell/UnifiedChrome.tsx`
- Modify: `src/features/project-shell/UnifiedChrome.test.tsx`
- Modify: `src/features/project-shell/ToolDrawer.tsx`
- Modify: `src/features/project-shell/ToolDrawer.test.tsx`
- Modify: `src/styles/layout.css`
- Modify: `src/styles/layout/project-shell.css`
- Create: `src/styles/layout/knowledge.css`
- Create: `src/features/knowledge/knowledgeStyle.test.ts`

**Interfaces:**

- Extends `ProjectWorkspaceView` with `'knowledge'`.
- Adds `onKnowledge: () => void` to `ToolDrawer`.
- Mounts `KnowledgePanel` only inside the trusted project shell.

- [ ] **Step 1: Write failing shell/navigation tests**

Update ToolDrawer test callbacks and assert `Knowledge` invokes only
`onKnowledge`. Add:

```typescript
expect(topbarSubtitle('knowledge', 'plume-demo')).toBe('Knowledge');
```

In `UnifiedSidebar.test.tsx`, prove a project session row is not highlighted
when `activeView="knowledge"`.

Mock `KnowledgePanel` in `App.test.tsx`, open a trusted project, open Workspace
views, choose Knowledge, and assert:

```typescript
expect(screen.getByTestId('knowledge-stub')).toBeInTheDocument();
expect(screen.getByText('Knowledge')).toBeInTheDocument();
expect(screen.queryByTestId('chat-stub')).not.toBeInTheDocument();
```

The style contract test must assert `layout.css` imports `knowledge.css`, the
workspace owns vertical overflow, the grid has a bounded navigation column,
memory text can wrap, and a constrained-width media query collapses to one
column.

- [ ] **Step 2: Run targeted shell tests and capture red**

```bash
./scripts/dev-env.sh npx --no-install vitest run src/App.test.tsx src/features/project-shell src/features/knowledge/knowledgeStyle.test.ts
```

Expected: FAIL because `knowledge` is not a workspace view and the stylesheet
does not exist.

- [ ] **Step 3: Wire the shared view type and shell**

In `UnifiedSidebar.tsx`:

```typescript
export type ProjectWorkspaceView =
  | 'local-chat'
  | 'project-chat'
  | 'files'
  | 'benchmarks'
  | 'knowledge';

const isChatView = activeView === 'local-chat' || activeView === 'project-chat';
```

In `ToolDrawer.tsx`, import `ProjectWorkspaceView`, remove the duplicate local
union, add a live `Knowledge` item after Files, and extend the icon union with
`knowledge`. In `UnifiedChrome.tsx`, return `Knowledge` before the chat
fallback.

In `App.tsx`:

```typescript
import { KnowledgePanel } from './features/knowledge/KnowledgePanel';

const openKnowledge = () => {
  setActiveView('knowledge');
  setToolDrawerOpen(false);
};

// In the central view switch, before local/project chat:
activeView === 'knowledge' ? (
  <div className="plume-project-knowledge-view">
    <KnowledgePanel />
  </div>
) : /* existing chat branches */
```

Pass `onKnowledge={openKnowledge}` to the drawer. Do not add Knowledge to the
no-project shell or Project Settings.

- [ ] **Step 4: Add the focused workspace styles**

Import `./layout/knowledge.css` immediately after `benchmarks.css`. Use existing
tokens and this layout contract:

```css
.plume-project-knowledge-view {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: var(--space-4);
}

.plume-knowledge {
  width: min(1120px, 100%);
  margin: 0 auto;
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}

.plume-knowledge-grid {
  display: grid;
  grid-template-columns: minmax(190px, 260px) minmax(0, 1fr);
  gap: var(--space-3);
}

.plume-knowledge-memory p,
.plume-knowledge-topic-content {
  overflow-wrap: anywhere;
}

@media (max-width: 760px) {
  .plume-knowledge-grid { grid-template-columns: minmax(0, 1fr); }
}
```

Finish the visual rules with existing chrome fill/line/radius/shadow tokens;
avoid fixed heights, gradients, new colors, and page-level scroll.

Add a simple book/network glyph in `project-shell.css` using the same 20px
pseudo-element convention as Files and Benchmarks.

- [ ] **Step 5: Run integration and full frontend tests**

```bash
./scripts/dev-env.sh npx --no-install vitest run src/App.test.tsx src/features/project-shell src/features/knowledge
./scripts/dev-env.sh npm run typecheck
git diff --check
```

Expected: all targeted tests and typecheck pass.

- [ ] **Step 6: Commit shell integration**

```bash
git add src/App.tsx src/App.test.tsx src/features/project-shell src/features/knowledge/knowledgeStyle.test.ts src/styles/layout.css src/styles/layout/project-shell.css src/styles/layout/knowledge.css
git commit -m "feat: add Knowledge workspace view"
```

---

### Task 5: Record D133 Capability Truth And Smoke Steps

**Files:**

- Modify: `AGENTS.md`
- Modify: `README.md`
- Modify: `docs/FEATURE_INVENTORY.md`
- Modify: `docs/ROADMAP.md`
- Modify: `docs/SMOKE_TESTING.md`

**Interfaces:**

- Changes `knowledge.workspace` from `researched` to `shipped` only after Tasks
  1–4 are committed and reachable.
- Records the exact Task 4 implementation commit as `lastVerifiedCommit`, so
  the roadmap checker produces no new freshness warning.

- [ ] **Step 1: Capture the implementation head**

```bash
git rev-parse HEAD
```

Expected: one 40-character SHA for the Task 4 commit. Use that exact output in
the inventory; do not use `HEAD`, a short SHA, the plan base, or a guessed
future squash SHA.

- [ ] **Step 2: Update feature truth**

In both the human table and `inventory-json`, set `knowledge.workspace` to
`shipped`. The JSON record must say:

```json
{
  "id": "knowledge.workspace",
  "track": "project-knowledge",
  "status": "shipped",
  "currentBehavior": "Trusted projects expose a read-only Knowledge workspace with capped topic navigation, exact-ref memory backlinks, unlinked and stale-linked views, provenance, and lexical memory-text search.",
  "missingBehavior": "The workspace cannot yet place sources into chat, persist a context shelf, perform semantic retrieval, generate topics, or mutate memory.",
  "frontendReachability": "Knowledge in the trusted Workspace views drawer.",
  "backendReachability": "Existing memory.index and memory.topics reads only; no new authority or IPC.",
  "automatedEvidence": [
    "src/features/knowledge/projection.test.ts",
    "src/features/knowledge/useKnowledgeData.test.tsx",
    "src/features/knowledge/KnowledgePanel.test.tsx",
    "src/App.test.tsx"
  ],
  "manualOrHardwareEvidence": "Packaged-app Knowledge smoke is required for the UI slice; no model or special hardware is required.",
  "dependencies": ["trusted project", "bounded memory.index and memory.topics reads"],
  "implementationPaths": [
    "src/features/knowledge/projection.ts",
    "src/features/knowledge/useKnowledgeData.ts",
    "src/features/knowledge/KnowledgePanel.tsx",
    "src/App.tsx"
  ],
  "sourceDocuments": [
    "docs/ROADMAP.md",
    "docs/superpowers/specs/2026-07-12-roadmap-navigation-design.md"
  ],
  "nextCommissionedSlice": "Typed explicit context shelf with manual Use in chat",
  "lastVerifiedCommit": "the exact 40-character Task 4 SHA captured in Step 1",
  "lastVerifiedDate": "2026-07-13"
}
```

The quoted SHA instruction is procedural, not literal file content: replace it
with the exact command output before running any checker.

- [ ] **Step 3: Update roadmap and project entry points**

- `docs/ROADMAP.md`: move Knowledge into the current floor and make the typed
  explicit context shelf the next deliverable. Keep semantic retrieval and
  dreaming as non-goals.
- `README.md`: add the read-only Knowledge workspace to the current shipped
  capability paragraph without claiming context placement.
- `AGENTS.md`: append one D133 status paragraph naming exact-ref backlinks,
  independent source failures/retries, stale-response suppression, lexical
  search, and the organization-metadata-only boundary. Do not rewrite old
  slice history.

- [ ] **Step 4: Add packaged smoke steps 56–61**

Append to the visual checklist and report format:

1. Open Workspace views → Knowledge; top bar and main region change, drawer
   closes.
2. Existing topic selection shows capped Markdown and only exact-ref backlinks.
3. All memories, Unlinked, and Stale links show correct counts/provenance; a
   stale ref is labelled missing and never opens another topic.
4. Search memories performs case-insensitive lexical text filtering and clearing
   restores the chosen view.
5. Exercise independent failure/retry using a temporary refused topic or entry
   path in a disposable fixture: the healthy source remains visible.
6. Switch projects while a read is in flight; no previous-project topic or
   memory repaints the new project. Confirm Settings still owns all mutations.

- [ ] **Step 5: Run documentation gates and focused tests**

```bash
./scripts/dev-env.sh npm run verify:docs
./scripts/dev-env.sh npx --no-install vitest run src/features/knowledge src/App.test.tsx src/features/project-shell
./scripts/dev-env.sh npm run typecheck
git diff --check
```

Expected: zero documentation errors/warnings, all tests pass, typecheck passes.

- [ ] **Step 6: Commit capability truth**

```bash
git add AGENTS.md README.md docs/FEATURE_INVENTORY.md docs/ROADMAP.md docs/SMOKE_TESTING.md
git commit -m "docs: record Knowledge workspace foundation"
```

---

### Task 6: Verify D133 As One Exact Head

**Files:**

- Modify only if verification finds a genuine defect.

**Interfaces:**

- Consumes: complete D133 branch.
- Produces: exact-head evidence, packaged-app smoke, independent review, and a
  focused PR.

- [ ] **Step 1: Run focused and full verification**

```bash
./scripts/dev-env.sh npm run verify:docs
./scripts/dev-env.sh npx --no-install vitest run src/features/knowledge src/App.test.tsx src/features/project-shell
PLUME_FULL_VERIFY=1 ./scripts/verify.sh
git diff --check origin/main...HEAD
git status --short
```

Expected: focused tests pass; full verifier has zero failures and only the two
existing documentation soft-cap warnings; diff check and status are clean.

- [ ] **Step 2: Build and launch the isolated packaged app smoke**

```bash
./scripts/smoke-app.sh
```

Use Codex Computer Use—not Claude—to execute Knowledge steps 56–61 in the
isolated smoke identity. Do not modify real Plume app data or privacy settings.
Record screenshots/observations for: drawer entry, topic/backlink view,
unlinked/stale provenance, lexical search, constrained-window layout, and
project switch isolation.

- [ ] **Step 3: Request exact-head review**

Review `origin/main..HEAD` against this plan and the approved design. Fix every
Critical/Important finding with regression evidence, rerun focused/full
verification, and repeat packaged smoke if UI behavior changed.

- [ ] **Step 4: Push and open a focused PR**

```bash
git push -u origin codex/knowledge-workspace-foundation
```

Open a non-draft PR only after exact-head review and local verification. Report
the exact SHA, focused test count, full verifier summary, packaged smoke result,
GitHub verify, and gitleaks. Do not merge while either check is pending.

---

## Plan Self-Review

- **Spec coverage:** Topic navigation, exact backlinks, unlinked memories,
  stale refs, provenance, lexical search, independent failure/retry, project
  isolation, stale async suppression, keyboard access, and packaged smoke each
  have a named task and test.
- **Boundary check:** No context shelf, `Use in chat`, drag/drop, semantic
  retrieval, mutation, background work, Rust, or new IPC appears in the plan.
- **Type consistency:** `KnowledgeMemory`, `KnowledgeTopic`,
  `KnowledgeProjection`, `KnowledgeSourceState`, `KnowledgeData`, and
  `ProjectWorkspaceView` names are consistent across producer/consumer tasks.
- **Placeholder check:** There are no TBD/TODO implementation gaps. The only
  run-time value is the exact Task 4 SHA, captured by command and immediately
  inserted into inventory truth.
