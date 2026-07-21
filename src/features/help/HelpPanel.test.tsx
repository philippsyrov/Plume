import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';

import { HelpPanel } from './HelpPanel';

const handbook = `# Plume Handbook

## Chat or Project?

Chat answers questions without access to a project folder.

## Available now / Planned

- Available now: task-owned Browser.
- Planned: scheduled automation.

| Feature | Status |
| --- | --- |
| **Browser** | [Available now](https://example.com) |
`;

describe('HelpPanel', () => {
  it('opens the bundled Handbook in place without an external link', async () => {
    render(<HelpPanel handbook={handbook} onClose={vi.fn()} />);

    expect(screen.getByRole('dialog', { name: 'Help' })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Chat or Project?' })).toBeInTheDocument();
    expect(screen.queryByRole('link')).not.toBeInTheDocument();

    expect(screen.getByRole('list', { name: 'Common help topics' })).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Open handbook' }));

    expect(screen.getByRole('button', { name: 'Back to Help' })).toHaveFocus();
    expect(screen.getByRole('heading', { name: 'Plume Handbook' })).toBeInTheDocument();
    expect(screen.getByText('Available now: task-owned Browser.')).toBeInTheDocument();
    expect(screen.getByText('Planned: scheduled automation.')).toBeInTheDocument();
    expect(screen.getByRole('table')).toHaveTextContent('Browser');
    expect(screen.getByRole('table')).toHaveTextContent('Available now');
    expect(screen.getByRole('table')).not.toHaveTextContent('---');
    expect(screen.queryByRole('link')).not.toBeInTheDocument();
  });

  it('returns from the full guide, traps focus, closes on Escape, and restores focus', async () => {
    const onClose = vi.fn();
    render(
      <>
        <button type="button">Background action</button>
        <HelpPanel handbook={handbook} onClose={onClose} />
      </>,
    );

    const close = screen.getByRole('button', { name: 'Close help' });
    await waitFor(() => expect(close).toHaveFocus());
    await userEvent.click(screen.getByRole('button', { name: 'Open handbook' }));
    await userEvent.click(screen.getByRole('button', { name: 'Back to Help' }));
    expect(screen.getByRole('button', { name: 'Open handbook' })).toHaveFocus();

    close.focus();
    await userEvent.tab();
    expect(screen.getByRole('button', { name: 'Open handbook' })).toHaveFocus();
    await userEvent.keyboard('{Escape}');
    expect(onClose).toHaveBeenCalledOnce();
  });
});
