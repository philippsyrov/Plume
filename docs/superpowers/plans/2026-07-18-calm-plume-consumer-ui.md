# Calm Plume Consumer UI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Simplify first-run chat, model selection, conversation presentation, and Library overview while preserving every existing provider, context, trust, and accessibility contract.

**Architecture:** Keep behavior in the current React owners and change only their presentation contracts. `ModelChooser` retains catalog and selection ownership, `ChatPanel` and `ChatEntryRow` retain chat state and evidence rendering, and `LibraryPanel` retains source and project-generation fences while receiving one existing shell callback for project opening.

**Tech Stack:** React 19, strict TypeScript, Testing Library, Vitest, Tauri 2 packaged-app smoke, existing Plume CSS tokens.

## Global Constraints

- Preserve warm paper, ink, pencil shading, serif prose, and existing state colors; add no gradient, one-off color, inline style, handcrafted icon, or new dependency.
- Preserve the top-bar Model control's accessible name `Model`, current-value description, `aria-expanded`, and dialog relationship.
- Preserve Apple and Qwen catalog states: checking, unavailable, downloading, verifying, starting, failed, start-failed, running, and selected.
- Preserve exact typed-context manifests, local/project memory separation, trust, path, size, hardlink, binary, and redaction boundaries.
- Memory links and backlinks remain organization metadata only and never select prompt context.
- The Browser remains human-controlled; this slice adds no Browser, computer-use, shell, tool, filesystem, prompt, provider, model-download, or IPC authority.
- The existing hand-drawn identity remains; this is hierarchy cleanup, not a new design system.
- Start every behavior change with a failing test and keep every code file at or below 800 lines.

---

## File Structure

- `src/features/model-picker/ModelChooser.tsx`: trigger, compact model rows, catalog actions, focus containment, dismissal, focus restoration.
- `src/features/model-picker/ModelChooser.test.tsx`: model-row semantics, provider-state behavior, Tab/Shift+Tab containment, dismissal, focus restoration.
- `src/styles/layout/model-chooser.css`: one restrained popover, divider-separated rows, narrow-window containment.
- `src/features/chat/disabledReason.ts`: concise no-selection placeholder and silent duplicate status.
- `src/features/chat/disabledReason.test.ts`: pure copy contract for no-selection and all other disabled states.
- `src/features/chat/ChatPanel.tsx`: empty-state CTA, composer status rendering, transcript and Clear placement.
- `src/features/chat/ChatPanel.test.tsx`: empty-state, context honesty, composer, Clear, and transcript preservation.
- `src/features/chat/ChatEntryRow.tsx`: quiet visible `You` and `Plume` labels while preserving accessible message labels and evidence.
- `src/features/chat/ChatEntryRow.test.tsx`: new focused coverage for visible roles, accessible labels, and metadata; existing `ChatPanel.test.tsx` and sibling tests retain manifests, errors, streaming, cancellation, copy, and diff coverage.
- `src/styles/layout/chat.css`: readable transcript measure, flatter turn hierarchy, quiet metadata and Clear, stable composer.
- `src/features/library/LibraryPanel.tsx`: actionable overview rows using current source-selection logic and an optional shell-owned open-project callback.
- `src/features/library/LibraryPanel.test.tsx`: projectless/trusted overview actions and unchanged scope/error/generation fences.
- `src/features/library/LibraryWorkspace.tsx`: pass the existing shell callback into `LibraryPanel`.
- `src/App.tsx`: pass `openProjectModal` to trusted-project Library.
- `src/features/project-shell/NoProjectChatView.tsx`: pass `openProjectModal` to projectless Library.
- `src/styles/layout/library.css`: compact summary rows and quieter Refresh control using existing tokens.
- `docs/UI_STYLE.md`: record the calm hierarchy and compact chooser/overview rules as current UI truth.
- `docs/FEATURE_INVENTORY.md`: update only PR-owned chat/model-picker/Library evidence and behavior; do not stamp unrelated Browser notices.
- `docs/ROADMAP.md`: mark the focused UI cleanup complete and keep deeper guarded coding-agent execution as future work.

---

### Task 1: Compact, keyboard-contained model chooser

