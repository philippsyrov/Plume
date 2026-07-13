import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

const layoutCss = readFileSync(join(process.cwd(), 'src/styles/layout.css'), 'utf8');
const knowledgeCss = readFileSync(
  join(process.cwd(), 'src/styles/layout/knowledge.css'),
  'utf8',
);

function blockOf(css: string, selector: string): string {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const match = css.match(new RegExp(`${escaped}\\s*\\{([^}]*)\\}`, 's'));
  if (!match?.[1]) throw new Error(`no rule block found for ${selector}`);
  return match[1];
}

describe('Knowledge workspace style contract', () => {
  it('loads the focused stylesheet immediately after Benchmarks', () => {
    expect(layoutCss).toMatch(
      /@import '\.\/layout\/benchmarks\.css';\s*@import '\.\/layout\/knowledge\.css';/,
    );
  });

  it('keeps vertical scrolling on the trusted workspace surface', () => {
    const workspace = blockOf(knowledgeCss, '.plume-project-knowledge-view');

    expect(workspace).toMatch(/min-height:\s*0/);
    expect(workspace).toMatch(/overflow-y:\s*auto/);
  });

  it('bounds navigation, wraps memory text, and collapses on constrained widths', () => {
    const grid = blockOf(knowledgeCss, '.plume-knowledge-grid');
    const wrapping = blockOf(
      knowledgeCss,
      '.plume-knowledge-memory p,\n.plume-knowledge-topic-content',
    );

    expect(grid).toMatch(
      /grid-template-columns:\s*minmax\(190px,\s*260px\)\s+minmax\(0,\s*1fr\)/,
    );
    expect(wrapping).toMatch(/overflow-wrap:\s*anywhere/);
    expect(knowledgeCss).toMatch(
      /@media\s*\(max-width:\s*760px\)[\s\S]*?\.plume-knowledge-grid\s*\{[\s\S]*?grid-template-columns:\s*minmax\(0,\s*1fr\)/,
    );
  });
});
