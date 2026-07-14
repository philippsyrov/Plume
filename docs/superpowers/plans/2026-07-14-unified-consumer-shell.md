# Unified Consumer Shell Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Plume read like one calm consumer app: one identity, collapsible sidebar, merged macOS chrome, consistent type/icons/menus, and a simple chat composer with advanced detail disclosed on demand.

**Architecture:** Consolidate shared shell state in `UnifiedChrome`, reduce `App.tsx` routing duplication, and replace locally invented visual values with audited tokens and small reusable controls. Preserve all current capabilities and authority; this slice changes presentation and navigation, not prompt assembly or execution.

**Tech Stack:** React 19, TypeScript, CSS custom properties, Tauri window styling, Vitest, Testing Library, packaged visual smoke.

## Global Constraints

- Do not remove Continue, Rewind, context provenance, model state, patch apply/revert, or safety errors.
- Rename technical concepts in visible copy only; exact filenames/ids/bytes remain under Details.
- Do not add a decorative Scheduled destination.
- Use one system UI face for controls/prose and one monospace face for code/evidence.
- No gradients, glass, heavy shadows, emoji/glyph icons, or unexplained compact toggles.

---

### Task 1: Token and component inventory

**Files:**
- Modify: `src/styles/tokens.css`
- Create: `src/features/project-shell/Icon.tsx`
- Create: `src/features/project-shell/Icon.test.tsx`
- Create: `src/features/project-shell/Disclosure.tsx`
- Create: `src/features/project-shell/Disclosure.test.tsx`
- Modify: `src/styles/layout/project-shell.css`

**Interfaces:**
- `IconName` is a closed union; `Icon` is decorative unless an explicit accessible label is provided by its button owner.
- `Disclosure({ summary, children })` preserves native keyboard semantics and exposes exact detail only on demand.

```tsx
<Disclosure summary="Project instructions">
  <ProjectInstructionDetails fileName="AGENTS.md" usage={usage} />
</Disclosure>
```

- [ ] Write tests pinning the allowed icon names, accessible-label behavior, Details disclosure keyboard flow, opaque popover backgrounds, and reduced motion.
- [ ] Add tokens for titlebar/sidebar/control sizes, layers, menu fill, focus ring, and typography scale; delete superseded local values as consumers migrate.
- [ ] Implement one inline SVG icon component using `currentColor`; replace CSS-drawn/glyph icons only when their owner migrates.
- [ ] Implement a reusable native `details`-based disclosure for exact provenance.
- [ ] Run `npm run test -- src/features/project-shell/Icon.test.tsx src/features/project-shell/Disclosure.test.tsx`; confirm GREEN.
- [ ] Commit: `refactor: unify shell controls`.

### Task 2: One identity and collapsible sidebar

**Files:**
- Modify: `src/features/project-shell/UnifiedSidebar.tsx`
- Modify: `src/features/project-shell/UnifiedSidebar.test.tsx`
- Modify: `src/features/project-shell/UnifiedChrome.tsx`
- Modify: `src/features/project-shell/UnifiedChrome.test.tsx`
- Modify: `src/App.tsx`
- Modify: `src/styles/layout/project-shell.css`

**Interfaces:**
- `UnifiedSidebar` gains `collapsed`, `onCollapsedChange`, `onLibrary`, and `onHelp`.
- `plume:sidebar-v1` stores only `{ collapsed: boolean }`; invalid JSON falls back expanded.

- [ ] Run `npm run test -- src/features/project-shell/UnifiedSidebar.test.tsx src/features/project-shell/UnifiedChrome.test.tsx`; expected RED is missing single-action/collapse behavior.

- [ ] Add failing tests for one visible `Plume`, one `New chat`, Search, Library, tasks/projects, quiet Settings/Help footer, collapse/restore, and persisted sidebar preference.
- [ ] Replace separate local/project creation labels with one New chat action whose lightweight chooser explains Chat versus Project when a project is open.
- [ ] Keep project grouping and scope badges visible on rows without repeating `local chat`/`project chat` across the canvas.
- [ ] Add visible collapse/expand controls and keyboard access; preserve session selection and Browser geometry after collapse.
- [ ] Remove the redundant footer identity/trust copy; move trust to the project row/status detail.
- [ ] Commit: `feat: simplify consumer navigation`.

### Task 3: Merged titlebar and coherent top bar