**Files:**
- Modify: `src/features/model-picker/ModelChooser.test.tsx`
- Modify: `src/features/model-picker/ModelChooser.tsx`
- Modify: `src/styles/layout/model-chooser.css`

**Interfaces:**
- Consumes: existing `ModelCatalogApi`, `ModelCatalogEntry`, `SelectedModelApi`, and `onOpenChange(open: boolean): void`.
- Produces: unchanged `ModelChooser` public props and a private `focusableDialogItems(dialog: HTMLDivElement): HTMLElement[]` helper.

- [ ] **Step 1: Add failing compact-row and focus-containment tests**

Add these cases to `ModelChooser.test.tsx`:

```tsx
it('renders two compact model rows instead of nested model cards', () => {
  renderChooser({ open: true });

  const dialog = screen.getByRole('dialog', { name: 'Choose a model' });
  expect(within(dialog).getAllByRole('group')).toHaveLength(2);
  expect(dialog.querySelectorAll('.plume-model-chooser-row')).toHaveLength(2);
  expect(dialog.querySelectorAll('.plume-model-chooser-card')).toHaveLength(0);
});

it('contains forward and backward Tab focus while open', async () => {
  render(<ControlledChooser />);
  await userEvent.click(screen.getByRole('button', { name: 'Model' }));

  const dialog = screen.getByRole('dialog', { name: 'Choose a model' });
  const apple = within(dialog).getByRole('button', { name: 'Use Apple Model' });
  const lastDetails = within(dialog).getAllByText('Details').at(-1)!;

  lastDetails.focus();
  await userEvent.keyboard('{Tab}');
  expect(apple).toHaveFocus();

  apple.focus();
  await userEvent.keyboard('{Shift>}{Tab}{/Shift}');
  expect(lastDetails).toHaveFocus();
});
```

Also import `within` from Testing Library.

- [ ] **Step 2: Run the focused test and verify red**

Run:

```bash
npx vitest run src/features/model-picker/ModelChooser.test.tsx
```

Expected: the compact-row case fails because `.plume-model-chooser-card` still exists, and the focus case fails because Tab reaches the outside button.

- [ ] **Step 3: Add Tab containment without changing dismissal behavior**

Extend the existing open-state key handler in `ModelChooser.tsx`:

```tsx
const onKeyDown = (event: KeyboardEvent) => {
  if (event.key === 'Escape') {
    event.preventDefault();
    onOpenChange(false);
    return;
  }
  if (event.key !== 'Tab' || dialogRef.current === null) return;

  const items = focusableDialogItems(dialogRef.current);
  if (items.length === 0) {
    event.preventDefault();
    dialogRef.current.focus();
    return;
  }
  const first = items[0]!;
  const last = items.at(-1)!;
  if (event.shiftKey && (document.activeElement === first || document.activeElement === dialogRef.current)) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && document.activeElement === last) {
    event.preventDefault();
    first.focus();
  }
};
```

Add the private helper at file end:

```tsx
function focusableDialogItems(dialog: HTMLDivElement): HTMLElement[] {
  return Array.from(
    dialog.querySelectorAll<HTMLElement>(
      'button:not(:disabled), summary, [href], input:not(:disabled), textarea:not(:disabled), select:not(:disabled), [tabindex]:not([tabindex="-1"])',
    ),
  ).filter((element) => element.getAttribute('aria-hidden') !== 'true');
}
```

Keep Escape, outside-pointer dismissal, and the existing effect that returns focus to the trigger.

- [ ] **Step 4: Replace card structure with compact row structure**

Change both provider sections to this semantic shape while keeping each existing state/action branch:

```tsx
<section
  className="plume-model-chooser-row"
  role="group"
  aria-labelledby="plume-apple-model-title"
>
  <div className="plume-model-chooser-row-main">
    <div className="plume-model-chooser-copy">
      <h4 id="plume-apple-model-title">Apple On-Device</h4>
      <p>Built into this Mac</p>
    </div>
    <div className="plume-model-chooser-row-action">{action}</div>
  </div>
  {status}
  <ModelDetails entry={entry} />
</section>
```

