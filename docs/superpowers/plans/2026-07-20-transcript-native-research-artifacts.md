# Transcript-Native Research Artifacts Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace Plume's visible research mode and artifact card with ordinary chat turns, clickable source links, and Markdown attachments created only by an explicit chat request.

**Architecture:** Add two typed persisted transcript entries that carry only immutable artifact identity and safe display metadata. `ChatPanel` routes a tiny deterministic set of research/export phrases, appends research references at completion, and renders those references through an artifact-loading transcript component. Window-level source handoff activates the owning human-controlled Browser and navigates through its existing policy path.

**Tech Stack:** React 19, TypeScript, Vitest/Testing Library, Tauri 2, Rust 2021, SQLite schema v6.

## Global Constraints

- No Create selector, research card, preview/source/details/export controls, or automatic export.
- Stage A uses only 1–10 exact attached Browser-text sources and Apple On-Device or fixed Qwen.
- The frontend sends opaque owner/artifact/version references; Rust re-resolves artifacts and owns native save-panel export.
- Only immutable Rust-owned HTTP(S) source URLs become interactive.
- Source activation uses the owning human-controlled Browser; it grants no agent Browser authority.
- The exported attachment stores no filesystem path and re-exports the exact artifact version when clicked.
- Start every behavior change with a failing test and preserve full existing verification.

---

### Task 1: Narrow Natural-Language Intents

**Files:**
- Create: `src/features/research/researchIntent.ts`
- Create: `src/features/research/researchIntent.test.ts`

**Interfaces:**
- Produces: `researchQuestion(input: string): string | null`
- Produces: `isMarkdownExportRequest(input: string): boolean`

- [ ] **Step 1: Write failing intent tests**

```ts
expect(researchQuestion('Quickly research feathered dinosaurs.'))
  .toBe('feathered dinosaurs.');
expect(researchQuestion('Tell me about dinosaurs')).toBeNull();
expect(isMarkdownExportRequest('Please export this as Markdown.')).toBe(true);
expect(isMarkdownExportRequest('Export this as PDF')).toBe(false);
```

- [ ] **Step 2: Run the focused test and verify RED**

Run: `npm run test -- src/features/research/researchIntent.test.ts`

Expected: FAIL because `researchIntent.ts` does not exist.

- [ ] **Step 3: Implement exact normalization and matching**

```ts
function normalized(input: string): string {
  return input.trim().replace(/[.]$/, '').replace(/\s+/g, ' ').toLowerCase();
}

export function researchQuestion(input: string): string | null {
  const trimmed = input.trim();
  const match = /^(?:please\s+|quickly\s+)?research\s+(.+)$/i.exec(trimmed);
  return match?.[1]?.trim() || null;
}

export function isMarkdownExportRequest(input: string): boolean {
  const value = normalized(input).replace(/^please\s+/, '');
  return value === 'export this as markdown' ||
    value === 'save this research as a markdown file';
}
```

- [ ] **Step 4: Run focused tests and verify GREEN**

Run: `npm run test -- src/features/research/researchIntent.test.ts`

