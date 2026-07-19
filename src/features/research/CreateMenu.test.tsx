import { fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { expect, it, vi } from 'vitest';

import { CreateMenu } from './CreateMenu';

it('supports menu keyboard navigation, dismiss, and trigger focus restoration', async () => {
  const onResearchNote = vi.fn();
  render(<CreateMenu disabledReason={null} onResearchNote={onResearchNote} />);
  const trigger = screen.getByRole('button', { name: 'Create' });

  await userEvent.click(trigger);
  const item = screen.getByRole('menuitem', { name: 'Research note' });
  expect(item).toHaveFocus();
  await userEvent.keyboard('{End}{Home}{ArrowDown}{ArrowUp}');
  expect(item).toHaveFocus();
  await userEvent.keyboard('{Escape}');
  expect(screen.queryByRole('menu')).not.toBeInTheDocument();
  expect(trigger).toHaveFocus();

  await userEvent.click(trigger);
  fireEvent.mouseDown(document.body);
  expect(screen.queryByRole('menu')).not.toBeInTheDocument();
  expect(trigger).toHaveFocus();

  await userEvent.click(trigger);
  await userEvent.click(screen.getByRole('menuitem', { name: 'Research note' }));
  expect(onResearchNote).toHaveBeenCalledOnce();
  expect(trigger).toHaveFocus();
});

it('keeps the stable action visible while explaining why research is unavailable', async () => {
  render(
    <CreateMenu
      disabledReason="Attach captured page text first."
      onResearchNote={vi.fn()}
    />,
  );
  await userEvent.click(screen.getByRole('button', { name: 'Create' }));

  expect(screen.getByRole('menuitem', { name: 'Research note' })).toBeDisabled();
  expect(screen.getByText('Attach captured page text first.')).toBeVisible();
});
