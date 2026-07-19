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
});