Expected: all intent tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/features/research/researchIntent.ts src/features/research/researchIntent.test.ts
git commit -m "feat: recognize narrow research chat intents"
```

### Task 2: Persist Typed Artifact and Export References

**Files:**
- Modify: `src/features/chat/useChat.ts`
- Modify: `src/features/sessions/transcript.ts`
- Modify: `src/features/sessions/transcript.test.ts`
- Modify: `src/lib/api/sessions.ts`
- Modify: `src-tauri/src/sessions/mod.rs`
- Modify: `src-tauri/src/sessions/validation.rs`
- Modify: `src-tauri/src/sessions/schema.rs`
- Modify: `src-tauri/src/sessions/tests.rs`
- Modify: `src-tauri/src/sessions/browser_workspace_tests.rs`

**Interfaces:**
- Produces `ResearchArtifactRef = { owner, artifactId, version }`.
- Adds `ChatEntry` variants `researchArtifact` and `researchExport`.
- Adds `ChatApi.appendEntries(entries: ChatEntry[]): void`.
- Persists the reference fields in SQLite schema v6 without source bodies or paths.

- [ ] **Step 1: Write failing frontend round-trip tests**

```ts
const entry: ChatEntry = {
  kind: 'researchArtifact',
  owner: { scope: 'local', sessionId: 's_1' },
  artifactId: 'ra_1',
  version: 2,
};
expect(wireToEntries(entriesToWire([entry]))).toEqual([entry]);
```

Add the matching `researchExport` case with `fileName: 'dinosaurs.md'` and no
path field.

- [ ] **Step 2: Run frontend persistence tests and verify RED**

Run: `npm run test -- src/features/sessions/transcript.test.ts`

Expected: FAIL because the variants are not representable.

- [ ] **Step 3: Write failing Rust parse/round-trip and migration tests**

```rust
let entry = json!({
  "kind": "researchArtifact",
  "owner": { "scope": "local", "sessionId": session.id },
  "artifactId": "ra_1",
  "version": 2
});
let parsed = parse_entries(&[entry]).expect("typed research ref");
save_transcript(&dir, &session.id, &parsed, false).unwrap();
assert_eq!(load(&dir, &session.id).unwrap().entries, parsed);
```

Add rejection tests for owner/session mismatch, invalid artifact ids, version 0,
unsafe filenames, and any unknown/path field. Add a v5 database migration test
that reaches schema version 6 and preserves existing messages.

- [ ] **Step 4: Run focused Rust tests and verify RED**

Run: `./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test sessions::'`

Expected: FAIL because schema v6 and transcript variants do not exist.

- [ ] **Step 5: Implement typed frontend variants and append API**

```ts
export type ResearchArtifactRef = {
  owner: SessionIdentity;
  artifactId: string;
  version: number;
};

// ChatEntry additions
| ({ kind: 'researchArtifact' } & ResearchArtifactRef)
| ({ kind: 'researchExport'; fileName: string } & ResearchArtifactRef)

// ChatApi addition
appendEntries: (next: ChatEntry[]) => void;
```

`appendEntries` must refuse while streaming and use one `setEntries(current =>
[...current, ...next])` update so persistence sees one stable boundary.

- [ ] **Step 6: Implement Rust variants, validation, row mapping, and schema v6**

Add nullable `artifact_scope`, `artifact_session_id`, `artifact_id`,
`artifact_version`, and `artifact_file_name` columns. Research rows keep
`content` as an empty string for FTS compatibility. Validate that the embedded
owner equals the session being saved, ids use existing opaque-id shape rules,
version is `>= 1`, and file names are a bounded basename ending in `.md`.

- [ ] **Step 7: Run frontend and Rust tests and verify GREEN**

Run:

```bash
npm run test -- src/features/sessions/transcript.test.ts src/features/chat/useChat.test.tsx
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test sessions::'
```

Expected: all focused persistence tests pass.

- [ ] **Step 8: Commit**

```bash
git add src/features/chat/useChat.ts src/features/sessions src/lib/api/sessions.ts src-tauri/src/sessions
git commit -m "feat: persist research transcript references"
```

### Task 3: Render Research as Ordinary Chat

**Files:**
- Create: `src/features/research/ResearchTranscriptEntry.tsx`
- Create: `src/features/research/ResearchTranscriptEntry.test.tsx`
- Modify: `src/features/research/SafeMarkdownPreview.tsx`
- Modify: `src/features/research/SafeMarkdownPreview.test.tsx`
- Modify: `src/features/chat/ChatEntryRow.tsx`
- Modify: `src/styles/layout/chat.css`
- Delete: `src/features/research/ResearchArtifactCard.tsx`
- Delete: `src/features/research/ResearchArtifactCard.test.tsx`

**Interfaces:**
- Consumes artifact owner/id/version and loads through `research.loadArtifact`.
- Produces `onOpenSource(url: string)` and `onReExport(ref)` user actions.

- [ ] **Step 1: Write failing transcript rendering tests**

