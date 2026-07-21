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
  it('presents one focused trust decision with safety and metadata behind disclosures', async () => {
    const user = userEvent.setup();
    const onTrust = vi.fn();
    const onClose = vi.fn();
    const { container } = render(
      <UntrustedProjectView meta={meta} onTrust={onTrust} onClose={onClose} />,
    );

    expect(container.querySelectorAll('.plume-trust-decision')).toHaveLength(1);
    expect(container.querySelector('[data-tauri-drag-region="true"]')).toBeInTheDocument();
    expect(screen.queryByText('Project safety')).not.toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Open plume-demo?' })).toBeInTheDocument();
    expect(screen.getByText(meta.root)).toBeInTheDocument();
    expect(screen.getByText('Plume needs your trust before it can read this folder.')).toBeInTheDocument();

    const safetySummary = screen.getByText('What does trust allow?');
    const safetyDetails = safetySummary.closest('details');
    expect(safetyDetails).not.toBeNull();
    expect(safetyDetails).not.toHaveAttribute('open');
    expect(within(safetyDetails!).getByText(/read eligible files in this folder/i)).toBeInTheDocument();
    expect(
      within(safetyDetails!).getByText(/changes still require you to choose Apply/i),
    ).toBeInTheDocument();
    expect(
      within(safetyDetails!).getByText(/moving or renaming the folder asks again/i),
    ).toBeInTheDocument();
    expect(safetyDetails).not.toHaveTextContent(/canonical path|per-machine|re-prompts/i);

    const technicalSummary = screen.getByText('Technical details');
    const technicalDetails = technicalSummary.closest('details');
    expect(technicalDetails).not.toBeNull();
    expect(technicalDetails).not.toHaveAttribute('open');
    expect(within(technicalDetails!).getByText('AGENTS.md')).toBeInTheDocument();
    expect(within(technicalDetails!).getByText('CLAUDE.md')).toBeInTheDocument();
    expect(within(technicalDetails!).getByText('npm')).toBeInTheDocument();
    expect(within(technicalDetails!).getByText('cargo')).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(onClose).toHaveBeenCalledOnce();
    await user.click(screen.getByRole('button', { name: 'Trust and open' }));
    expect(onTrust).toHaveBeenCalledWith(meta.root);
  });

  it('keeps the trust decision keyboard-visible without weakening the choice', () => {
    render(<UntrustedProjectView meta={meta} onTrust={vi.fn()} onClose={vi.fn()} />);

    expect(screen.getByRole('button', { name: 'Cancel' })).toBeEnabled();
    expect(screen.getByRole('button', { name: 'Trust and open' })).toBeEnabled();
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    expect(screen.queryByText(/already trusted|recommended/i)).not.toBeInTheDocument();
  });
});
