import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';

import type { ProjectMeta } from '../../lib/api/project';
import { UntrustedProjectView } from './UntrustedProjectView';

const meta: ProjectMeta = {
  id: 'project-1',
  root: '/Users/example/Code/plume-demo',
  hasAgentsMd: true,
  hasClaudeMd: false,
  packageManagers: ['npm', 'cargo'],
  git: null,
  trust: 'unknown',
};

describe('UntrustedProjectView', () => {
  it('leads with ordinary trust language and keeps exact metadata under Technical details', async () => {
    const user = userEvent.setup();
    const onTrust = vi.fn();
    render(<UntrustedProjectView meta={meta} onTrust={onTrust} onClose={vi.fn()} />);

    const alert = screen.getByRole('alert');
    expect(alert).toHaveTextContent(
      "Until you trust it, Plume won't read its files or use project tools.",
    );
    expect(alert).toHaveTextContent(
      'Trust applies to this folder on this Mac. Moving or renaming it asks again.',
    );
    expect(alert).not.toHaveTextContent(/canonical path|per-machine|re-prompts/i);

    const summary = screen.getByText('Technical details');
    const details = summary.closest('details');
    expect(details).not.toBeNull();
    expect(details).not.toHaveAttribute('open');
    expect(within(details!).getByText(meta.root)).toBeInTheDocument();
    expect(within(details!).getByText('AGENTS.md')).toBeInTheDocument();
    expect(within(details!).getByText('CLAUDE.md')).toBeInTheDocument();
    expect(within(details!).getByText('npm')).toBeInTheDocument();
    expect(within(details!).getByText('cargo')).toBeInTheDocument();

    await user.click(summary);
    expect(details).toHaveAttribute('open');
    await user.click(screen.getByRole('button', { name: 'Trust this project' }));
    expect(onTrust).toHaveBeenCalledWith(meta.root);
  });
});
