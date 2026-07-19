import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { expect, it, vi } from 'vitest';

import { ResearchProgress } from './ResearchProgress';

it('announces calm phase copy, keeps Stop visible, and hides counters in Details', async () => {
  const onStop = vi.fn();
  render(
    <ResearchProgress
      status="running"
      steps={[
        {
          phase: 'summarizing',
          summary: 'Reading source 1 of 2',
          current: 1,
          total: 2,
          logicalTurns: 1,
          providerCalls: 1,
          state: 'active',
        },
      ]}
      details={['Retrying once with the exact tool-call format']}
      error={null}
      onStop={onStop}
    />,
  );

  expect(screen.getByRole('status')).toHaveTextContent('Reading source 1 of 2');
  await userEvent.click(screen.getByRole('button', { name: 'Stop research' }));
  expect(onStop).toHaveBeenCalledOnce();
  expect(screen.queryByText(/1 model call/)).not.toBeInTheDocument();
  await userEvent.click(screen.getByText('Details'));
  expect(screen.getByText(/1 model call/)).toBeVisible();
  expect(screen.getByText(/Retrying once/)).toBeVisible();
});
