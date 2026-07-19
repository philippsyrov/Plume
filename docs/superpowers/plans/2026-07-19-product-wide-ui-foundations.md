# Product-Wide UI Foundations Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Establish one tested visual foundation for Plume's existing surfaces so later screen cleanups reuse the same typography, spacing, controls, focus, themes, and floating-surface rules.

**Architecture:** Keep the existing CSS import architecture and React ownership boundaries. Promote the already-shipped consumer-shell values into global semantic tokens, make the existing `ink-*` primitives consume those tokens, and normalize the shared modal/disclosure shell without changing application state, routes, IPC, provider behavior, or screen structure.

**Tech Stack:** React 19, strict TypeScript, CSS custom properties, Vitest, Testing Library, Tauri 2 packaged-app smoke, existing Plume assets and dependencies only.

## Global Constraints

- Preserve Plume's warm paper-and-ink identity; add no gradient, glossy card, novelty decoration, emoji control, handcrafted icon, or competing icon set.
- Use the existing macOS system stack for interface and prose; reserve monospace for code, paths, commands, technical identifiers, and measured values.
- Use whitespace and type hierarchy before borders; borders identify controls, selected items, or major regions.
- Preserve one obvious primary action per state, visible focus, keyboard operation, accessible names, error visibility, and reduced-motion behavior.
- Preserve Apple, Qwen, Ollama, trust, memory, provenance, Browser, research, patch, and session behavior exactly.
- Add no dependency, runtime, provider, network call, model download, IPC command, persistence key, filesystem authority, prompt authority, or tool authority.
- Keep every code file at or below the enforced 800-line cap. This slice must reduce `src/styles/layout/project-shell.css`, not grow it.
- Start each observable contract change with a failing test. Run focused tests before the full verifier.

---

## File Structure

- `src/styles/tokens.css`: single source of truth for palette, semantic type scale, control geometry, surface geometry, focus, shadows, and light/dark token values.
- `src/styles/ink.css`: existing reusable panel, divider, badge, and button primitives; consumes tokens and owns their hover, active, focus, and disabled states.
- `src/styles/layout/surfaces.css`: shared disclosure, modal frame, modal header, modal close control, backdrop, and reduced-motion rules extracted from the oversized project-shell stylesheet.
- `src/styles/layout.css`: imports `surfaces.css` before screen-specific styles.
- `src/styles/layout/project-shell.css`: keeps project-shell layout and Settings body composition; stops owning global tokens and shared floating-surface rules.
- `src/features/project-shell/visualFoundation.test.ts`: CSS contract tests for token ownership, semantic values, primitives, theme parity, and import order.
- `src/features/project-shell/Disclosure.test.tsx`: keeps native disclosure behavior and pins the new shared stylesheet owner.
- `src/features/project-shell/ModalDialog.test.tsx`: keeps focus trapping/restoration and pins the shared modal shell classes.
- `src/features/project-shell/supportedMinimumLayout.test.ts`: reads global theme tokens from `tokens.css` after their ownership moves out of `project-shell.css`.
- `docs/UI_STYLE.md`: current contributor contract for the semantic foundation and its allowed usage.

---

### Task 1: Promote semantic visual tokens to the global source of truth

**Files:**
- Create: `src/features/project-shell/visualFoundation.test.ts`
- Modify: `src/features/project-shell/typographyTokens.test.ts`
- Modify: `src/features/project-shell/supportedMinimumLayout.test.ts`
- Modify: `src/styles/tokens.css`
- Modify: `src/styles/layout/project-shell.css`

**Interfaces:**
- Consumes: `document.documentElement.dataset.plumeTheme` values `light` and `dark`; existing `--paper`, `--ink`, `--space-*`, `--text-*`, and `--plume-chrome-*` consumers.
- Produces: global CSS tokens `--type-page-title`, `--type-section-title`, `--type-body`, `--type-secondary`, `--type-metadata`, `--leading-title`, `--leading-body`, `--leading-compact`, `--surface-fill`, `--surface-muted`, `--surface-hover`, `--surface-line`, `--surface-line-strong`, `--radius-control`, `--radius-panel`, `--radius-window`, `--shadow-panel`, and `--shadow-control`.

- [ ] **Step 1: Write the failing global-token ownership tests**

