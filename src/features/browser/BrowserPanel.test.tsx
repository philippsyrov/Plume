import { fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { TaskBrowserApi } from './useTaskBrowser';
import { BrowserPanel } from './BrowserPanel';

const identity = { scope: 'project' as const, sessionId: `s_${'a'.repeat(32)}` };
const mocks = vi.hoisted(() => ({ browser: null as TaskBrowserApi | null }));

vi.mock('./useTaskBrowser', async (importOriginal) => ({
  ...(await importOriginal<typeof import('./useTaskBrowser')>()),
  useTaskBrowser: () => mocks.browser,
}));

beforeEach(() => {
  vi.stubGlobal('ResizeObserver', class { observe() {} disconnect() {} });
  mocks.browser = fixture();
});

describe('BrowserPanel', () => {
  it('keeps Browser and its chat together in split view', () => {
    render(<BrowserPanel identity={identity} chatPane={<p>Task conversation</p>} onUseInChat={vi.fn()} />);
    expect(screen.getByLabelText('Browser')).toHaveClass('plume-browser-split');
    expect(screen.getByLabelText('Task chat')).toHaveTextContent('Task conversation');
    expect(screen.getByRole('button', { name: 'Expand Browser' })).toBeInTheDocument();
  });

  it('opens public addresses as HTTPS and exact loopback only after approval', async () => {
    const user = userEvent.setup();
    const navigate = vi.fn()
      .mockResolvedValueOnce({ kind: 'opened' })
      .mockResolvedValueOnce({ kind: 'needsApproval', origin: 'http://localhost:5173' })
      .mockResolvedValueOnce({ kind: 'opened' });
    mocks.browser = fixture({ navigate });
    render(<BrowserPanel identity={identity} chatPane={null} onUseInChat={vi.fn()} />);
    const address = screen.getByRole('textbox', { name: 'Web address' });
    const page = screen.getByLabelText('Web page').closest('.plume-browser-page');
    expect(page).not.toHaveClass('has-approval');

    await user.clear(address);
    await user.type(address, 'example.com/docs');
    await user.click(screen.getByRole('button', { name: 'Open address' }));
    expect(navigate).toHaveBeenCalledWith('https://example.com/docs');

    await user.clear(address);
    await user.type(address, 'localhost:5173');
    await user.click(screen.getByRole('button', { name: 'Open address' }));
    expect(screen.getByText('Open this local site?')).toBeInTheDocument();
    expect(page).toHaveClass('has-approval');
    await user.click(screen.getByRole('button', { name: 'Open' }));
    expect(navigate).toHaveBeenLastCalledWith('http://localhost:5173/', 'http://localhost:5173');
    expect(page).not.toHaveClass('has-approval');
  });

  it('adds the opaque captured source to the same chat', async () => {
    const user = userEvent.setup();
    const source = { kind: 'browserTextEvidence' as const, evidenceId: `be_${'c'.repeat(32)}` };
    const captureText = vi.fn().mockResolvedValue({
      kind: 'captured', source,
      evidence: { evidenceId: source.evidenceId, captureKind: 'selection', sourceUrl: 'https://example.com/', title: null, capturedAtMs: 1, bytes: 12, sha256: 'ab'.repeat(32), redactionCount: 0, truncated: false, preview: 'hello' },
    });
    const onUseInChat = vi.fn().mockResolvedValue('added');
    mocks.browser = fixture({ captureText });
    render(<BrowserPanel identity={identity} chatPane={null} onUseInChat={onUseInChat} />);

    await user.click(screen.getByRole('button', { name: 'Use selection' }));
    expect(onUseInChat).toHaveBeenCalledWith(source);
    expect(screen.getByText(/Added selection from example.com/)).toBeInTheDocument();
  });

  it('uses HTTP for bracketed IPv6 loopback', async () => {
    const user = userEvent.setup();
    render(<BrowserPanel identity={identity} chatPane={null} onUseInChat={vi.fn()} />);
    fireEvent.change(screen.getByRole('textbox', { name: 'Web address' }), { target: { value: '[::1]:57880' } });
    await user.click(screen.getByRole('button', { name: 'Open address' }));
    expect(mocks.browser?.navigate).toHaveBeenCalledWith('http://[::1]:57880/');
  });
});

function fixture(overrides: Partial<TaskBrowserApi> = {}): TaskBrowserApi {
  const tab = { id: `bt_${'b'.repeat(32)}`, position: 0, currentHistoryIndex: 0, manualReopenRequired: false, restorationStatus: 'restorable' as const, history: [{ position: 0, url: 'https://example.com/', recordedAtMs: 1 }] };
  return {
    workspace: { sessionId: identity.sessionId, scope: identity.scope, layoutMode: 'split', splitWidthPx: 560, activeTabId: tab.id, tabs: [tab], recovery: null },
    activeTab: tab,
    busy: false,
    errorMessage: null,
    navigate: vi.fn().mockResolvedValue({ kind: 'opened' }),
    back: vi.fn().mockResolvedValue(true), forward: vi.fn().mockResolvedValue(true), reload: vi.fn().mockResolvedValue(true),
    setGeometry: vi.fn().mockResolvedValue(undefined), setLayout: vi.fn().mockResolvedValue(true), openTab: vi.fn().mockResolvedValue(true), closeTab: vi.fn().mockResolvedValue(true), selectTab: vi.fn().mockResolvedValue(true),
    captureText: vi.fn().mockResolvedValue({ kind: 'failed' }), captureScreenshot: vi.fn().mockResolvedValue({ kind: 'failed' }),
    ...overrides,
  };
}
