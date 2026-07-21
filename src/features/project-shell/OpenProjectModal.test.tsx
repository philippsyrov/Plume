import { act, fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  chooseProjectFolder: vi.fn(),
  onDragDropEvent: vi.fn(),
}));

vi.mock('../../lib/api/project', () => ({
  chooseProjectFolder: mocks.chooseProjectFolder,
}));

vi.mock('@tauri-apps/api/webview', () => ({
  getCurrentWebview: () => ({ onDragDropEvent: mocks.onDragDropEvent }),
}));

import { OpenProjectView } from './UnifiedChrome';

describe('OpenProjectView', () => {
  beforeEach(() => {
    mocks.chooseProjectFolder.mockReset();
    mocks.onDragDropEvent.mockReset();
    mocks.onDragDropEvent.mockResolvedValue(vi.fn());
  });

  it('renders project opening inline instead of as a dialog overlay', () => {
    render(<OpenProjectView onOpen={vi.fn()} onClose={vi.fn()} />);

    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(screen.getByRole('region', { name: 'Open a project' })).toBeVisible();
  });

  it('opens a chosen folder through the existing project-open callback', async () => {
    const user = userEvent.setup();
    const onOpen = vi.fn().mockResolvedValue(true);
    const onClose = vi.fn();
    mocks.chooseProjectFolder.mockResolvedValue('/Users/me/Code/plume');
    render(<OpenProjectView onOpen={onOpen} onClose={onClose} />);

    await user.click(screen.getByRole('button', { name: 'Choose folder…' }));

    expect(mocks.chooseProjectFolder).toHaveBeenCalledOnce();
    expect(onOpen).toHaveBeenCalledWith('/Users/me/Code/plume');
    expect(onClose).toHaveBeenCalledOnce();
  });

  it('keeps the inline view open when the native picker is cancelled', async () => {
    const user = userEvent.setup();
    const onOpen = vi.fn();
    const onClose = vi.fn();
    mocks.chooseProjectFolder.mockResolvedValue(null);
    render(<OpenProjectView onOpen={onOpen} onClose={onClose} />);

    await user.click(screen.getByRole('button', { name: 'Choose folder…' }));

    expect(onOpen).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });

  it('keeps manual path entry behind an explicit disclosure', async () => {
    const user = userEvent.setup();
    render(<OpenProjectView onOpen={vi.fn()} onClose={vi.fn()} />);

    expect(screen.queryByRole('textbox', { name: 'Project path' })).not.toBeInTheDocument();
    await user.click(screen.getByText('Enter path instead'));
    expect(screen.getByRole('textbox', { name: 'Project path' })).toBeVisible();
  });

  it('opens the first folder dropped from Finder', async () => {
    const onOpen = vi.fn().mockResolvedValue(true);
    const onClose = vi.fn();
    let listener: ((event: { payload: { type: string; paths: string[] } }) => void) | null = null;
    mocks.onDragDropEvent.mockImplementation(async (next: typeof listener) => {
      listener = next;
      return vi.fn();
    });
    render(<OpenProjectView onOpen={onOpen} onClose={onClose} />);
    await act(async () => undefined);

    await act(async () => {
      listener?.({ payload: { type: 'drop', paths: ['/Users/me/Code/plume'] } });
    });

    expect(onOpen).toHaveBeenCalledWith('/Users/me/Code/plume');
    expect(onClose).toHaveBeenCalledOnce();
  });

  it('ignores picker results after the inline view closes', async () => {
    const user = userEvent.setup();
    let finishPicker: ((path: string | null) => void) | null = null;
    mocks.chooseProjectFolder.mockReturnValue(
      new Promise<string | null>((resolve) => {
        finishPicker = resolve;
      }),
    );
    const onOpen = vi.fn();
    const { unmount } = render(<OpenProjectView onOpen={onOpen} onClose={vi.fn()} />);
    await user.click(screen.getByRole('button', { name: 'Choose folder…' }));
    unmount();

    await act(async () => {
      finishPicker?.('/Users/me/Code/stale');
    });

    expect(onOpen).not.toHaveBeenCalled();
  });

  it('does not accept another candidate while opening', async () => {
    const user = userEvent.setup();
    mocks.chooseProjectFolder.mockResolvedValue('/Users/me/Code/plume');
    render(<OpenProjectView onOpen={vi.fn()} onClose={vi.fn()} busy />);

    expect(screen.getByRole('button', { name: 'Choose folder…' })).toBeDisabled();
    await user.click(screen.getByText('Enter path instead'));
    expect(screen.getByRole('textbox', { name: 'Project path' })).toBeDisabled();
  });

  it('exposes the visible drop target as a keyboard-equivalent folder choice', () => {
    render(<OpenProjectView onOpen={vi.fn()} onClose={vi.fn()} />);

    const dropTarget = screen.getByText('Drop a folder from Finder');
    expect(dropTarget).toBeVisible();
    fireEvent.keyDown(dropTarget, { key: 'Enter' });
    expect(screen.getByRole('button', { name: 'Choose folder…' })).toBeEnabled();
  });
});