Use the same structure for Qwen with its existing progress, retry, Cancel, Selected, and Details branches. Do not merge Apple and Qwen state machines.

- [ ] **Step 5: Flatten the chooser CSS**

Replace card selectors with row selectors and pin the compact geometry:

```css
.plume-model-chooser-popover {
  width: min(380px, calc(100vw - 32px));
  padding: var(--space-3);
}

.plume-model-chooser-cards {
  display: grid;
}

.plume-model-chooser-row {
  display: grid;
  gap: var(--space-2);
  padding: var(--space-3) 0;
}

.plume-model-chooser-row + .plume-model-chooser-row {
  border-top: 1px solid var(--plume-chrome-line);
}

.plume-model-chooser-row-main {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
}

.plume-model-chooser-copy {
  min-width: 0;
}

.plume-model-chooser-row-action {
  flex: 0 0 auto;
}
```

Remove the per-card border, radius, background, and nested padding. Keep the existing mobile fixed positioning and status/error/progress/Details styling.

- [ ] **Step 6: Run model-picker tests and typecheck**

Run:

```bash
npx vitest run src/features/model-picker/ModelChooser.test.tsx src/features/model-picker/useModelCatalog.test.tsx src/features/model-picker/useModelCatalog.lifecycle.test.tsx
npm run typecheck
```

Expected: all model-picker tests pass and TypeScript reports no errors.

- [ ] **Step 7: Commit the independently reviewable chooser change**

```bash
git add src/features/model-picker/ModelChooser.tsx src/features/model-picker/ModelChooser.test.tsx src/styles/layout/model-chooser.css
git commit -m "feat: simplify the model chooser"
```

---

### Task 2: Calm empty chat, composer, and transcript

**Files:**
- Modify: `src/features/chat/disabledReason.test.ts`
- Modify: `src/features/chat/disabledReason.ts`
- Modify: `src/features/chat/ChatPanel.test.tsx`
- Modify: `src/features/chat/ChatPanel.tsx`
- Create: `src/features/chat/ChatEntryRow.test.tsx`
- Modify: `src/features/chat/ChatEntryRow.tsx`
- Modify: `src/styles/layout/chat.css`

**Interfaces:**
- Consumes: existing `DisabledReason`, `ChatEntry`, `ChatApi`, `onChooseModel?: () => void`, and exact accepted-turn context manifests.
- Produces: unchanged chat APIs; `chatStatusText(null, 'no-selection', false)` returns an empty string and visible completed-message roles become `You` or `Plume`.

- [ ] **Step 1: Add failing no-selection and empty-state tests**

In `disabledReason.test.ts`, pin the non-repeating copy:

```ts
expect(inputPlaceholder(null, 'no-selection')).toBe('Choose a model to start');
expect(chatStatusText(null, 'no-selection', false)).toBe('');
```

In `ChatPanel.test.tsx`, add:

```tsx
it('offers one no-model action without repeating the state below the composer', () => {
  render(
    <ChatPanel
      selected={null}
      onClearSelection={vi.fn()}
      inspectorSelection={null}
      inspectorLineRange={null}
      projectHasInstructions={false}
      mlxServers={makeMlxServers(null)}
      includeProjectContext={false}
      variant="simple"
      onChooseModel={vi.fn()}
    />,
  );

  expect(screen.getAllByRole('button', { name: 'Choose a model' })).toHaveLength(1);
  expect(screen.getByLabelText('Message to send')).toHaveAttribute(
    'placeholder',
    'Choose a model to start',
  );
  expect(screen.queryByText('No model selected.')).not.toBeInTheDocument();
});
```

Use the file's existing fixture builder rather than introducing a second full `ChatPanel` prop fixture.

- [ ] **Step 2: Add failing transcript-role and evidence-preservation tests**

Create `ChatEntryRow.test.tsx` with completed rows and assert both visible and accessible labels:

