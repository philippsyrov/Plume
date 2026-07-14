import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

const read = (relativePath: string) =>
  readFileSync(join(process.cwd(), relativePath), 'utf8');

const projectShellCss = read('src/styles/layout/project-shell.css');
const shellCss = read('src/styles/layout/shell.css');
const browserCss = read('src/styles/layout/browser.css');
const tauriConfig = JSON.parse(read('src-tauri/tauri.conf.json')) as {
  app: { windows: Array<{ label: string; minWidth: number; minHeight: number }> };
};

function compactMediaBlock(css: string): { breakpoint: number; body: string } {
  const match = css.match(/@media \(max-width:\s*(\d+)px\)\s*\{([\s\S]*?)\n\}/);
  if (!match?.[1] || !match[2]) throw new Error('compact media block not found');
  return { breakpoint: Number(match[1]), body: match[2] };
}

function ruleBody(css: string, selector: string): string {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const match = css.match(new RegExp(`${escaped}\\s*\\{([^}]*)\\}`, 's'));
  if (!match?.[1]) throw new Error(`rule not found for ${selector}`);
  return match[1];
}

describe('layout at the supported Tauri window minimum', () => {
  it('keeps the compact breakpoint reachable and close to the 900px minimum', () => {
    const mainWindow = tauriConfig.app.windows.find(({ label }) => label === 'main');
    expect(mainWindow).toMatchObject({ minWidth: 900, minHeight: 600 });

    const { breakpoint } = compactMediaBlock(projectShellCss);
    expect(breakpoint).toBeGreaterThanOrEqual(mainWindow!.minWidth);
    expect(breakpoint).toBeLessThanOrEqual(mainWindow!.minWidth + 60);
  });

  it('preserves the compact shell rules needed at minimum width', () => {
    const { body } = compactMediaBlock(projectShellCss);
    expect(ruleBody(body, '.plume-project-codex')).toMatch(
      /grid-template-columns:\s*240px minmax\(0, 1fr\)/,
    );
    expect(ruleBody(body, '.plume-project-files-view')).toMatch(
      /grid-template-columns:\s*1fr/,
    );
    expect(ruleBody(body, '.plume-unified-topbar')).toMatch(/flex-wrap:\s*wrap/);
    expect(ruleBody(body, '.plume-unified-actions')).toMatch(/flex-wrap:\s*wrap/);
    expect(
      ruleBody(body, '.plume-unified-actions .plume-no-project-model-picker'),
    ).toMatch(/min-width:\s*100%[\s\S]*max-width:\s*100%/);
    expect(ruleBody(body, '.plume-open-project-form')).toMatch(
      /grid-template-columns:\s*1fr/,
    );
  });

  it('keeps document-level horizontal overflow clipped', () => {
    expect(ruleBody(shellCss, 'html,\nbody,\n#root')).toMatch(/overflow:\s*hidden/);
  });

  it('reserves macOS traffic-light clearance without shrinking the main column', () => {
    expect(ruleBody(shellCss, ':root')).toMatch(
      /--plume-macos-titlebar-clearance:\s*38px/,
    );
    expect(ruleBody(projectShellCss, '.plume-project-sidebar')).toMatch(
      /padding-top:\s*var\(--plume-macos-titlebar-clearance\)/,
    );
    expect(ruleBody(projectShellCss, '.plume-unified-topbar')).toMatch(
      /min-height:\s*var\(--plume-macos-titlebar-clearance\)/,
    );
  });

  it('uses one solid shell surface with an explicit dark-theme counterpart', () => {
    expect(ruleBody(projectShellCss, '.plume-project-main')).not.toMatch(
      /linear-gradient/,
    );
    expect(ruleBody(projectShellCss, '.plume-project-sidebar')).not.toMatch(
      /linear-gradient/,
    );
    expect(ruleBody(projectShellCss, '.plume-unified-topbar')).toMatch(
      /background:\s*var\(--plume-chrome-fill\)/,
    );
    expect(projectShellCss).toMatch(
      /@media \(prefers-color-scheme:\s*dark\)[\s\S]*\.plume-project-codex\s*\{[\s\S]*--plume-chrome-fill:/,
    );
  });

  it('preserves Browser child geometry at the narrow supported layout', () => {
    expect(ruleBody(browserCss, '.plume-browser-split')).toMatch(
      /grid-template-columns:\s*minmax\(320px,\s*var\(--plume-browser-split-width,\s*560px\)\)\s+8px\s+minmax\(300px,\s*1fr\)/,
    );
    expect(ruleBody(projectShellCss, '.plume-project-codex')).toMatch(
      /grid-template-columns:\s*var\(--sidebar-width\)\s+minmax\(0,\s*1fr\)/,
    );
  });
});