Create `src/features/project-shell/visualFoundation.test.ts`:

```ts
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

const read = (path: string) => readFileSync(join(process.cwd(), path), 'utf8');
const tokens = read('src/styles/tokens.css');
const projectShell = read('src/styles/layout/project-shell.css');

function tokenValue(name: string, css = tokens): string {
  const match = css.match(new RegExp(`--${name}:\\s*([^;]+);`));
  if (!match?.[1]) throw new Error(`missing --${name}`);
  return match[1].trim();
}

describe('product-wide visual foundation', () => {
  it('defines one semantic type and geometry scale', () => {
    expect(tokenValue('type-page-title')).toBe('20px');
    expect(tokenValue('type-section-title')).toBe('15px');
    expect(tokenValue('type-body')).toBe('14px');
    expect(tokenValue('type-secondary')).toBe('12px');
    expect(tokenValue('type-metadata')).toBe('11px');
    expect(tokenValue('leading-title')).toBe('1.2');
    expect(tokenValue('leading-body')).toBe('1.45');
    expect(tokenValue('leading-compact')).toBe('1.3');
    expect(tokenValue('radius-control')).toBe('8px');
    expect(tokenValue('radius-panel')).toBe('10px');
    expect(tokenValue('radius-window')).toBe('16px');
  });

  it('owns shared surface values globally rather than inside one project screen', () => {
    for (const name of [
      'surface-fill',
      'surface-muted',
      'surface-hover',
      'surface-line',
      'surface-line-strong',
      'shadow-panel',
      'shadow-control',
    ]) {
      expect(() => tokenValue(name)).not.toThrow();
    }
    expect(tokenValue('plume-chrome-fill')).toBe('var(--surface-fill)');
    expect(tokenValue('plume-chrome-line')).toBe('var(--surface-line)');
    expect(projectShell).not.toMatch(/--plume-chrome-(?:line|fill|muted|hover|radius|shadow)/);
  });

  it('provides dark values at the document theme boundary', () => {
    expect(tokens).toMatch(/\[data-plume-theme='dark'\]\s*\{/);
    expect(tokens).toMatch(/\[data-plume-theme='dark'\][^}]*--surface-fill:\s*#1b1b19;/s);
    expect(tokens).toMatch(/\[data-plume-theme='dark'\][^}]*--surface-line-strong:\s*#55534c;/s);
    expect(projectShell).not.toContain("[data-plume-theme='dark']");
  });
});
```

- [ ] **Step 2: Extend the failing typography test**

Append this case to `src/features/project-shell/typographyTokens.test.ts`:

```ts
it('maps the legacy size tokens onto the semantic scale', () => {
  expect(tokenValue('text-xs')).toBe('var(--type-metadata)');
  expect(tokenValue('text-sm')).toBe('var(--type-secondary)');
  expect(tokenValue('text-md')).toBe('var(--type-body)');
  expect(tokenValue('text-lg')).toBe('var(--type-section-title)');
  expect(tokenValue('text-xl')).toBe('16px');
});
```

This compatibility mapping prevents a product-wide visual jump before each
screen is deliberately migrated to semantic names.

- [ ] **Step 3: Run the focused tests and verify red**

Run:

```bash
npx vitest run src/features/project-shell/visualFoundation.test.ts src/features/project-shell/typographyTokens.test.ts src/features/project-shell/supportedMinimumLayout.test.ts
```

Expected: `visualFoundation.test.ts` fails because the semantic and surface
tokens do not exist, and `typographyTokens.test.ts` fails because the legacy
tokens still contain literal pixel values.

- [ ] **Step 4: Add semantic tokens and compatibility aliases**

In `src/styles/tokens.css`, replace the current typography-size block and add
the shared surface block with these exact values:

```css
  /* Semantic typography */
  --type-page-title: 20px;
  --type-section-title: 15px;
  --type-body: 14px;
  --type-secondary: 12px;
  --type-metadata: 11px;
  --leading-title: 1.2;
  --leading-body: 1.45;
  --leading-compact: 1.3;

  /* Compatibility aliases while screen styles migrate deliberately. */
  --text-xs: var(--type-metadata);
  --text-sm: var(--type-secondary);
  --text-md: var(--type-body);
  --text-lg: var(--type-section-title);
  --text-xl: 16px;

  /* Shared controls and floating surfaces */
  --surface-fill: #fffefa;
  --surface-muted: #f4f2eb;
  --surface-hover: #fbfaf5;
  --surface-line: #dedad0;
  --surface-line-strong: #c8c2b6;
  --radius-control: 8px;
  --radius-panel: 10px;
  --radius-window: 16px;
  --shadow-panel: 0 14px 36px rgba(17, 17, 17, 0.08);
  --shadow-control:
    inset 0 0 0 1px var(--surface-line-strong),
    0 1px 2px rgba(17, 17, 17, 0.06);

  /* Compatibility aliases for untouched screen styles. */
  --plume-chrome-fill: var(--surface-fill);
  --plume-chrome-muted: var(--surface-muted);
  --plume-chrome-hover: var(--surface-hover);
  --plume-chrome-line: var(--surface-line);
  --plume-chrome-line-strong: var(--surface-line-strong);
  --plume-chrome-radius-control: var(--radius-control);
  --plume-chrome-radius-panel: var(--radius-panel);
  --plume-chrome-radius-window: var(--radius-window);
  --plume-chrome-shadow-panel: var(--shadow-panel);
  --plume-chrome-control-shadow: var(--shadow-control);
```

Retain the existing `--radius-small`, `--radius-soft`, `--stroke`, and
`--stroke-thin` aliases because older feature styles still consume them. Set
the existing focus token to `--focus-ring: 2px solid var(--ink);` so it remains
visible in both themes without a separate dark override.

At file end add the document-level dark override:

```css
[data-plume-theme='dark'] {
  color-scheme: dark;
  --paper: #1b1b19;
  --paper-deep: #22221f;
  --ink: #f1f0eb;
  --ink-soft: #d4d2ca;
  --pencil: #a7a49a;
  --menu-fill: #242421;
  --surface-fill: #1b1b19;
  --surface-muted: #242421;
  --surface-hover: #2e2e2a;
  --surface-line: #3a3935;
  --surface-line-strong: #55534c;
  --shadow-panel: 0 8px 20px rgba(0, 0, 0, 0.28);
  --shadow-control:
    inset 0 0 0 1px var(--surface-line-strong),
    0 1px 2px rgba(0, 0, 0, 0.24);
}
```

- [ ] **Step 5: Replace scoped consumer-shell variables with semantic aliases**

Delete the `.plume-project { --plume-chrome-* }` declaration block and both
dark-theme variable blocks from `src/styles/layout/project-shell.css`.

Then replace every consumer-shell token name in that file mechanically:

```text
--plume-chrome-fill           -> --surface-fill
--plume-chrome-muted          -> --surface-muted
--plume-chrome-hover          -> --surface-hover
--plume-chrome-line           -> --surface-line
--plume-chrome-line-strong    -> --surface-line-strong
--plume-chrome-radius-control -> --radius-control
--plume-chrome-radius-panel   -> --radius-panel
--plume-chrome-radius-window  -> --radius-window
--plume-chrome-shadow-panel   -> --shadow-panel
--plume-chrome-control-shadow -> --shadow-control
```

Do not change selectors, declarations, spacing, layout, or component markup in
this step.

- [ ] **Step 6: Update the existing minimum-layout assertions**

In `src/features/project-shell/supportedMinimumLayout.test.ts`, keep
`tokensCss` and change the theme/surface cases to:

```ts
it('keeps modal copy on the active appearance ink token', () => {
  expect(ruleBody(projectShellCss, '.plume-project-settings-window')).toMatch(
    /color:\s*var\(--ink\)/,
  );
});

it('applies global dark appearance tokens to every surface', () => {
  expect(tokensCss).toMatch(/\[data-plume-theme='dark'\]\s*\{/);
  expect(tokensCss).toMatch(/--surface-fill:\s*#1b1b19/);
  expect(ruleBody(projectShellCss, '.plume-project-codex')).toMatch(
    /background:\s*var\(--surface-fill\)/,
  );
});
```

Update other assertions in this file from `--plume-chrome-*` to the exact new
semantic token names where the production declaration moved.

- [ ] **Step 7: Run the focused tests and typecheck**

Run:

```bash
npx vitest run src/features/project-shell/visualFoundation.test.ts src/features/project-shell/typographyTokens.test.ts src/features/project-shell/supportedMinimumLayout.test.ts src/features/appearance/useAppearance.test.tsx
npm run typecheck
```