```tsx
import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import { ChatEntryRow } from './ChatEntryRow';

describe('ChatEntryRow', () => {
it('uses quiet human role labels without changing message accessibility', () => {
  const { rerender } = render(
    <ChatEntryRow entry={{ kind: 'message', message: { role: 'user', content: 'Hello' } }} />,
  );
  expect(screen.getByLabelText('user message')).toHaveTextContent('You');

  rerender(
    <ChatEntryRow
      entry={{
        kind: 'message',
        message: { role: 'assistant', content: 'Hi' },
        modelUsed: 'Qwen Coder 1.5B',
        durationMs: 564,
      }}
    />,
  );
  expect(screen.getByLabelText('assistant message')).toHaveTextContent('Plume');
  expect(screen.getByText(/served by Qwen Coder 1.5B/)).toBeInTheDocument();
});
});
```

Retain `ChatPanel.test.tsx` and the existing sibling tests that cover context-manifest chips, errors, streaming, cancellation, diff previews, and copy actions.

- [ ] **Step 3: Run the focused chat tests and verify red**

Run:

```bash
npx vitest run src/features/chat/disabledReason.test.ts src/features/chat/ChatPanel.test.tsx src/features/chat/ChatEntryRow.test.tsx
```

Expected: failures show the old period-bearing placeholder, visible `No model selected.`, and lowercase raw role names.

- [ ] **Step 4: Implement the minimal copy and role changes**

In `disabledReason.ts`:

```ts
case 'no-selection':
  return 'Choose a model to start';
```

and in `chatStatusText`:

```ts
case 'no-selection':
  return '';
```

Render the status span only when the returned text is non-empty:

```tsx
{chatStatusText(selected, disabledReason, isStreaming) ? (
  <span className="plume-chat-status" role="status" aria-live="polite">
    {chatStatusText(selected, disabledReason, isStreaming)}
  </span>
) : <span className="plume-chat-form-spacer" aria-hidden="true" />}
```

In `ChatEntryRow.tsx`, derive and render the visible label without changing the list item's `aria-label`:

```tsx
const visibleRole = message.role === 'user' ? 'You' : 'Plume';
// ...
<span className="plume-chat-entry-role">{visibleRole}</span>
```

Use `Plume` for streaming and cancelled assistant rows and `Error` for error rows.

- [ ] **Step 5: Flatten only simple-mode transcript presentation**

Keep developer/legacy panel borders unchanged and add simple-mode overrides:

```css
.plume-chat-simple .plume-chat-entry {
  width: min(100%, 720px);
  padding: var(--space-2) 0;
  border: 0;
  border-radius: 0;
  background: transparent;
}

.plume-chat-simple .plume-chat-entry-user {
  align-self: flex-end;
  max-width: min(82%, 620px);
  padding: var(--space-2) var(--space-3);
  border-radius: var(--plume-chrome-radius-panel);
  background: var(--paper-deep);
}

.plume-chat-simple .plume-chat-entry-assistant,
.plume-chat-simple .plume-chat-entry-streaming,
.plume-chat-simple .plume-chat-entry-cancelled,
.plume-chat-simple .plume-chat-entry-error {
  align-self: center;
}

.plume-chat-entry-role {
  text-transform: none;
  letter-spacing: 0;
  font-size: var(--text-xs);
  font-weight: 600;
}

.plume-chat-entry-meta {
  font-family: var(--font-ui);
  font-size: 10px;
  opacity: 0.78;
}

.plume-chat-form-spacer {
  flex: 1;
}
```

Reduce the simple composer's shadow to the existing control shadow and keep its single boundary. Keep errors, streaming/cancelled distinction, diff borders, and exact evidence chips intact.

- [ ] **Step 6: Run all chat tests and typecheck**

Run:

```bash
npx vitest run src/features/chat
npm run typecheck
```

Expected: every chat test passes and TypeScript reports no errors.

- [ ] **Step 7: Commit the chat cleanup**

```bash
git add src/features/chat/disabledReason.ts src/features/chat/disabledReason.test.ts src/features/chat/ChatPanel.tsx src/features/chat/ChatPanel.test.tsx src/features/chat/ChatEntryRow.tsx src/features/chat/ChatEntryRow.test.tsx src/styles/layout/chat.css
git commit -m "feat: calm the consumer chat surface"
```

---

### Task 3: Useful Library overview with honest scope actions

