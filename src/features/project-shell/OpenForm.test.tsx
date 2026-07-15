import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  onDragDropEvent: vi.fn().mockResolvedValue(vi.fn()),
}));

vi.mock('@tauri-apps/api/webview', () => ({
  getCurrentWebview: () => ({ onDragDropEvent: mocks.onDragDropEvent }),
}));

import { OpenForm } from './OpenForm';

describe('OpenForm consumer copy', () => {
  it('explains folder opening without roadmap or absolute-path jargon', () => {
    render(
      <OpenForm
        path=""
        busy={false}
        onOpen={vi.fn()}
        onChange={vi.fn()}
        onChatOnly={vi.fn()}
      />,
    );

    expect(screen.getByText(/Paste a folder path below, or drag the folder/)).toBeInTheDocument();
    expect(screen.getByRole('textbox', { name: 'Project folder' })).toHaveAttribute(
      'placeholder',
      'Paste a folder path',
    );
    expect(screen.queryByText(/absolute path|file picker|plugin|later slice/i)).not.toBeInTheDocument();
  });
});