Expected: all named tests pass and TypeScript reports no errors.

- [ ] **Step 8: Commit the token migration**

```bash
git add src/styles/tokens.css src/styles/layout/project-shell.css src/features/project-shell/visualFoundation.test.ts src/features/project-shell/typographyTokens.test.ts src/features/project-shell/supportedMinimumLayout.test.ts
git commit -m "feat: establish the shared visual token system"
```

---

### Task 2: Normalize the existing Ink primitives

**Files:**
- Modify: `src/features/project-shell/visualFoundation.test.ts`
- Modify: `src/styles/ink.css`

**Interfaces:**
- Consumes: Task 1's semantic tokens and the existing class names `.ink-panel`, `.ink-divider`, `.ink-badge`, and `.ink-button`.
- Produces: unchanged class names with consistent surface, typography, control height, hover, active, focus, and disabled behavior. No React prop or markup change.

- [ ] **Step 1: Add failing primitive-contract tests**

Add these constants and test to `visualFoundation.test.ts`:

```ts
const ink = read('src/styles/ink.css');

function ruleBody(css: string, selector: string): string {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const match = css.match(new RegExp(`${escaped}\\s*\\{([^}]*)\\}`, 's'));
  if (!match?.[1]) throw new Error(`missing rule ${selector}`);
  return match[1];
}

it('gives existing Ink primitives one consistent control contract', () => {
  expect(ruleBody(ink, '.ink-panel')).toMatch(/border:\s*1px solid var\(--surface-line\)/);
  expect(ruleBody(ink, '.ink-panel')).toMatch(/border-radius:\s*var\(--radius-panel\)/);
  expect(ruleBody(ink, '.ink-button')).toMatch(/min-height:\s*var\(--control-height\)/);
  expect(ruleBody(ink, '.ink-button')).toMatch(/font-size:\s*var\(--type-body\)/);
  expect(ruleBody(ink, '.ink-button:hover:not\(:disabled\)')).toMatch(/background:\s*var\(--surface-hover\)/);
  expect(ruleBody(ink, '.ink-button:active:not\(:disabled\)')).toMatch(/transform:\s*translateY\(1px\)/);
  expect(ruleBody(ink, '.ink-button:focus-visible')).toMatch(/outline:\s*2px solid var\(--ink\)/);
  expect(ruleBody(ink, '.ink-button:disabled')).toMatch(/opacity:\s*0\.58/);
  expect(ruleBody(ink, '.ink-badge')).toMatch(/font-size:\s*var\(--type-metadata\)/);
});
```

- [ ] **Step 2: Run the focused test and verify red**

Run:

```bash
npx vitest run src/features/project-shell/visualFoundation.test.ts
```

Expected: FAIL because `ink.css` still uses the legacy stroke, radius, literal
font-size, and lacks shared hover/active/opacity rules.

- [ ] **Step 3: Implement the minimal shared primitive rules**

Replace `src/styles/ink.css` with:

```css
/* Shared paper-and-ink primitives. Screen styles may adjust layout, but these
   classes own the common surface, type, interaction, and focus contract. */

.ink-panel {
  border: 1px solid var(--surface-line);
  border-radius: var(--radius-panel);
  background: var(--surface-fill);
}

.ink-divider {
  margin: var(--space-3) 0;
  border: 0;
  border-top: 1px solid var(--surface-line);
}

.ink-badge {
  display: inline-flex;
  align-items: center;
  min-height: 20px;
  padding: 0 var(--space-2);
  border: 1px solid var(--surface-line-strong);
  border-radius: var(--radius-small);
  background: var(--surface-muted);
  color: var(--ink-soft);
  font-family: var(--font-ui);
  font-size: var(--type-metadata);
  line-height: var(--leading-compact);
}

.ink-button {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: var(--space-2);
  min-height: var(--control-height);
  padding: 0 var(--space-3);
  border: 0;
  border-radius: var(--radius-control);
  background: var(--surface-fill);
  color: var(--ink);
  box-shadow: var(--shadow-control);
  font-family: var(--font-ui);
  font-size: var(--type-body);
  line-height: var(--leading-compact);
  cursor: pointer;
}

.ink-button:hover:not(:disabled) {
  background: var(--surface-hover);
}

.ink-button:active:not(:disabled) {
  transform: translateY(1px);
}

.ink-button:focus-visible {
  outline: 2px solid var(--ink);
  outline-offset: 2px;
}

.ink-button:disabled {
  color: var(--pencil);
  opacity: 0.58;
  cursor: not-allowed;
}
```