```tsx
render(<ResearchTranscriptEntry entry={entry} onOpenSource={open} onReExport={reExport} />);
expect(await screen.findByText('A claim.')).toBeVisible();
expect(screen.queryByText('Open note')).not.toBeInTheDocument();
await user.click(screen.getByRole('button', { name: 'Example' }));
expect(open).toHaveBeenCalledWith('https://example.com');
```

Prove invalid/non-HTTP(S) source URLs render as text, a review-needed artifact
shows one short warning, and a Markdown attachment renders one clean filename
link without a path.

- [ ] **Step 2: Run focused component tests and verify RED**

Run: `npm run test -- src/features/research/ResearchTranscriptEntry.test.tsx`

Expected: FAIL because the component does not exist.

- [ ] **Step 3: Implement transcript-native artifact loading and rendering**

Use `loadResearchArtifact({ owner, artifactId, version })`, fence stale async
responses by the exact serialized ref, render `SafeMarkdownPreview` inside the
ordinary assistant entry shell, and render a source footer from
`loaded.sources`. Do not render Details or export controls.

- [ ] **Step 4: Implement export attachment rendering**

Render an icon-library document icon and filename button only for a
`researchExport` entry. Clicking calls `onReExport` with the exact ref. Keep
cancel quiet and show failures as one inline `role="alert"` message.

- [ ] **Step 5: Run focused tests and verify GREEN**

Run:

```bash
npm run test -- src/features/research/ResearchTranscriptEntry.test.tsx src/features/research/SafeMarkdownPreview.test.tsx
```

Expected: all transcript artifact tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/features/research src/features/chat/ChatEntryRow.tsx src/styles/layout/chat.css
git commit -m "feat: render research inside chat"
```

### Task 4: Route Research and Explicit Export Through the Composer

**Files:**
- Modify: `src/features/chat/ChatPanel.tsx`
- Modify: `src/features/chat/ChatPanel.test.tsx`
- Delete: `src/features/research/CreateMenu.tsx`
- Delete: `src/features/research/CreateMenu.test.tsx`

**Interfaces:**
- Consumes `researchQuestion`, `isMarkdownExportRequest`, `ChatApi.appendEntries`, and `useResearchRun`.
- Produces callbacks `onOpenResearchSource` and normal persisted transcript boundaries.

- [ ] **Step 1: Write failing chat-only flow tests**

Prove the Create selector and research summary are absent. Submit `Quickly
research feathered dinosaurs.` with one eligible Browser-text source and assert
`research.start` receives `question: 'feathered dinosaurs.'`. Assert artifact
completion appends exactly one `researchArtifact` ref. Assert a normal message
does not start research.

- [ ] **Step 2: Write failing export tests**

Submit `Please export this as Markdown.` after a research ref. Assert the user
message is appended, `exportResearchArtifact` receives the latest exact ref,
and a saved outcome appends one `researchExport` entry. Assert completion alone
never exports, cancellation adds no attachment, failure adds an error row, and
ambiguous text follows normal chat.

- [ ] **Step 3: Run ChatPanel tests and verify RED**

Run: `npm run test -- src/features/chat/ChatPanel.test.tsx`

Expected: failures name the still-visible Create menu/card and missing routing.

- [ ] **Step 4: Remove mode state and implement submit routing**

In order: reject while busy; detect exact export request; detect exact research
request; otherwise call ordinary `send`. Research requires the current model,
saved owner, supported provider, live MLX handle when applicable, and 1–10
eligible Browser text refs. Missing evidence appends one plain error/help row
without calling a provider.

- [ ] **Step 5: Append artifact ref once per exact identity**

Watch `research.artifact`; before appending, check `entries` for the same owner,
artifact id, and version so remount restoration cannot duplicate it.

- [ ] **Step 6: Run ChatPanel tests and verify GREEN**

Run: `npm run test -- src/features/chat/ChatPanel.test.tsx`

Expected: all chat-only flow tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/features/chat src/features/research
git commit -m "feat: route research through normal chat"
```

### Task 5: Open Sources in the Owning Plume Browser

