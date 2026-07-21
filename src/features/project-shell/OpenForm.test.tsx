import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  onDragDropEvent: vi.fn().mockResolvedValue(vi.fn()),
  chooseProjectFolder: vi.fn(),
}));

vi.mock('@tauri-apps/api/webview', () => ({
  getCurrentWebview: () => ({ onDragDropEvent: mocks.onDragDropEvent }),
}));
vi.mock('../../lib/api/project', () => ({
  chooseProjectFolder: mocks.chooseProjectFolder,
}));

import { OpenForm } from './OpenForm';

describe('OpenForm consumer copy', () => {
  it('leads with native choice and Finder drop without path jargon', () => {
    render(
      <OpenForm
        path=""
        busy={false}
        onOpen={vi.fn()}
        onChange={vi.fn()}
        onChatOnly={vi.fn()}
      />,
    );

    expect(screen.getByRole('button', { name: 'Choose folder…' })).toBeInTheDocument();
    expect(screen.getByText('Drop a folder from Finder')).toBeInTheDocument();
    expect(screen.queryByRole('textbox', { name: 'Project folder' })).not.toBeInTheDocument();
    expect(screen.queryByText(/absolute path|file picker|plugin|later slice/i)).not.toBeInTheDocument();
  });

  it('opens a native selection and keeps manual entry disclosed', async () => {
    const user = userEvent.setup();
    const onOpen = vi.fn();
    mocks.chooseProjectFolder.mockResolvedValue('/Users/me/Code/plume');
    render(
      <OpenForm
        path=""
        busy={false}
        onOpen={onOpen}
        onChange={vi.fn()}
        onChatOnly={vi.fn()}
      />,
    );

    await user.click(screen.getByRole('button', { name: 'Choose folder…' }));
    expect(onOpen).toHaveBeenCalledWith('/Users/me/Code/plume');

    await user.click(screen.getByRole('button', { name: 'Enter path instead' }));
    expect(screen.getByRole('textbox', { name: 'Project folder' })).toBeVisible();
  });
});