Do not add primary, danger, or quiet variants yet. The screen slices will add
only the variants they actually use.

- [ ] **Step 4: Run focused primitive and consumer tests**

Run:

```bash
npx vitest run src/features/project-shell/visualFoundation.test.ts src/features/chat/ChatPanel.test.tsx src/features/benchmarks/benchmarksStyle.test.ts src/features/providers/LocalModelsPanel.test.tsx
```

Expected: all named tests pass. If an existing screen override intentionally
owns a different height or density, preserve that screen override rather than
adding a new global exception.

- [ ] **Step 5: Commit the primitive normalization**

```bash
git add src/styles/ink.css src/features/project-shell/visualFoundation.test.ts
git commit -m "feat: normalize shared ink controls"
```

---

### Task 3: Extract and normalize shared floating surfaces

**Files:**
- Create: `src/styles/layout/surfaces.css`
- Modify: `src/styles/layout.css`
- Modify: `src/styles/layout/project-shell.css`
- Modify: `src/features/project-shell/visualFoundation.test.ts`
- Modify: `src/features/project-shell/Disclosure.test.tsx`
- Modify: `src/features/project-shell/ModalDialog.test.tsx`

**Interfaces:**
- Consumes: `ModalDialog` classes `.plume-project-settings-backdrop` and `.plume-project-settings-window`; `Disclosure` classes `.plume-disclosure`, `.plume-disclosure-summary`, and `.plume-disclosure-content`; Task 1 tokens.
- Produces: the same DOM and class names, with shared styling owned by `surfaces.css`. Focus trap, Escape, outside-click close, focus restoration, native details behavior, and accessible names remain unchanged.

- [ ] **Step 1: Add failing stylesheet-ownership tests**

Add to `visualFoundation.test.ts`:

```ts
const layout = read('src/styles/layout.css');
const surfaces = read('src/styles/layout/surfaces.css');

it('loads shared surfaces before screen-specific styles', () => {
  expect(layout).toMatch(
    /@import '\.\/layout\/surfaces\.css';[\s\S]*@import '\.\/layout\/project-shell\.css';/,
  );
});

it('keeps modal and disclosure geometry in the shared surface layer', () => {
  expect(surfaces).toMatch(/\.plume-project-settings-backdrop\s*\{/);
  expect(surfaces).toMatch(/\.plume-project-settings-window\s*\{/);
  expect(surfaces).toMatch(/\.plume-disclosure-summary\s*\{/);
  expect(surfaces).toMatch(/@media \(prefers-reduced-motion:\s*reduce\)/);
  expect(projectShell).not.toMatch(/\.plume-project-settings-backdrop\s*\{/);
  expect(projectShell).not.toMatch(/\.plume-disclosure-summary\s*\{/);
});
```

Because reading a missing file throws during collection, create an empty
`src/styles/layout/surfaces.css` in the same patch as the test. The assertions
still fail for missing rules, which is the intended red state.

- [ ] **Step 2: Pin the component classes at the DOM boundary**

Append to `ModalDialog.test.tsx`:

```tsx
it('uses the shared modal shell classes without changing dialog semantics', () => {
  render(
    <ModalDialog labelledBy="modal-title" onClose={vi.fn()}>
      <h2 id="modal-title">Settings</h2>
      <button type="button">Close</button>
    </ModalDialog>,
  );

  expect(screen.getByRole('dialog', { name: 'Settings' })).toHaveClass(
    'plume-project-settings-window',
  );
  expect(screen.getByRole('dialog', { name: 'Settings' }).parentElement).toHaveClass(
    'plume-project-settings-backdrop',
  );
});
```

Change the stylesheet reads in `Disclosure.test.tsx`:

```ts
const surfaces = read('src/styles/layout/surfaces.css');
```

Then replace both `shell` references in the opaque-fill test with `surfaces`.
Keep the session-menu assertion reading `project-shell.css`, because session
menus remain screen-owned in this slice. Change the reduced-motion assertion
to read `surfaces`.

