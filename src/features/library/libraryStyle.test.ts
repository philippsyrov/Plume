import fs from 'node:fs';

import { describe, expect, it } from 'vitest';

describe('Library workspace styles', () => {
  it('loads its owned stylesheet and collapses the three-pane browser on narrow windows', () => {
    const layout = fs.readFileSync('src/styles/layout.css', 'utf8');
    const styles = fs.readFileSync('src/styles/layout/library.css', 'utf8');

    expect(layout).toContain("@import './layout/library.css';");
    expect(styles).toMatch(/\.plume-library-browser\s*\{[^}]*grid-template-columns:/s);
    expect(styles).toMatch(/@media \(max-width: 760px\)[\s\S]*\.plume-library-grid/s);
    expect(styles).toContain('overflow-y: auto');
  });
});
