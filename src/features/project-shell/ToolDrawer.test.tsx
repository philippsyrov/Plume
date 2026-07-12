import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';

import { ToolDrawer } from './ToolDrawer';

function renderDrawer() {
  const callbacks = {
    onChat: vi.fn(),
    onFiles: vi.fn(),
    onBenchmarks: vi.fn(),
    onOpenProject: vi.fn(),
    onClose: vi.fn(),
  };

  render(
    <ToolDrawer
      hasProject
      activeView="project-chat"
      {...callbacks}
    />,
  );

  return callbacks;
}

describe('ToolDrawer', () => {
  it('names the drawer as workspace navigation', () => {
    renderDrawer();

    expect(screen.getByRole('complementary', { name: 'Workspace views' })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Workspace views' })).toBeInTheDocument();
    expect(screen.getByText('Choose where to work')).toBeInTheDocument();
    expect(screen.getByRole('navigation', { name: 'Workspace view picker' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Close workspace views' })).toBeInTheDocument();
  });

  it('keeps existing workspace item navigation', async () => {
    const callbacks = renderDrawer();
    const user = userEvent.setup();

    await user.click(screen.getByRole('button', { name: 'Files' }));
    await user.click(screen.getByRole('button', { name: 'Benchmarks' }));
    await user.click(screen.getByRole('button', { name: 'Project chat open' }));

    expect(callbacks.onFiles).toHaveBeenCalledOnce();
    expect(callbacks.onBenchmarks).toHaveBeenCalledOnce();
    expect(callbacks.onChat).toHaveBeenCalledOnce();
  });
});
