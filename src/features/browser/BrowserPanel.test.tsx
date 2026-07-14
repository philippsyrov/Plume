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
  it('places task chat before the right-hand Browser in split view', () => {
    render(<BrowserPanel identity={identity} chatPane={<p>Task conversation</p>} onUseInChat={vi.fn()} />);
    const root = screen.getByLabelText('Browser');
    const chat = screen.getByLabelText('Task chat');
    const page = screen.getByLabelText('Web page').closest('.plume-browser-page');

    expect(root).toHaveClass('plume-browser-split');
    expect(root).toHaveStyle('--plume-browser-split-width: 560px');
    expect(chat).toHaveTextContent('Task conversation');
    expect(chat.compareDocumentPosition(page!)).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
    expect(screen.getByRole('button', { name: 'Expand Browser' })).toBeInTheDocument();
    expect(screen.getByRole('separator', { name: 'Resize Browser and chat' })).toBeInTheDocument();
  });

  it('persists Browser width using right-hand divider keyboard geometry', () => {
    const setSplitWidth = vi.fn().mockResolvedValue(true);
    mocks.browser = fixture({ setSplitWidth });
    render(<BrowserPanel identity={identity} chatPane={null} onUseInChat={vi.fn()} />);
    const separator = screen.getByRole('separator', { name: 'Resize Browser and chat' });

    fireEvent.keyDown(separator, { key: 'ArrowLeft' });
    expect(setSplitWidth).toHaveBeenLastCalledWith(584);
    fireEvent.keyDown(separator, { key: 'ArrowRight' });
    expect(setSplitWidth).toHaveBeenLastCalledWith(536);
  });

  it('persists Browser width using right-hand divider pointer geometry', () => {
    const setSplitWidth = vi.fn().mockResolvedValue(true);
    const bounds = vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue({
      width: 1_000, height: 700, x: 0, y: 0, top: 0, right: 1_000, bottom: 700, left: 0,
      toJSON: () => ({}),
    });
    mocks.browser = fixture({ setSplitWidth });
    render(<BrowserPanel identity={identity} chatPane={null} onUseInChat={vi.fn()} />);

    const pointer = (type: string, clientX: number) => {
      const event = new Event(type, { bubbles: true });
      Object.defineProperty(event, 'clientX', { value: clientX });
      return event;
    };
    fireEvent(screen.getByRole('separator', { name: 'Resize Browser and chat' }), pointer('pointerdown', 560));
    fireEvent(window, pointer('pointermove', 520));
    fireEvent(window, pointer('pointerup', 520));

    expect(setSplitWidth).toHaveBeenCalledWith(600);
    bounds.mockRestore();
  });

  it('clamps a large restored split width to keep chat visible', () => {
    const bounds = vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue({
      width: 900, height: 700, x: 0, y: 0, top: 0, right: 900, bottom: 700, left: 0,
      toJSON: () => ({}),
    });
    const workspace = { ...fixture().workspace!, splitWidthPx: 1_600 };
    mocks.browser = fixture({ workspace });
    render(<BrowserPanel identity={identity} chatPane={null} onUseInChat={vi.fn()} />);
    expect(screen.getByLabelText('Browser')).toHaveStyle('--plume-browser-split-width: 532px');
    expect(screen.getByRole('separator', { name: 'Resize Browser and chat' })).toHaveAttribute(
      'aria-valuemax',
      '532',
    );
    bounds.mockRestore();
  });

  it('keeps the same task composer reachable when Browser expands and returns', async () => {
    const user = userEvent.setup();
    const setLayout = vi.fn().mockResolvedValue(true);
    mocks.browser = fixture({
      setLayout,
      workspace: { ...fixture().workspace!, layoutMode: 'expanded', splitWidthPx: 612 },
    });
    render(<BrowserPanel identity={identity} chatPane={<textarea aria-label="Task message" />} onUseInChat={vi.fn()} />);

    expect(screen.getByLabelText('Browser')).toHaveClass('plume-browser-expanded');
    expect(screen.getByRole('textbox', { name: 'Task message' })).toBeVisible();
    expect(screen.queryByRole('separator', { name: 'Resize Browser and chat' })).not.toBeInTheDocument();
    expect(screen.getByLabelText('Browser')).toHaveStyle('--plume-browser-split-width: 612px');

    await user.click(screen.getByRole('button', { name: 'Return to split view' }));
    expect(setLayout).toHaveBeenCalledWith('split');
  });

  it('uses shared SVG icons for ordinary Browser controls', () => {
    const second = {
      ...fixture().activeTab!,
      id: `bt_${'c'.repeat(32)}`,
      position: 1,
    };
    mocks.browser = fixture({
      workspace: { ...fixture().workspace!, tabs: [fixture().activeTab!, second] },
    });
    render(<BrowserPanel identity={identity} chatPane={null} onUseInChat={vi.fn()} />);

    for (const label of [
      'Back',
      'Forward',
      'Reload',
      'New browser tab',
      'Expand Browser',
    ]) {
      expect(screen.getByRole('button', { name: label }).querySelector('svg')).toBeInTheDocument();
    }
    expect(screen.getAllByRole('button', { name: 'Close example.com' })).toHaveLength(2);
    for (const close of screen.getAllByRole('button', { name: 'Close example.com' })) {
      expect(close.querySelector('svg')).toBeInTheDocument();
    }
    expect(screen.getByRole('button', { name: 'Back' })).not.toHaveTextContent('←');
    expect(screen.getByRole('button', { name: 'Reload' })).not.toHaveTextContent('↻');
  });

  it('keeps an in-progress address draft across same-tab polling objects', async () => {
    const user = userEvent.setup();
    const { rerender } = render(
      <BrowserPanel identity={identity} chatPane={null} onUseInChat={vi.fn()} />,
    );
    const address = screen.getByRole('textbox', { name: 'Web address' });

    await user.clear(address);
    await user.type(address, 'docs.example.com/draft');
    mocks.browser = fixture({
      activeTab: { ...fixture().activeTab! },
      workspace: { ...fixture().workspace!, tabs: [{ ...fixture().activeTab! }] },
    });
    rerender(<BrowserPanel identity={identity} chatPane={null} onUseInChat={vi.fn()} />);

    expect(address).toHaveValue('docs.example.com/draft');
  });

  it('returns an abandoned address draft to the live page after blur', async () => {
    const user = userEvent.setup();
    render(<BrowserPanel identity={identity} chatPane={null} onUseInChat={vi.fn()} />);
    const address = screen.getByRole('textbox', { name: 'Web address' });

    await user.clear(address);
    await user.type(address, 'unfinished.example');
    await user.tab();

    expect(address).toHaveValue('https://example.com/');
  });

  it('replaces an address draft when the active tab changes', async () => {
    const user = userEvent.setup();
    const { rerender } = render(
      <BrowserPanel identity={identity} chatPane={null} onUseInChat={vi.fn()} />,
    );
    const address = screen.getByRole('textbox', { name: 'Web address' });
    await user.clear(address);
    await user.type(address, 'unfinished.example');

    const nextTab = {
      ...fixture().activeTab!,
      id: `bt_${'e'.repeat(32)}`,
      history: [{ position: 0, url: 'https://next.example/', recordedAtMs: 2 }],
    };
    mocks.browser = fixture({
      activeTab: nextTab,
      workspace: { ...fixture().workspace!, activeTabId: nextTab.id, tabs: [nextTab] },
    });
    rerender(<BrowserPanel identity={identity} chatPane={null} onUseInChat={vi.fn()} />);

    expect(address).toHaveValue('https://next.example/');
  });

  it('opens one explicit Attach menu and restores focus when Escape closes it', async () => {
    const user = userEvent.setup();
    render(<BrowserPanel identity={identity} chatPane={null} onUseInChat={vi.fn()} />);
    const attach = screen.getByRole('button', { name: 'Attach page evidence' });

    expect(attach.closest('.plume-browser-toolbar')).not.toBeNull();
    expect(document.querySelector('.plume-browser-evidence')).not.toBeInTheDocument();
    expect(screen.queryByRole('status')).not.toBeInTheDocument();
    await user.click(attach);
    expect(attach).toHaveAttribute('aria-expanded', 'true');
    const menu = screen.getByRole('menu', { name: 'Attach page evidence' });
    expect(menu).toBeInTheDocument();
    expect(menu.closest('.plume-browser-toolbar')).toBeNull();
    expect(screen.getByLabelText('Web page').closest('.plume-browser-page')).toHaveClass(
      'has-attach-menu',
    );
    expect(screen.getByRole('menuitem', { name: 'Selected text' })).toHaveFocus();
    expect(screen.getByRole('menuitem', { name: 'Readable page text' })).toBeInTheDocument();
    expect(screen.getByRole('menuitem', { name: 'Visible screenshot' })).toBeInTheDocument();

    await user.keyboard('{ArrowUp}');
    expect(screen.getByRole('menuitem', { name: 'Visible screenshot' })).toHaveFocus();
    await user.keyboard('{ArrowDown}');
    expect(screen.getByRole('menuitem', { name: 'Selected text' })).toHaveFocus();

    await user.keyboard('{Escape}');
    expect(screen.queryByRole('menu', { name: 'Attach page evidence' })).not.toBeInTheDocument();
    expect(screen.getByLabelText('Web page').closest('.plume-browser-page')).not.toHaveClass(
      'has-attach-menu',
    );
    expect(attach).toHaveFocus();
  });

  it('keeps tab selection and close as separate semantic buttons', async () => {
    const user = userEvent.setup();
    const closeTab = vi.fn().mockResolvedValue(true);
    const selectTab = vi.fn().mockResolvedValue(true);
    const second = {
      ...fixture().activeTab!,
      id: `bt_${'f'.repeat(32)}`,
      position: 1,
      history: [{ position: 0, url: 'https://second.example/', recordedAtMs: 2 }],
    };
    mocks.browser = fixture({
      closeTab,
      selectTab,
      workspace: { ...fixture().workspace!, tabs: [fixture().activeTab!, second] },
    });
    render(<BrowserPanel identity={identity} chatPane={null} onUseInChat={vi.fn()} />);

    expect(screen.getByRole('tablist', { name: 'Browser tabs' })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'example.com' })).toHaveAttribute('aria-selected', 'true');
    const secondSelect = screen.getByRole('tab', { name: 'second.example' });
    expect(secondSelect).toHaveAttribute('aria-selected', 'false');
    expect(secondSelect.closest('.plume-browser-tab')).not.toHaveClass('is-active');
    await user.click(secondSelect);
    expect(selectTab).toHaveBeenCalledWith(second.id);

    const close = screen.getByRole('button', { name: 'Close second.example' });
    close.focus();
    await user.keyboard('{Enter}');
    expect(closeTab).toHaveBeenCalledWith(second.id);
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

    await user.click(screen.getByRole('button', { name: 'Attach page evidence' }));
    await user.click(screen.getByRole('menuitem', { name: 'Selected text' }));
    expect(onUseInChat).toHaveBeenCalledWith(source);
    expect(screen.getByText(/Added selection from example.com/)).toBeInTheDocument();
    expect(screen.getByRole('status')).toHaveTextContent(/Added selection from example.com/);
    expect(screen.getByRole('button', { name: 'Attach page evidence' })).toHaveFocus();
  });

  it('does not attach a delayed capture after its Browser unmounts', async () => {
    let finish!: (value: Awaited<ReturnType<TaskBrowserApi['captureScreenshot']>>) => void;
    const captureScreenshot = vi.fn().mockReturnValue(new Promise((resolve) => { finish = resolve; }));
    const onUseInChat = vi.fn().mockResolvedValue('added');
    mocks.browser = fixture({ captureScreenshot });
    const { unmount } = render(
      <BrowserPanel identity={identity} chatPane={null} onUseInChat={onUseInChat} />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Attach page evidence' }));
    fireEvent.click(screen.getByRole('menuitem', { name: 'Visible screenshot' }));
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
