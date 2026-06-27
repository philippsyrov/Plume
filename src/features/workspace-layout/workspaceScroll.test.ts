import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

// D98: the left column (which holds the Agent settings, Run-one-step, and
// dry-run cards plus the file tree / providers / memory) is a scroll
// container, so every panel — and a long agent event log — is reachable at
// any window height instead of being clipped by the shell's overflow:hidden.
// Layout lives in CSS that happy-dom doesn't load, so (like the D61/D87
// layout contracts) we assert against the stylesheet source directly.
const read = (rel: string) => readFileSync(join(process.cwd(), rel), 'utf8');
const workspaceCss = read('src/styles/layout/workspace.css');
const navigatorCss = read('src/styles/layout/navigator.css');
const innerPanelsCss = read('src/styles/layout/inner-panels.css');

describe('Left-column scroll contract (D98)', () => {
  it('makes the left column a vertical scroll container', () => {
    expect(workspaceCss).toMatch(
      /\.plume-workspace-left\s*\{[^}]*overflow-y:\s*auto[^}]*\}/s,
    );
  });

  it('holds the panels at natural height so the column scrolls past them', () => {
    // Without this the flex children would shrink to fit (no overflow → no
    // scroll), re-clipping the agent cards.
    expect(workspaceCss).toMatch(
      /\.plume-workspace-left\s*>\s*\*\s*\{[^}]*flex-shrink:\s*0[^}]*\}/s,
    );
  });

  it('caps the file navigator so it co-scrolls instead of collapsing or dominating', () => {
    // It must no longer `flex: 1`-fill the column: with a scrolling column a
    // fill-and-grow tree collapses to nothing when siblings overflow. A
    // max-height + its own internal scroll keeps it bounded.
    expect(navigatorCss).toMatch(/\.plume-navigator\s*\{[^}]*max-height:[^}]*\}/s);
    expect(navigatorCss).not.toMatch(/\.plume-navigator\s*\{[^}]*flex:\s*1;[^}]*\}/s);
  });

  it('pins the inner-panel toggle strip so the recovery chips stay reachable mid-scroll', () => {
    expect(innerPanelsCss).toMatch(
      /\.plume-inner-toggles\s*\{[^}]*position:\s*sticky[^}]*\}/s,
    );
  });
});