**Files:**
- Modify: `src/features/library/LibraryPanel.test.tsx`
- Modify: `src/features/library/LibraryPanel.tsx`
- Modify: `src/features/library/LibraryWorkspace.tsx`
- Modify: `src/App.tsx`
- Modify: `src/features/project-shell/NoProjectChatView.tsx`
- Modify: `src/styles/layout/library.css`

**Interfaces:**
- Consumes: existing `projectIdentity: string | null`, internal `selectSection(next: LibrarySection)`, and shell-owned `openProjectModal(): void`.
- Produces: optional `onOpenProject?: () => void` on `LibraryPanel`, required `onOpenProject: () => void` on `LibraryWorkspace`, and no new IPC or storage path.

- [ ] **Step 1: Add failing projectless overview-action test**

Add to `LibraryPanel.test.tsx`:

```tsx
it('turns the projectless overview into two honest actionable summaries', async () => {
  const onOpenProject = vi.fn();
  render(<LibraryPanel projectIdentity={null} onOpenProject={onOpenProject} />);

  const overview = screen.getByRole('region', { name: 'Library overview' });
  expect(within(overview).getByText('1 memory item')).toBeVisible();
  await userEvent.click(within(overview).getByRole('button', { name: 'Browse About you' }));
  expect(screen.getByRole('searchbox', { name: 'Search About you' })).toBeVisible();

  await userEvent.click(screen.getByRole('button', { name: 'Overview' }));
  await userEvent.click(within(screen.getByRole('region', { name: 'Library overview' }))
    .getByRole('button', { name: 'Open project' }));
  expect(onOpenProject).toHaveBeenCalledOnce();
});
```

- [ ] **Step 2: Add failing trusted-project source-action test**

```tsx
it('opens project memory and topics from separate trusted overview actions', async () => {
  render(<LibraryPanel projectIdentity="/project/a" />);
  const overview = screen.getByRole('region', { name: 'Library overview' });

  await userEvent.click(within(overview).getByRole('button', { name: 'Browse This project' }));
  expect(screen.getByRole('searchbox', { name: 'Search This project' })).toBeVisible();

  await userEvent.click(screen.getByRole('button', { name: 'Overview' }));
  await userEvent.click(within(screen.getByRole('region', { name: 'Library overview' }))
    .getByRole('button', { name: 'Browse Topics' }));
  expect(screen.getByRole('searchbox', { name: 'Search Topics' })).toBeVisible();
});
```

- [ ] **Step 3: Run the Library test and verify red**

Run:

```bash
npx vitest run src/features/library/LibraryPanel.test.tsx
```

Expected: failures report missing overview actions.

- [ ] **Step 4: Add overview callbacks without weakening project fences**

Extend `LibraryPanel` props:

```tsx
onOpenProject?: () => void;
```

Pass `selectSection` and `onOpenProject` into `LibraryOverview`. Replace the loose heading/paragraph sequence with two rows:

```tsx
<section className="plume-library-overview" aria-label="Library overview">
  <article className="plume-library-summary-row">
    <div>
      <h3>About you</h3>
      <p>Stored on this Mac and available without opening a project.</p>
      <span>{sourceCount(data.userMemory, 'memory')}</span>
    </div>
    <button type="button" onClick={() => onSelectSection('user-memory')}>
      Browse About you
    </button>
  </article>
  <article className="plume-library-summary-row">
    <div>
      <h3>This project</h3>
      <p>Stored only for this trusted project.</p>
      {projectIdentity === null
        ? <span>Open a trusted project to see its memory and topics.</span>
        : <span>{sourceCount(data.projectMemory, 'memory')} · {sourceCount(data.topics, 'topic')}</span>}
    </div>
    {projectIdentity === null ? (
      onOpenProject ? <button type="button" onClick={onOpenProject}>Open project</button> : null
    ) : (
      <div className="plume-library-summary-actions">
        <button type="button" onClick={() => onSelectSection('project-memory')}>Browse This project</button>
        <button type="button" onClick={() => onSelectSection('topics')}>Browse Topics</button>
      </div>
    )}
  </article>
</section>
```

Do not touch `useLibraryData`, projection, link/backlink rendering, context handoff, or generation refs.

