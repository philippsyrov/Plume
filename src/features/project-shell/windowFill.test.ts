import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

// D64: the unified workspace fills the window edge-to-edge — the
// compact shell has no outer gutter and the codex surface has no
// card frame (the OS window chrome is the frame). Layout lives in
// CSS that happy-dom doesn't load, so (like the D61/D87/D98 layout
// contracts) we assert against the stylesheet source directly.
const read = (rel: string) => readFileSync(join(process.cwd(), rel), 'utf8');
const shellCss = read('src/styles/layout/shell.css');
const projectShellCss = read('src/styles/layout/project-shell.css');
const surfacesCss = read('src/styles/layout/surfaces.css');

/** The body of the first `selector { … }` block in `css`. */
function blockOf(css: string, selector: string): string {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const match = css.match(new RegExp(`${escaped}\\s*\\{([^}]*)\\}`, 's'));
  if (!match?.[1]) throw new Error(`no rule block found for ${selector}`);
  return match[1];
}

describe('Window-fill unified shell (D64)', () => {
  it('drops the outer gutter on the compact shell', () => {
    const compact = blockOf(shellCss, '.plume-shell-compact');
    expect(compact).toMatch(/padding:\s*0/);
    expect(compact).toMatch(/gap:\s*0/);
    // The single-row collapse (D13) must survive the gutter removal.
    expect(compact).toMatch(/grid-template-rows:\s*1fr/);
  });

  it('keeps the padded card layout for the hero views', () => {
    // Only the compact variant goes edge-to-edge; the open form and
    // trust gate keep the base shell's outer padding.
    expect(blockOf(shellCss, '.plume-shell')).toMatch(/padding:\s*var\(--space-5\)/);
  });

  it('removes the card frame from the codex surface root', () => {
    const codex = blockOf(projectShellCss, '.plume-project-codex');
    // No window-level border, radius, or shadow — custom-property
    // definitions (`--plume-chrome-*`) don't count as declarations.
    expect(codex).not.toMatch(/^\s*border:/m);
    expect(codex).not.toMatch(/^\s*border-radius:/m);
    expect(codex).not.toMatch(/^\s*box-shadow:/m);
    // Internal scrolling contract: the root still clips, panes
    // inside own their own scroll (D13 rule).
    expect(codex).toMatch(/overflow:\s*hidden/);
  });

  it('keeps the internal dividers that now separate the full-bleed panes', () => {
    // With the outer frame gone, these hairlines are what visually
    // separates sidebar from main and topbar from content.
    expect(blockOf(projectShellCss, '.plume-project-sidebar')).toMatch(
      /border-right:\s*1px solid var\(--plume-chrome-line\)/,
    );
    expect(blockOf(projectShellCss, '.plume-unified-topbar')).toMatch(
      /border-bottom:\s*1px solid var\(--plume-chrome-line\)/,
    );
  });

  it('keeps rounded corners and shadows on the internal floating surfaces', () => {
    expect(blockOf(projectShellCss, '.plume-tool-drawer')).toMatch(
      /border-radius:\s*var\(--plume-chrome-radius-window\)/,
    );
    expect(blockOf(surfacesCss, '.plume-project-settings-window')).toMatch(
      /border-radius:\s*var\(--radius-window\)/,
    );
  });
});
