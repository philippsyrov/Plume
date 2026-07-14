import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';

import { ToolDrawer } from './ToolDrawer';

function renderDrawer(hasProject = true) {
  const callbacks = {
    onChat: vi.fn(),
    onBrowser: vi.fn(),
    onFiles: vi.fn(),
    onKnowledge: vi.fn(),
    onBenchmarks: vi.fn(),
    onOpenProject: vi.fn(),
    onClose: vi.fn(),
  };

  render(
    <ToolDrawer
      hasProject={hasProject}
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

  it('routes Knowledge only through the Knowledge callback', async () => {
    const callbacks = renderDrawer();
    const user = userEvent.setup();

    await user.click(screen.getByRole('button', { name: 'Knowledge' }));

    expect(callbacks.onKnowledge).toHaveBeenCalledOnce();
    expect(callbacks.onFiles).not.toHaveBeenCalled();
    expect(callbacks.onBenchmarks).not.toHaveBeenCalled();
    expect(callbacks.onChat).not.toHaveBeenCalled();
    expect(callbacks.onOpenProject).not.toHaveBeenCalled();
    expect(callbacks.onClose).not.toHaveBeenCalled();
  });

  it('opens Browser with or without a project', async () => {
    const projectCallbacks = renderDrawer(true);
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'Browser' }));
    expect(projectCallbacks.onBrowser).toHaveBeenCalledOnce();

    document.body.replaceChildren();
    const localCallbacks = renderDrawer(false);
    await user.click(screen.getByRole('button', { name: 'Browser' }));
    expect(localCallbacks.onBrowser).toHaveBeenCalledOnce();
    expect(localCallbacks.onOpenProject).not.toHaveBeenCalled();
  });
});
