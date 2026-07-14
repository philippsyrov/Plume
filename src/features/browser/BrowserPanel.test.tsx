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

  it('keeps loopback-lookalike public hosts on HTTPS', async () => {
    const user = userEvent.setup();
    render(<BrowserPanel />);

    await user.type(
      screen.getByRole('textbox', { name: 'Web address' }),
      '127.0.0.1.example.com/path',
    );
    await user.click(screen.getByRole('button', { name: 'Go' }));

    expect(mocks.workspace?.open).toHaveBeenCalledWith(
      'https://127.0.0.1.example.com/path',
    );
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

  it('clears an old local approval when a later public navigation succeeds', async () => {
    const user = userEvent.setup();
    mocks.workspace = workspaceFixture({
      open: vi
        .fn()
        .mockResolvedValueOnce({ kind: 'needsApproval', origin: 'http://localhost:5173' })
        .mockResolvedValueOnce({ kind: 'opened' }),
    });
    render(<BrowserPanel />);
    const address = screen.getByRole('textbox', { name: 'Web address' });

    await user.type(address, 'localhost:5173');
    await user.click(screen.getByRole('button', { name: 'Go' }));
    expect(screen.getByText('Allow this local site?')).toBeInTheDocument();

    await user.clear(address);
    await user.type(address, 'example.com');
    await user.click(screen.getByRole('button', { name: 'Go' }));

    expect(screen.queryByText('Allow this local site?')).not.toBeInTheDocument();
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
    render(<BrowserPanel onUseInChat={vi.fn()} />);

    for (const name of ['Back', 'Forward', 'Reload', 'Show', 'Close']) {
      expect(screen.getByRole('button', { name })).toBeInTheDocument();
    }
    expect(screen.getByText('Opening example.com…')).toBeInTheDocument();
    expect(screen.getByText('Sandboxed window')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Use selection in chat' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Use page text in chat' })).toBeDisabled();
  });

  it('keeps capture simple and disabled without a trusted project chat', () => {
    render(<BrowserPanel />);
    expect(screen.queryByText(/agent/i)).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Use screenshot in chat' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Use selection in chat' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Use page text in chat' })).toBeDisabled();
    expect(screen.getByText('Open a trusted project to use page text in chat.')).toBeInTheDocument();
  });

  it('captures a selection and hands only its opaque record to project chat', async () => {
    const user = userEvent.setup();
    const onUseInChat = vi.fn().mockResolvedValue('added');
    mocks.workspace = workspaceFixture({
      state: {
        open: true,
        windowLabel: 'browser-sandbox',
        requestedUrl: 'https://example.com/',
        currentUrl: 'https://example.com/',
        title: 'Example',
        loading: false,
        failure: null,
      },
      captureText: vi.fn().mockResolvedValue({
        kind: 'captured',
        evidence: evidenceFixture('selection'),
      }),
    });
    render(<BrowserPanel onUseInChat={onUseInChat} />);

    await user.click(screen.getByRole('button', { name: 'Use selection in chat' }));

    expect(mocks.workspace?.captureText).toHaveBeenCalledWith('selection');
    expect(onUseInChat).toHaveBeenCalledWith({
      kind: 'browserTextEvidence',
      evidenceId: `be_${'a'.repeat(32)}`,
    });
    expect(screen.getByRole('status')).toHaveTextContent(
      'Added selection from example.com · 12 B · example text',
    );
  });

  it('captures a screenshot and hands only its opaque record to project chat', async () => {
    const user = userEvent.setup();
    const onUseInChat = vi.fn().mockResolvedValue('added');
    mocks.workspace = workspaceFixture({
      state: {
        open: true,
        windowLabel: 'browser-sandbox',
        requestedUrl: 'https://example.com/',
        currentUrl: 'https://example.com/',
        title: 'Example',
        loading: false,
        failure: null,
      },
      captureScreenshot: vi.fn().mockResolvedValue({
        kind: 'captured',
        evidence: {
          evidenceId: `bs_${'c'.repeat(32)}`,
          sourceUrl: 'https://example.com/',
          title: 'Example',
          capturedAtMs: 9,
          width: 800,
          height: 600,
          bytes: 1234,
          sha256: 'ab'.repeat(32),
        },
      }),
    });
    render(<BrowserPanel onUseInChat={onUseInChat} />);

    await user.click(screen.getByRole('button', { name: 'Use screenshot in chat' }));

    expect(onUseInChat).toHaveBeenCalledWith({
      kind: 'browserScreenshotEvidence',
      evidenceId: `bs_${'c'.repeat(32)}`,
    });
    expect(screen.getByRole('status')).toHaveTextContent('Added screenshot · 800×600 · 1.2 KB');
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
    captureText: vi.fn().mockResolvedValue({ kind: 'failed' }),
    captureScreenshot: vi.fn().mockResolvedValue({ kind: 'failed' }),
    close: vi.fn().mockResolvedValue(true),
    ...overrides,
  };
}

function evidenceFixture(captureKind: 'selection' | 'page') {
  return {
    evidenceId: `be_${'a'.repeat(32)}`,
    captureKind,
    sourceUrl: 'https://example.com/',
    title: 'Example',
    capturedAtMs: 7,
    bytes: 12,
    redactionCount: 0,
    truncated: false,
    preview: 'example text',
  } as const;
}