- [ ] **Step 5: Wire the existing open-project callback through the shell**

Add `onOpenProject: () => void` to `LibraryWorkspace`, pass it into `LibraryPanel`, and supply `openProjectModal` at both current call sites in `App.tsx` and `NoProjectChatView.tsx`:

```tsx
<LibraryWorkspace
  projectIdentity={null}
  disabled={persisted.chat.status === 'streaming'}
  onUseInChat={libraryHandoff.useItemInChat}
  onDropSource={libraryHandoff.useSourceInChat}
  onOpenProject={openProjectModal}
/>
```

Use the same prop for the trusted-project call so the component interface is uniform, even though its overview renders source actions instead.

- [ ] **Step 6: Style summary rows and quiet Refresh**

Add:

```css
.plume-library-overview {
  max-width: 760px;
  display: grid;
  align-content: start;
  gap: 0;
}

.plume-library-summary-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-4);
  padding: var(--space-4) 0;
}

.plume-library-summary-row + .plume-library-summary-row {
  border-top: 1px solid var(--plume-chrome-line);
}

.plume-library-summary-row p,
.plume-library-summary-row span {
  color: var(--pencil);
}

.plume-library-summary-actions {
  display: flex;
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: var(--space-2);
}

.plume-library-header > button {
  color: var(--pencil);
  background: transparent;
}
```

Under the existing 760 px media query, stack each summary row and left-align its actions. Use only existing tokens.

- [ ] **Step 7: Run Library, shell, and type tests**

Run:

```bash
npx vitest run src/features/library src/App.test.tsx
npm run typecheck
```

Expected: Library and shell tests pass, including project switch, source isolation, and late-notice fences.

- [ ] **Step 8: Commit the Library cleanup**

```bash
git add src/features/library/LibraryPanel.tsx src/features/library/LibraryPanel.test.tsx src/features/library/LibraryWorkspace.tsx src/features/project-shell/NoProjectChatView.tsx src/App.tsx src/styles/layout/library.css
git commit -m "feat: simplify the Library overview"
```

---

### Task 4: Current documentation and evidence ownership

**Files:**
- Modify: `docs/UI_STYLE.md`
- Modify: `docs/FEATURE_INVENTORY.md`
- Modify: `docs/ROADMAP.md`
- Modify: `src/features/README.md` only if ownership paths changed during implementation.

**Interfaces:**
- Consumes: the exact implementation commits from Tasks 1-3 and their passing tests.
- Produces: current source-of-truth wording and PR-attributable verification pointers only.

- [ ] **Step 1: Update current UI truth**

Add a short **Calm consumer hierarchy** paragraph to `docs/UI_STYLE.md` stating:

```markdown
The consumer chat uses one primary action in an empty state, divider-separated
model rows inside one popover, a readable border-light transcript, and quiet
runtime metadata. Library keeps its source tree and reading canvas; its
overview uses two scope summaries rather than dashboard cards. Borders frame
controls and major regions, not every nested group.
```

- [ ] **Step 2: Refresh only affected inventory records**

Update `library.workspace` to name the actionable overview summaries and add the final packaged-smoke evidence after Task 5. Update the model-picker-owning `providers.apple-foundation` and `providers.mlx-managed` records only if their frontend reachability or automated evidence wording needs the compact chooser contract.

Run:

```bash
git rev-parse HEAD
```

Use that returned implementation ancestor for affected `lastVerifiedCommit` fields. Do not modify `browser.workspace` or other warning-only records unless this PR changes an owned path named by that record.

- [ ] **Step 3: Advance the roadmap without claiming agent execution**

Change the Local Models track's next-deliverable paragraph to say the focused consumer UI cleanup is complete and the remaining target-hardware benchmark matrix is still pending. Keep Apple/Qwen described as chat providers, not a broad tool-executing agent.

- [ ] **Step 4: Verify documentation and commit**

Run:

```bash
npm run verify:docs
git diff --check
```

Expected: documentation checks pass; unrelated pre-existing inventory notices may remain visible and must not be blindly stamped.

