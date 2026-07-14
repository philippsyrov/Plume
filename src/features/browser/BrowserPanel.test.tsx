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
    expect(screen.getByLabelText('Browser')).toHaveStyle('--plume-browser-split-width: 560px');
    expect(screen.getByLabelText('Task chat')).toHaveTextContent('Task conversation');
    expect(screen.getByRole('button', { name: 'Expand Browser' })).toBeInTheDocument();
    expect(screen.getByRole('separator', { name: 'Resize Browser and chat' })).toBeInTheDocument();
  });

  it('persists the task split width from keyboard resizing', async () => {
    const setSplitWidth = vi.fn().mockResolvedValue(true);
    mocks.browser = fixture({ setSplitWidth });
    render(<BrowserPanel identity={identity} chatPane={null} onUseInChat={vi.fn()} />);
    const separator = screen.getByRole('separator', { name: 'Resize Browser and chat' });
    fireEvent.keyDown(separator, { key: 'ArrowRight' });
    expect(setSplitWidth).toHaveBeenCalledWith(584);
  });

  it('clamps a large restored split width to keep chat visible', () => {
    const bounds = vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue({
      width: 900, height: 700, x: 0, y: 0, top: 0, right: 900, bottom: 700, left: 0,
      toJSON: () => ({}),
    });
    const workspace = { ...fixture().workspace!, splitWidthPx: 1_600 };
    mocks.browser = fixture({ workspace });
    render(<BrowserPanel identity={identity} chatPane={null} onUseInChat={vi.fn()} />);
    expect(screen.getByLabelText('Browser')).toHaveStyle('--plume-browser-split-width: 592px');
    bounds.mockRestore();
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

  it('asks for fresh exact-origin approval before returning to restored loopback history', async () => {
    const user = userEvent.setup();
    const back = vi.fn()
      .mockResolvedValueOnce({ kind: 'needsApproval', origin: 'http://localhost:5173' })
      .mockResolvedValueOnce({ kind: 'opened' });
    const tab = {
      ...fixture().activeTab!,
      currentHistoryIndex: 1,
      history: [
        { position: 0, url: 'http://localhost:5173/', recordedAtMs: 1 },
        { position: 1, url: 'https://example.com/', recordedAtMs: 2 },
      ],
    };
    mocks.browser = fixture({
      back,
      activeTab: tab,
      workspace: { ...fixture().workspace!, tabs: [tab] },
    });
    render(<BrowserPanel identity={identity} chatPane={null} onUseInChat={vi.fn()} />);

    await user.click(screen.getByRole('button', { name: 'Back' }));
    expect(screen.getByText('Open this local site?')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Open' }));
    expect(back).toHaveBeenLastCalledWith('http://localhost:5173');
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

  it('does not attach a delayed capture after its Browser unmounts', async () => {
    let finish!: (value: Awaited<ReturnType<TaskBrowserApi['captureScreenshot']>>) => void;
    const captureScreenshot = vi.fn().mockReturnValue(new Promise((resolve) => { finish = resolve; }));
    const onUseInChat = vi.fn().mockResolvedValue('added');
    mocks.browser = fixture({ captureScreenshot });
    const { unmount } = render(
      <BrowserPanel identity={identity} chatPane={null} onUseInChat={onUseInChat} />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Screenshot' }));
    unmount();
    finish({
      kind: 'captured',
      source: { kind: 'browserScreenshotEvidence', evidenceId: `bs_${'d'.repeat(32)}` },
      evidence: { evidenceId: `bs_${'d'.repeat(32)}`, sourceUrl: 'https://example.com/', title: null, capturedAtMs: 1, bytes: 12, sha256: 'ab'.repeat(32), width: 100, height: 100 },
    });
    await Promise.resolve();
    expect(onUseInChat).not.toHaveBeenCalled();
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
    back: vi.fn().mockResolvedValue({ kind: 'opened' }), forward: vi.fn().mockResolvedValue({ kind: 'opened' }), reload: vi.fn().mockResolvedValue(true),
    setGeometry: vi.fn().mockResolvedValue(undefined), setLayout: vi.fn().mockResolvedValue(true), setSplitWidth: vi.fn().mockResolvedValue(true), openTab: vi.fn().mockResolvedValue(true), closeTab: vi.fn().mockResolvedValue(true), selectTab: vi.fn().mockResolvedValue(true),
    captureText: vi.fn().mockResolvedValue({ kind: 'failed' }), captureScreenshot: vi.fn().mockResolvedValue({ kind: 'failed' }),
    ...overrides,
  };
}
