import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';

import { Disclosure } from './Disclosure';

const read = (relativePath: string) =>
  readFileSync(join(process.cwd(), relativePath), 'utf8');

describe('Disclosure', () => {
  it('uses native details keyboard behavior and keeps detail closed by default', async () => {
    const user = userEvent.setup();
    render(
      <Disclosure summary="Project instructions">
        <p>AGENTS.md · 2.1 KB</p>
      </Disclosure>,
    );

    const summary = screen.getByText('Project instructions');
    const details = summary.closest('details');
    expect(details).not.toHaveAttribute('open');

    await user.tab();
    expect(summary).toHaveFocus();

    await user.click(summary);
    expect(details).toHaveAttribute('open');

    await user.click(summary);
    expect(details).not.toHaveAttribute('open');
  });

  it('uses an opaque shared fill for disclosure and menu surfaces', () => {
    const tokens = read('src/styles/tokens.css');
    const shell = read('src/styles/layout/project-shell.css');
    const surfaces = read('src/styles/layout/surfaces.css');

    expect(tokens).toMatch(/--menu-fill:\s*#[0-9a-f]{6};/i);
    expect(surfaces).toMatch(
      /\.plume-disclosure-content\s*\{[^}]*background:\s*var\(--menu-fill\)/s,
    );
    expect(shell).toMatch(
      /\.plume-session-menu\s*\{[^}]*background:\s*var\(--menu-fill\)/s,
    );
  });

  it('pins reduced-motion behavior for shell controls', () => {
    const surfaces = read('src/styles/layout/surfaces.css');

    expect(surfaces).toMatch(/@media \(prefers-reduced-motion:\s*reduce\)/);
    expect(surfaces).toMatch(/transition-duration:\s*0\.01ms\s*!important/);
    expect(surfaces).toMatch(/animation-duration:\s*0\.01ms\s*!important/);
  });
});