- [ ] **Step 3: Run the focused tests and verify red**

Run:

```bash
npx vitest run src/features/project-shell/visualFoundation.test.ts src/features/project-shell/ModalDialog.test.tsx src/features/project-shell/Disclosure.test.tsx
```

Expected: the DOM behavior tests pass, while stylesheet ownership tests fail
because the rules are still in `project-shell.css`.

- [ ] **Step 4: Move shared rules without changing their selectors**

Add this import to `src/styles/layout.css` immediately after `shell.css`:

```css
@import './layout/surfaces.css';
```

Move these complete rule blocks from `project-shell.css` into
`surfaces.css`, replacing only old token names with Task 1 semantic names:

```text
.plume-project-codex :is(button, select, textarea, input):focus-visible
.plume-disclosure-summary
.plume-disclosure-summary:focus-visible
.plume-disclosure-content
.plume-project-settings-backdrop
.plume-project-settings-window
.plume-project-settings-header
.plume-project-settings-header h3
.plume-project-settings-header p
.plume-project-settings-close
@media (prefers-reduced-motion: reduce)
```

The moved modal frame must still use these declarations:

```css
.plume-project-settings-window {
  width: min(1040px, calc(100vw - 48px));
  max-height: min(820px, calc(100vh - 48px));
  display: flex;
  flex-direction: column;
  min-height: 0;
  overflow: hidden;
  border: 1px solid var(--surface-line);
  border-radius: var(--radius-window);
  background: var(--surface-fill);
  color: var(--ink);
  box-shadow:
    0 30px 90px rgba(17, 17, 17, 0.30),
    0 2px 0 rgba(255, 255, 255, 0.86) inset;
}
```

Normalize the moved header typography to semantic tokens:

```css
.plume-project-settings-header h3 {
  margin: 0;
  font-family: var(--font-prose);
  font-size: var(--type-section-title);
  font-weight: 600;
  line-height: var(--leading-title);
}

.plume-project-settings-header p {
  margin: var(--space-1) 0 0;
  color: var(--pencil);
  font-family: var(--font-ui);
  font-size: var(--type-secondary);
  line-height: var(--leading-compact);
}
```

Do not move `.plume-project-settings-body` or any Settings page/card rules;
their information architecture belongs to the later Settings slice.

- [ ] **Step 5: Run the focused behavior and stylesheet tests**

Run:

```bash
npx vitest run src/features/project-shell/visualFoundation.test.ts src/features/project-shell/ModalDialog.test.tsx src/features/project-shell/Disclosure.test.tsx src/features/project-shell/supportedMinimumLayout.test.ts src/features/sessions/SessionDialogs.test.tsx src/App.test.tsx
npm run typecheck
```

Expected: all named tests pass, modal focus behavior is unchanged, and
TypeScript reports no errors.

- [ ] **Step 6: Commit the shared surface extraction**

```bash
git add src/styles/layout.css src/styles/layout/surfaces.css src/styles/layout/project-shell.css src/features/project-shell/visualFoundation.test.ts src/features/project-shell/Disclosure.test.tsx src/features/project-shell/ModalDialog.test.tsx
git commit -m "refactor: centralize shared floating surfaces"
```

---

### Task 4: Document, verify, and visually prove the foundation

**Files:**
- Modify: `docs/UI_STYLE.md`
- Modify: `docs/superpowers/specs/2026-07-19-product-wide-calm-ui-design.md`

**Interfaces:**
- Consumes: Tasks 1–3 exact CSS contract and the current packaged smoke flow.
- Produces: current contributor guidance and exact-head before/after evidence for later screen slices. It does not claim the product-wide cleanup is complete.

- [ ] **Step 1: Update the current style contract**

In `docs/UI_STYLE.md`, replace the current typography bullets with:

```markdown
## Typography

- Interface and prose use the macOS-first system stack in `--font-ui` and
  `--font-prose`; code, paths, commands, identifiers, and measured values use
  `--font-code`.
- Use semantic sizes: `--type-page-title`, `--type-section-title`,
  `--type-body`, `--type-secondary`, and `--type-metadata`.
- Use `--leading-title`, `--leading-body`, and `--leading-compact`; do not add
  one-off line heights without a screen-specific readability reason.
- Italics do not represent routine status. Use secondary colour and semantic
  size instead.
```