**Files:**
- Modify: `src-tauri/tauri.conf.json`
- Modify: `src/features/project-shell/UnifiedChrome.tsx`
- Modify: `src/features/project-shell/UnifiedChrome.test.tsx`
- Modify: `src/styles/layout/shell.css`
- Modify: `src/styles/layout/project-shell.css`

**Interfaces:**
- `UnifiedChrome` owns the single task title and draggable titlebar region.
- Interactive descendants use `data-tauri-drag-region="false"`; only empty chrome space drags the window.

- [ ] Add configuration/unit tests for overlay/transparent-titlebar behavior supported by the pinned Tauri version and a draggable region that excludes controls.
- [ ] Merge the top strip with the shell surface in light and dark themes while retaining macOS traffic-light clearance.
- [ ] Show task title once; model/project status becomes compact named controls, not repeated Plume subtitles.
- [ ] Prove narrow-window minimums and Browser child geometry remain correct.
- [ ] Commit: `feat: merge macos titlebar`.

### Task 4: Consumer chat composer and progressive disclosure

**Files:**
- Modify: `src/features/chat/ChatPanel.tsx`
- Modify: `src/features/chat/ChatPanel.test.tsx`
- Modify: `src/features/chat/ModeToggle.tsx`
- Modify: `src/features/chat/InstructionsBadge.tsx`
- Modify: `src/features/chat/ContextPreview.tsx`
- Modify: `src/features/chat/ContextShelf.tsx`
- Modify: `src/styles/layout/chat.css`
- Modify: `src/styles/layout/system-chips.css`

**Interfaces:**
- `TaskAction = 'answer' | 'proposeDiff'`; visible descriptions map to existing wire modes without changing payload values.
- `Project instructions` summary owns a Details disclosure containing `AGENTS.md` and exact preview facts.

- [ ] Run `npm run test -- src/features/chat/ChatPanel.test.tsx src/features/chat/InstructionsBadge.test.tsx`; expected RED is the old technical copy and floating mode control.

- [ ] Add failing tests for a familiar empty composer, one direct model action, action-mode explanation, and no raw paragraph glyph/filename/byte count in the default view.
- [ ] Rename `AGENTS.md` to **Project instructions** in visible copy; move exact file/size and prompt manifests into Details.
- [ ] Replace the floating Chat/Propose diff segment with an explained action selector near the composer; preserve sent-turn mode badges and backend payloads.
- [ ] Keep context chips readable by human title/type; exact refs and byte counts remain inspectable.
- [ ] Ensure local Chat never presents project-action affordances.
- [ ] Commit: `feat: simplify chat composer`.

### Task 5: Menus, Continue/Rewind, and visual consistency

**Files:**
- Modify: `src/features/sessions/SessionRow.tsx`
- Modify: `src/features/sessions/SessionRow.test.tsx`
- Modify: `src/features/sessions/SessionDialogs.tsx`
- Modify: `src/features/sessions/SessionDialogs.test.tsx`
- Modify: `src/features/project-shell/ToolDrawer.tsx`
- Modify: `src/features/project-shell/ToolDrawer.test.tsx`
- Modify: relevant `src/styles/layout/*.css`

- [ ] Add tests for opaque menus, viewport-safe placement, Escape/outside-click, focus return, and keyboard navigation.
- [ ] Add plain explanations: Continue copies the whole conversation into a new chat; Rewind creates a new chat ending before selected recent turns; originals stay unchanged.
- [ ] Replace remaining ellipsis/glyph icons and inconsistent row/control heights with shared components/tokens.
- [ ] Run a CSS scan test forbidding transparent menu fills, unapproved font families, and new hardcoded radii in migrated surfaces.
- [ ] Run `npm run test -- src/features/sessions/SessionRow.test.tsx src/features/sessions/SessionDialogs.test.tsx src/features/project-shell/ToolDrawer.test.tsx`; confirm GREEN.
- [ ] Commit: `fix: polish menus and branching actions`.

### Task 6: Visual smoke and publication

- [ ] Update `docs/UI_STYLE.md`, inventory, and smoke checklist with presentation-only truth.
- [ ] Package and inspect light/dark, project/no-project, narrow/large, empty/populated, menus/dialogs, split/expanded Browser, and reduced motion.
- [ ] Capture before/after screenshots for review without including private project names/content.
- [ ] Run full verification and exact-head review before merge.
