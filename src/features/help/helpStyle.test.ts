import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

describe('Help layout styles', () => {
  it('gives the compact topic list breathing room without restoring cards', () => {
    const styles = readFileSync('src/styles/layout/project-shell.css', 'utf8');
    const body = styles.match(/\.plume-help-body\s*\{([^}]*)\}/s)?.[1] ?? '';

    expect(body).toMatch(/padding:\s*var\(--space-4\)/);
    expect(styles).toMatch(/\.plume-help-topics li \+ li\s*\{[^}]*border-top:/s);
    expect(styles).not.toMatch(/\.plume-help-topics li\s*\{[^}]*border:\s*1px/s);
  });
});
