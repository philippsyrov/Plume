import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import { ChatEntryRow } from './ChatEntryRow';

describe('ChatEntryRow', () => {
  it('uses quiet human role labels without changing message accessibility', () => {
    const { rerender } = render(
      <ChatEntryRow entry={{ kind: 'message', message: { role: 'user', content: 'Hello' } }} />,
    );
    expect(screen.getByLabelText('user message')).toHaveTextContent('You');

    rerender(
      <ChatEntryRow
        entry={{
          kind: 'message',
          message: { role: 'assistant', content: 'Hi' },
          modelUsed: 'Qwen Coder 1.5B',
          durationMs: 564,
        }}
      />,
    );
    expect(screen.getByLabelText('assistant message')).toHaveTextContent('Plume');
    expect(screen.getByText(/served by Qwen Coder 1.5B/)).toBeInTheDocument();
  });
});