Add this section after the visual rules:

```markdown
## Shared foundations

`src/styles/tokens.css` owns palette, type, spacing, control geometry, surface
geometry, focus, shadows, and light/dark values. `src/styles/ink.css` owns the
existing reusable paper-and-ink controls. `src/styles/layout/surfaces.css` owns
shared modal, disclosure, focus, and reduced-motion rules.

Screen styles may arrange these primitives and deliberately adjust density,
but must not redefine the global token families. Prefer whitespace and type
hierarchy before adding another border. A future screen slice should add a new
variant only when a shipped state actually uses it.
```

- [ ] **Step 2: Mark only the foundations delivery in the approved spec**

In `docs/superpowers/specs/2026-07-19-product-wide-calm-ui-design.md`, change
the status to:

```markdown
**Status:** Approved; foundations candidate implemented, screen slices pending
```

Do not mark Settings, Chat, Library, Browser, Files, Help, Benchmarks, session
management, README screenshots, or efficiency measurement complete.

- [ ] **Step 3: Run documentation and full verification**

Run:

```bash
npm run verify:docs
git diff --check
PLUME_FULL_VERIFY=1 ./scripts/verify.sh
```

Expected: full verification reports zero failures. The three existing
documentation soft-cap warnings may remain; do not expand this slice to fix
them.

- [ ] **Step 4: Build the exact packaged app**

Run without downloading models or runtimes:

```bash
CARGO_NET_OFFLINE=true ./scripts/dev-env.sh bash -lc 'source "$HOME/.cargo/env" 2>/dev/null; npm run tauri -- build --debug --bundles app --config src-tauri/tauri.smoke.conf.json'
```

Expected: the debug smoke bundle is created under
`src-tauri/target/debug/bundle/macos/Plume Smoke.app`.

- [ ] **Step 5: Perform the exact-head visual and interaction smoke**

At 1152×768 and the configured 900×600 minimum, compare the new packaged app
beside the accepted audit screenshots for:

```text
light: empty project Chat, model chooser, Search, Settings header/body,
       project trust, Help, projectless Library
dark:  Settings, Chat, model chooser, Search
keys:  Tab/Shift+Tab, Escape, focus return, disclosure toggle
states: disabled button, provider error, destructive action, reduced motion
```

Pass when:

```text
- type hierarchy is consistent and no text clips or overlaps;
- controls share height, radius, focus, and disabled treatment unless a
  documented compact override applies;
- light/dark modal and disclosure surfaces are opaque and readable;
- no layout, route, copy, authority, provider, or persistence behavior changed;
- every captured after-image is labelled with the exact Git head.
```

If a screenshot reveals a screen-specific problem, record it for its owning
slice rather than broadening this foundations PR.

- [ ] **Step 6: Commit the verified documentation**

```bash
git add docs/UI_STYLE.md docs/superpowers/specs/2026-07-19-product-wide-calm-ui-design.md
git commit -m "docs: record the shared UI foundation"
```

- [ ] **Step 7: Run the pre-PR exact-head gate**

Run:

```bash
PLUME_FULL_VERIFY=1 ./scripts/verify.sh
git status --short
git rev-parse HEAD
```

Expected: verification has zero failures, the worktree is clean, and the exact
head is recorded for findings-only review. Do not push, open a PR, or merge
without the user's explicit instruction.

---

## Self-Review Record

- **Spec coverage:** This plan implements only delivery slice 1, Foundations.
  Trust, Chat, Settings information architecture, Library, Help, Benchmarks,
  Files, Browser, sessions, final README screenshots, and efficiency
  measurement remain intentionally assigned to later plans.
- **Authority coverage:** No task changes React state ownership, IPC, local
  models, trust, persistence, context, research, Browser, patch, memory, or
  session behavior.
- **Type consistency:** All tasks use the same semantic token names defined by
  Task 1. Existing `ink-*`, modal, and disclosure class names remain unchanged.
- **Placeholder scan:** The plan contains no unresolved markers, unspecified
  error handling, or open-ended implementation step.
- **Risk boundary:** The only broad visual change is globalizing token values
  already used by the consumer shell. Exact-head light/dark and minimum-window
  smoke is mandatory before the PR is considered complete.
