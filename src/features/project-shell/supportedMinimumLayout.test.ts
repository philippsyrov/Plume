import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

const read = (relativePath: string) =>
  readFileSync(join(process.cwd(), relativePath), 'utf8');

const projectShellCss = read('src/styles/layout/project-shell.css');
const modelChooserCss = read('src/styles/layout/model-chooser.css');
const shellCss = read('src/styles/layout/shell.css');
const browserCss = read('src/styles/layout/browser.css');
const tokensCss = read('src/styles/tokens.css');
const surfacesCss = read('src/styles/layout/surfaces.css');
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
    expect(body).not.toContain('plume-no-project-model-picker');
    expect(ruleBody(modelChooserCss, '.plume-model-chooser-trigger')).toMatch(
      /min-width:\s*120px/,
    );
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

  it('keeps the warm paper shell as the default instead of silently following macOS dark mode', () => {
    expect(ruleBody(projectShellCss, '.plume-project-main')).not.toMatch(
      /linear-gradient/,
    );
    expect(ruleBody(projectShellCss, '.plume-project-sidebar')).not.toMatch(
      /linear-gradient/,
    );
    expect(ruleBody(projectShellCss, '.plume-unified-topbar')).toMatch(
      /background:\s*var\(--plume-chrome-fill\)/,
    );
    expect(projectShellCss).not.toContain('@media (prefers-color-scheme: dark)');
  });

  it('preserves Browser child geometry at the narrow supported layout', () => {
    const sidebarWidth = Number(tokensCss.match(/--sidebar-width:\s*(\d+)px/)?.[1]);
    const safeBrowserStackWidth = sidebarWidth + 360 + 8 + 320;
    expect(ruleBody(browserCss, '.plume-browser-split')).toMatch(
      /grid-template-columns:\s*minmax\(360px,\s*1fr\)\s+0\s+minmax\(320px,\s*var\(--plume-browser-split-width,\s*560px\)\)/,
    );
    expect(browserCss).toMatch(
      new RegExp(`@media\\s*\\(max-width:\\s*${safeBrowserStackWidth}px\\)[\\s\\S]*\\.plume-browser-split\\s*\\{[\\s\\S]*grid-template-areas:\\s*"browser"\\s*"chat"`),
    );
    expect(browserCss).not.toMatch(
      /\.plume-browser-split\s+\.plume-browser-chat\s*\{[^}]*display:\s*none/,
    );
    expect(ruleBody(projectShellCss, '.plume-project-codex')).toMatch(
      /grid-template-columns:\s*var\(--sidebar-width\)\s+minmax\(0,\s*1fr\)/,
    );
  });

  it('uses shared radius tokens for Browser tabs', () => {
    expect(ruleBody(browserCss, '.plume-browser-tab')).toMatch(
      /border-radius:\s*var\(--radius-small\)/,
    );
    expect(browserCss).not.toContain('.plume-browser-attach-menu');
    expect(browserCss).not.toMatch(/--radius-(?:control|menu)/);
  });

  it('keeps Browser notices in a chrome row outside native host geometry', () => {
    expect(ruleBody(browserCss, '.plume-browser-page.has-chrome-stack')).toMatch(
      /grid-template-rows:\s*38px 46px auto minmax\(180px, 1fr\)/,
    );
    expect(ruleBody(browserCss, '.plume-browser-chrome-stack')).toMatch(/display:\s*flex/);
    expect(ruleBody(browserCss, '.plume-browser-notice')).not.toMatch(/position:\s*absolute/);
  });

  it('keeps split chat padded while the resize target overlays the seam', () => {
    expect(ruleBody(browserCss, '.plume-browser-split .plume-browser-chat')).toMatch(
      /padding-inline:\s*var\(--space-3\)/,
    );
    const resizer = ruleBody(browserCss, '.plume-browser-resizer');
    expect(resizer).toMatch(/width:\s*12px/);
    expect(resizer).toMatch(/transform:\s*translateX\(-12px\)/);
    expect(resizer).not.toMatch(/border-left:/);
  });

  it('keeps expanded Browser chat as a compact centered composer', () => {
    const expandedChat = ruleBody(browserCss, '.plume-browser-expanded .plume-browser-chat');
    const hiddenExpandedChat = ruleBody(browserCss, '.plume-browser-expanded .plume-browser-chat[hidden]');
    expect(expandedChat).toMatch(/width:\s*clamp\(480px,\s*62%,\s*900px\)/);
    expect(expandedChat).toMatch(/max-width:\s*calc\(100% - 32px\)/);
    expect(expandedChat).toMatch(/justify-self:\s*center/);
    expect(expandedChat).toMatch(/margin:\s*10px 0 12px/);
    expect(expandedChat).toMatch(/background:\s*var\(--plume-chrome-fill/);
    expect(expandedChat).toMatch(/max-height:\s*min\(42vh,\s*360px\)/);
    expect(expandedChat).toMatch(/overflow-y:\s*auto/);
    expect(expandedChat).not.toMatch(/overflow:\s*hidden/);
    expect(hiddenExpandedChat).toMatch(/display:\s*none/);
    expect(ruleBody(browserCss, '.plume-browser-expanded')).toMatch(/grid-template-rows:\s*minmax\(0, 1fr\)/);
  });

  it('keeps modal copy on the active appearance ink token', () => {
    expect(ruleBody(surfacesCss, '.plume-project-settings-window')).toMatch(
      /color:\s*var\(--ink\)/,
    );
  });

  it('applies dark appearance tokens to trusted and untrusted project surfaces', () => {
    expect(tokensCss).toMatch(/\[data-plume-theme='dark'\]\s*\{/);
    expect(tokensCss).toMatch(/--surface-fill:\s*#1b1b19/);
    expect(ruleBody(projectShellCss, '.plume-project-codex')).toMatch(
      /background:\s*var\(--surface-fill\)/,
    );
  });
});
