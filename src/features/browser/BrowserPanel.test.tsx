import { fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { BrowserWorkspace } from './useBrowserWorkspace';
import { BrowserPanel } from './BrowserPanel';

const mocks = vi.hoisted(() => ({ workspace: null as BrowserWorkspace | null }));

vi.mock('./useBrowserWorkspace', () => ({
  useBrowserWorkspace: () => mocks.workspace,
}));

describe('BrowserPanel', () => {
  beforeEach(() => {
    mocks.workspace = workspaceFixture();
  });

  it('opens a plain public address as HTTPS', async () => {
    const user = userEvent.setup();
    render(<BrowserPanel />);

    await user.type(screen.getByRole('textbox', { name: 'Web address' }), 'example.com/docs');
    await user.click(screen.getByRole('button', { name: 'Go' }));

    expect(mocks.workspace?.open).toHaveBeenCalledWith('https://example.com/docs');
  });

  it('defaults bracketed IPv6 loopback to HTTP', async () => {
    const user = userEvent.setup();
    render(<BrowserPanel />);

    fireEvent.change(screen.getByRole('textbox', { name: 'Web address' }), {
      target: { value: '[::1]:57880' },
    });
    await user.click(screen.getByRole('button', { name: 'Go' }));

    expect(mocks.workspace?.open).toHaveBeenCalledWith('http://[::1]:57880/');
  });

  it('asks once before opening the exact local origin and supports cancel', async () => {
    const user = userEvent.setup();
    mocks.workspace = workspaceFixture({
      open: vi
        .fn()
        .mockResolvedValueOnce({ kind: 'needsApproval', origin: 'http://localhost:5173' })
        .mockResolvedValueOnce({ kind: 'needsApproval', origin: 'http://localhost:5173' })
        .mockResolvedValueOnce({ kind: 'opened' }),
    });
    const view = render(<BrowserPanel />);

    await user.type(screen.getByRole('textbox', { name: 'Web address' }), 'localhost:5173');
    await user.click(screen.getByRole('button', { name: 'Go' }));
    expect(screen.getByText('Allow this local site?')).toBeInTheDocument();
    expect(screen.getByText('http://localhost:5173')).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(screen.queryByText('Allow this local site?')).not.toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Go' }));
    await user.click(screen.getByRole('button', { name: 'Open local site' }));
    expect(mocks.workspace?.open).toHaveBeenLastCalledWith(
      'http://localhost:5173/',
      'http://localhost:5173',
    );
    view.unmount();
  });

  it('shows fixed human controls and quiet browser state', () => {
    mocks.workspace = workspaceFixture({
      state: {
        open: true,
        windowLabel: 'browser-sandbox',
        requestedUrl: 'https://example.com/',
        currentUrl: 'https://example.com/path',
        title: null,
        loading: true,
        failure: null,
      },
    });
    render(<BrowserPanel />);

    for (const name of ['Back', 'Forward', 'Reload', 'Show', 'Close']) {
      expect(screen.getByRole('button', { name })).toBeInTheDocument();
    }
    expect(screen.getByText('Opening example.com…')).toBeInTheDocument();
    expect(screen.getByText('Sandboxed window')).toBeInTheDocument();
  });

  it('contains no agent controls or evidence controls in this slice', () => {
    render(<BrowserPanel />);
    expect(screen.queryByText(/agent/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/screenshot|attach|evidence|excerpt/i)).not.toBeInTheDocument();
  });

  it('shows a short error without exposing backend details', () => {
    mocks.workspace = workspaceFixture({ errorMessage: 'Browser unavailable. Try again.' });
    render(<BrowserPanel />);
    expect(screen.getByRole('status')).toHaveTextContent('Browser unavailable. Try again.');
  });
});

function workspaceFixture(overrides: Partial<BrowserWorkspace> = {}): BrowserWorkspace {
  return {
    state: {
      open: false,
      windowLabel: null,
      requestedUrl: null,
      currentUrl: null,
      title: null,
      loading: false,
      failure: null,
    },
    initialLoading: false,
    busy: false,
    errorMessage: null,
    refresh: vi.fn(),
    open: vi.fn().mockResolvedValue({ kind: 'opened' }),
    focus: vi.fn().mockResolvedValue(true),
    back: vi.fn().mockResolvedValue(true),
    forward: vi.fn().mockResolvedValue(true),
    reload: vi.fn().mockResolvedValue(true),
    close: vi.fn().mockResolvedValue(true),
    ...overrides,
  };
}