**Files:**
- Modify: `src/features/browser/TaskBrowserWorkspace.tsx`
- Modify: `src/features/browser/BrowserPanel.tsx`
- Modify: `src/features/browser/BrowserPanel.test.tsx`
- Modify: `src/App.tsx`
- Modify: `src/App.test.tsx`
- Modify: `src/features/project-shell/NoProjectChatView.tsx`

**Interfaces:**
- `ChatPanelProps.onOpenResearchSource?: (owner: SessionIdentity, url: string) => void`.
- `TaskBrowserWorkspace.openRequest?: { key: number; url: string }`.

- [ ] **Step 1: Write failing App and Browser tests**

Click a research source from a local and project chat. Assert App keeps the
same session identity, activates Browser, and passes the exact URL as a new
request. Assert Browser calls its existing `navigate(url)` path. Prove a
localhost `needsApproval` result still shows the existing approval UI.

- [ ] **Step 2: Run focused tests and verify RED**

Run: `npm run test -- src/App.test.tsx src/features/browser/BrowserPanel.test.tsx`

Expected: FAIL because the open request is not wired.

- [ ] **Step 3: Implement owner-fenced window routing**

Reject the callback when the current surface identity differs from the source
owner. Otherwise set `{ key: previous + 1, url }`, activate Browser, and pass
the request through `TaskBrowserWorkspace` to `BrowserPanel`.

- [ ] **Step 4: Consume each Browser request exactly once**

Track the last request key in a ref. On a new key, call the same `navigateTo`
used by the address bar so URL normalization, localhost approval, errors, and
history remain centralized.

- [ ] **Step 5: Run focused tests and verify GREEN**

Run: `npm run test -- src/App.test.tsx src/features/browser/BrowserPanel.test.tsx`

Expected: all owner and Browser policy tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/App.tsx src/App.test.tsx src/features/browser src/features/project-shell/NoProjectChatView.tsx
git commit -m "feat: open research sources in Plume Browser"
```

### Task 6: Documentation, Exact-Head Verification, and Packaged Smoke

**Files:**
- Modify: `docs/USER_GUIDE.md`
- Modify: `docs/SMOKE_TESTING.md`
- Modify: `docs/FEATURE_INVENTORY.md`
- Modify: `docs/ARCHITECTURE.md`
- Modify: `docs/IPC_CONTRACT.md`
- Modify: `src/features/README.md`
- Modify: `src-tauri/src/README.md`

**Interfaces:**
- Documents only the exact shipped Stage A behavior and the separately future search/fetch target.

- [ ] **Step 1: Update current contracts and inventory**

Describe normal chat research intent, typed persisted artifact/export refs,
source Browser handoff, and explicit-only export. Remove card/selectors from
current behavior. Keep search/fetch/agent Browser authority unshipped.

- [ ] **Step 2: Run focused and full frontend tests**

Run:

```bash
npm run test -- src/features/research src/features/chat src/features/sessions src/features/browser src/App.test.tsx
npm run typecheck
npm run test
```

Expected: all tests pass with no unhandled errors.

- [ ] **Step 3: Run the full verifier**

Run: `PLUME_FULL_VERIFY=1 ./scripts/verify.sh`

Expected: 0 failures; only documented soft-cap warnings remain.

- [ ] **Step 4: Commit docs and evidence pins**

```bash
git add docs src/features/README.md src-tauri/src/README.md
git commit -m "docs: pin transcript-native research evidence"
```

- [ ] **Step 5: Run packaged 1152x768 smoke**

Run `./scripts/smoke-app.sh` once. Verify a research request shows only normal
chat plus progress/Stop, completion shows the answer and source links, source
click opens the same chat's Plume Browser, completion creates no file, and the
explicit export phrase creates one Markdown attachment. Quit the app and prove
no packaged Plume process remains.

- [ ] **Step 6: Exact-head review and publish**

Review `git diff codex/shell-archive-cleanup...HEAD`, verify the exact SHA and
clean worktree, push `codex/chat-context-cleanup`, and rewrite draft PR #168 so
its title/body describe the transcript-native implementation. Do not merge.