```bash
git add docs/UI_STYLE.md docs/FEATURE_INVENTORY.md docs/ROADMAP.md src/features/README.md
git commit -m "docs: record the calm consumer UI"
```

If `src/features/README.md` is unchanged, omit it from `git add`.

---

### Task 5: Full verification and exact-head packaged visual QA

**Files:**
- Modify: implementation or tests only when a verified failure requires a fix.
- Modify: `docs/FEATURE_INVENTORY.md` only to record the final exact-head smoke after any fix commits.

**Interfaces:**
- Consumes: the complete branch from Tasks 1-4 and the baseline screenshots in `/tmp/plume-ui-audit-2026-07-18/`.
- Produces: exact-head automated evidence, matched before/after screenshots, packaged UI evidence, and a review-ready PR.

- [ ] **Step 1: Run focused and full automated verification**

```bash
npx vitest run src/features/model-picker src/features/chat src/features/library src/App.test.tsx
npm run typecheck
npm run test
PLUME_FULL_VERIFY=1 ./scripts/verify.sh
```

Expected: focused suites, TypeScript, full frontend tests, Rust/clippy gates, docs checks, and verifier pass with zero failures. Only documented soft-cap warnings are acceptable.

- [ ] **Step 2: Run pre-commit and gitleaks at the exact implementation head**

Create a no-op verification commit only if required by the repository hook flow; otherwise run the configured hook commands from `docs/DEVELOPMENT.md`. Confirm gitleaks reports no leaks before pushing.

- [ ] **Step 3: Build the packaged app through the project-local environment**

```bash
./scripts/dev-env.sh npm run tauri build
```

Expected: the Apple Silicon app bundle is created under `src-tauri/target/release/bundle/macos/Plume.app` without downloading new model weights or mutating signed resources at runtime.

- [ ] **Step 4: Exercise exact-head UI paths with Computer Use**

At the same 1152×768 window size used by the baseline, verify:

1. fresh New chat shows one Choose a model action and no duplicate no-model status;
2. Model opens two compact rows, contains forward/backward Tab focus, closes with Escape/outside click, and restores trigger focus;
3. Apple selection and one short reply work when host availability reports available;
4. installed Qwen starts, selects, and returns one clean short reply without a control marker;
5. transcript shows quiet You/Plume labels while model/duration evidence remains present;
6. projectless Library browses About you and Open project invokes the existing project chooser;
7. trusted-project Library opens This project and Topics separately without changing context automatically;
8. Settings, Help, and workspace overlays remain reachable above an active Browser;
9. quit/relaunch restores a valid chat/model/Browser state and sweeps managed Qwen on normal Quit.

- [ ] **Step 5: Compare before and after screenshots together**

Capture exact-viewport after images for empty chat, model chooser, and Library. Open each beside its matching baseline:

```text
/tmp/plume-ui-audit-2026-07-18/01-empty-chat.png
/tmp/plume-ui-audit-2026-07-18/02-model-picker.png
/tmp/plume-ui-audit-2026-07-18/03-library.png
```

Reject the implementation if controls crop, text wraps badly, spacing is inconsistent, borders still nest unnecessarily, focus is invisible, or a smaller state is harder to understand. Fix with a failing regression when the problem is behavioral; fix CSS with a narrow style regression when it is visual containment.

- [ ] **Step 6: Record final smoke provenance and re-run docs verification**

After the final fix commit, update only affected inventory manual-evidence text with the exact reviewed head and the paths exercised. Run:

```bash
npm run verify:docs
git diff --check
git add docs/FEATURE_INVENTORY.md
git commit -m "docs: record calm UI packaged smoke"
PLUME_FULL_VERIFY=1 ./scripts/verify.sh
```

- [ ] **Step 7: Push, wait for CI, and request exact-head findings-only review**

```bash
git push -u origin codex/calm-plume-ui
gh pr create --title "Calm Plume consumer UI" --body-file /tmp/calm-plume-pr.md
gh pr checks --watch
```

The PR body must name the exact head, focused tests, full verifier result, documented warnings, packaged-app source head, compared screenshot paths, and unchanged capability firewalls. Do not merge until GitHub verify/gitleaks are green and an independent exact-head findings-only review has no unresolved Important issue.
