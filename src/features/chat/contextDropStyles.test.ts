import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

const css = readFileSync(
  join(process.cwd(), 'src/styles/layout/context-drop.css'),
  'utf8',
);

describe('context drop visual contract', () => {
  it('keeps the temporary tray restrained and disables motion when requested', () => {
    expect(css).toContain('.plume-context-drop-tray');
    expect(css).toContain('.plume-context-shelf-item-emphasized');
    expect(css).toMatch(/@media \(prefers-reduced-motion: reduce\)/);
    expect(css).toMatch(
      /@media \(prefers-reduced-motion: reduce\)[\s\S]*animation:\s*none/,
    );
    expect(css).not.toContain('box-shadow:');
    expect(css).not.toContain('filter: drop-shadow');
  });
});
