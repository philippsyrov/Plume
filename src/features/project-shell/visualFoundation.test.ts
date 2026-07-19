import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

const read = (path: string) => readFileSync(join(process.cwd(), path), 'utf8');
const tokens = read('src/styles/tokens.css');
const projectShell = read('src/styles/layout/project-shell.css');
const ink = read('src/styles/ink.css');

function tokenValue(name: string, css = tokens): string {
  const match = css.match(new RegExp(`--${name}:\\s*([^;]+);`));
  if (!match?.[1]) throw new Error(`missing --${name}`);
  return match[1].trim();
}

function ruleBody(css: string, selector: string): string {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const match = css.match(new RegExp(`${escaped}\\s*\\{([^}]*)\\}`, 's'));
  if (!match?.[1]) throw new Error(`missing rule ${selector}`);
  return match[1];
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
    expect(projectShell).not.toMatch(
      /^\s*--plume-chrome-(?:line|fill|muted|hover|radius|shadow)[\w-]*\s*:/m,
    );
  });

  it('provides dark values at the document theme boundary', () => {
    expect(tokens).toMatch(/\[data-plume-theme='dark'\]\s*\{/);
    expect(tokens).toMatch(
      /\[data-plume-theme='dark'\][^}]*--surface-fill:\s*#1b1b19;/s,
    );
    expect(tokens).toMatch(
      /\[data-plume-theme='dark'\][^}]*--surface-line-strong:\s*#55534c;/s,
    );
    expect(projectShell).not.toContain("[data-plume-theme='dark']");
  });

  it('gives existing Ink primitives one consistent control contract', () => {
    expect(ruleBody(ink, '.ink-panel')).toMatch(
      /border:\s*1px solid var\(--surface-line\)/,
    );
    expect(ruleBody(ink, '.ink-panel')).toMatch(
      /border-radius:\s*var\(--radius-panel\)/,
    );
    expect(ruleBody(ink, '.ink-button')).toMatch(
      /min-height:\s*var\(--control-height\)/,
    );
    expect(ruleBody(ink, '.ink-button')).toMatch(
      /font-size:\s*var\(--type-body\)/,
    );
    expect(ruleBody(ink, '.ink-button:hover:not\(:disabled\)')).toMatch(
      /background:\s*var\(--surface-hover\)/,
    );
    expect(ruleBody(ink, '.ink-button:active:not\(:disabled\)')).toMatch(
      /transform:\s*translateY\(1px\)/,
    );
    expect(ruleBody(ink, '.ink-button:focus-visible')).toMatch(
      /outline:\s*2px solid var\(--ink\)/,
    );
    expect(ruleBody(ink, '.ink-button:disabled')).toMatch(/opacity:\s*0\.58/);
    expect(ruleBody(ink, '.ink-badge')).toMatch(
      /font-size:\s*var\(--type-metadata\)/,
    );
  });
});
