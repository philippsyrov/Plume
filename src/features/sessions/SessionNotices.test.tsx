// Phase 1B: a full store and a transient save failure need different copy.
// Telling a user at the cap that Plume "retries automatically" is false — the
// retry can never succeed — and it hides the one action that would fix it.

import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import { SessionNotices } from './SessionNotices';

describe('SessionNotices', () => {
  it('promises an automatic retry only when the failure is transient', () => {
    render(<SessionNotices notice={null} saveError="database is locked" />);

    expect(screen.getByRole('alert')).toHaveTextContent('retries automatically');
  });

  it('tells a user at the cap what to do instead of promising a retry', () => {
    render(<SessionNotices notice={null} saveError="this chat store is full." storageFull />);

    const alert = screen.getByRole('alert');
    expect(alert).not.toHaveTextContent('retries automatically');
    expect(alert).toHaveTextContent('nothing has been deleted');
  });

  it('names both halves of the recovery path, not deletion alone', () => {
    // Deletion is the only way to reclaim space, but it is also irreversible.
    // Now that export ships, telling the user to delete without telling them
    // they can keep a copy first points them at data loss.
    render(<SessionNotices notice={null} saveError="this chat store is full." storageFull />);

    const alert = screen.getByRole('alert');
    expect(alert).toHaveTextContent(/export/i);
    expect(alert).toHaveTextContent(/delete/i);
  });

  it('warns before writes stop, without raising an alert', () => {
    render(
      <SessionNotices notice={null} saveError={null} storageWarning="This chat store is nearly full." />,
    );

    expect(screen.getByRole('status')).toHaveTextContent('nearly full');
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });

  it('drops the approaching-cap warning once saving actually fails', () => {
    // Two messages about the same problem read as two problems.
    render(
      <SessionNotices
        notice={null}
        saveError="this chat store is full."
        storageFull
        storageWarning="This chat store is nearly full."
      />,
    );

    expect(screen.queryByText(/nearly full/)).not.toBeInTheDocument();
    expect(screen.getByRole('alert')).toBeInTheDocument();
  });
});
