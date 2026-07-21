import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';

import { ModalDialog } from './ModalDialog';

describe('ModalDialog', () => {
  it('captures and restores focus, traps Tab, and closes on Escape', async () => {
    const onClose = vi.fn();
    const view = render(<button type="button">Open settings</button>);
    const opener = screen.getByRole('button', { name: 'Open settings' });
    opener.focus();
    view.rerender(
      <>
        <button type="button">Open settings</button>
        <ModalDialog labelledBy="settings-title" onClose={onClose}>
          <h2 id="settings-title">Settings</h2>
          <button type="button">First setting</button>
          <button type="button">Last setting</button>
        </ModalDialog>
      </>,
    );

    await waitFor(() => expect(screen.getByRole('button', { name: 'First setting' })).toHaveFocus());
    screen.getByRole('button', { name: 'Last setting' }).focus();
    await userEvent.tab();
    expect(screen.getByRole('button', { name: 'First setting' })).toHaveFocus();
    await userEvent.tab({ shift: true });
    expect(screen.getByRole('button', { name: 'Last setting' })).toHaveFocus();
    await userEvent.keyboard('{Escape}');
    expect(onClose).toHaveBeenCalledOnce();

    view.rerender(<button type="button">Open settings</button>);
    expect(screen.getByRole('button', { name: 'Open settings' })).toHaveFocus();
  });

  it('treats a closed disclosure summary as focusable and ignores its hidden controls', async () => {
    render(
      <ModalDialog labelledBy="settings-title" onClose={vi.fn()}>
        <h2 id="settings-title">Settings</h2>
        <button type="button">First setting</button>
        <details>
          <summary>Advanced project tools</summary>
          <button type="button">Hidden advanced action</button>
        </details>
      </ModalDialog>,
    );

    const summary = screen.getByText('Advanced project tools');
    summary.focus();
    await userEvent.tab();
    expect(screen.getByRole('button', { name: 'First setting' })).toHaveFocus();
    await userEvent.tab({ shift: true });
    expect(summary).toHaveFocus();
  });

  it('ignores controls inside hidden settings pages when trapping focus', async () => {
    render(
      <ModalDialog labelledBy="settings-title" onClose={vi.fn()}>
        <h2 id="settings-title">Settings</h2>
        <button type="button">First setting</button>
        <button type="button">Last setting</button>
        <section hidden>
          <button type="button">Hidden setting</button>
        </section>
      </ModalDialog>,
    );

    screen.getByRole('button', { name: 'Last setting' }).focus();
    await userEvent.tab();
    expect(screen.getByRole('button', { name: 'First setting' })).toHaveFocus();
    expect(document.activeElement).not.toHaveTextContent('Hidden setting');
  });

  it('uses the shared modal shell classes without changing dialog semantics', () => {
    render(
      <ModalDialog labelledBy="modal-title" onClose={vi.fn()}>
        <h2 id="modal-title">Settings</h2>
        <button type="button">Close</button>
      </ModalDialog>,
    );

    const dialog = screen.getByRole('dialog', { name: 'Settings' });
    expect(dialog).toHaveClass('plume-project-settings-window');
    expect(dialog.parentElement).toHaveClass('plume-project-settings-backdrop');
  });
});
