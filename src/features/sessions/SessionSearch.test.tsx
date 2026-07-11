// D66: search overlay behavior over mocked `sessions.search` IPC.
// The rules under test: one call per scope (never mixed), debounced
// input, stale responses dropped, literal-text queries only reach the
// backend when searchable, selection routing with scope, and the
// snippet marker contract.

import { act, fireEvent, render, renderHook, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { SessionSearchHit } from '../../lib/api/sessions';
import {
  SEARCH_DEBOUNCE_MS,
  SessionSearchOverlay,
  hasSearchableText,
  snippetParts,
  useSearchShortcut,
} from './SessionSearch';

const api = vi.hoisted(() => ({
  searchSessions: vi.fn(),
}));

vi.mock('../../lib/api/sessions', () => ({
  searchSessions: api.searchSessions,
  SEARCH_SNIPPET_START: '\uE000',
  SEARCH_SNIPPET_END: '\uE001',
  MAX_SEARCH_RESULTS: 20,
}));

function hit(id: string, title: string, overrides: Partial<SessionSearchHit> = {}): SessionSearchHit {
  return {
    id,
    title,
    updatedAtMs: Date.now(),
    archivedAtMs: null,
    matchKind: 'title',
    snippet: null,
    ...overrides,
  };
}

async function typeAndSettle(text: string) {
  fireEvent.change(screen.getByRole('combobox'), { target: { value: text } });
  await act(async () => {
    vi.advanceTimersByTime(SEARCH_DEBOUNCE_MS + 10);
  });
  // Let the mocked IPC promises resolve.
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
}

describe('SessionSearchOverlay', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers();
    api.searchSessions.mockResolvedValue({ hits: [] });
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it('queries each scope separately and renders separate sections', async () => {
    api.searchSessions.mockImplementation(({ scope }: { scope: string }) =>
      Promise.resolve({
        hits:
          scope === 'local'
            ? [hit('l1', 'local gradient chat')]
            : [hit('p1', 'project gradient chat')],
      }),
    );
    render(
      <SessionSearchOverlay
        projectAvailable
        notice={null}
        onSelect={vi.fn().mockResolvedValue(true)}
        onClose={vi.fn()}
      />,
    );
    await typeAndSettle('gradient');

    const scopes = api.searchSessions.mock.calls.map(([p]) => p.scope);
    expect(scopes.sort()).toEqual(['local', 'project']);
    for (const [payload] of api.searchSessions.mock.calls) {
      expect(payload.query).toBe('gradient');
    }
    expect(screen.getByText('Chats')).toBeInTheDocument();
    expect(screen.getByText('Project chats')).toBeInTheDocument();
    expect(screen.getByText('local gradient chat')).toBeInTheDocument();
    expect(screen.getByText('project gradient chat')).toBeInTheDocument();
  });

  it('without a project only the local database is queried', async () => {
    render(
      <SessionSearchOverlay
        projectAvailable={false}
        notice={null}
        onSelect={vi.fn().mockResolvedValue(true)}
        onClose={vi.fn()}
      />,
    );
    await typeAndSettle('anything');
    const scopes = api.searchSessions.mock.calls.map(([p]) => p.scope);
    expect(scopes).toEqual(['local']);
  });

  it('debounces: rapid keystrokes produce one query with the final text', async () => {
    render(
      <SessionSearchOverlay
        projectAvailable={false}
        notice={null}
        onSelect={vi.fn().mockResolvedValue(true)}
        onClose={vi.fn()}
      />,
    );
    const input = screen.getByRole('combobox');
    fireEvent.change(input, { target: { value: 'g' } });
    await act(async () => {
      vi.advanceTimersByTime(SEARCH_DEBOUNCE_MS / 2);
    });
    fireEvent.change(input, { target: { value: 'gr' } });
    await act(async () => {
      vi.advanceTimersByTime(SEARCH_DEBOUNCE_MS / 2);
    });
    fireEvent.change(input, { target: { value: 'gradient' } });
    await act(async () => {
      vi.advanceTimersByTime(SEARCH_DEBOUNCE_MS + 10);
    });
    await act(async () => {
      await Promise.resolve();
    });
    expect(api.searchSessions).toHaveBeenCalledTimes(1);
    expect(api.searchSessions.mock.calls[0]?.[0].query).toBe('gradient');
  });

  it('drops a stale response that resolves after a newer query', async () => {
    let releaseFirst: (v: { hits: SessionSearchHit[] }) => void = () => undefined;
    api.searchSessions
      .mockReturnValueOnce(
        new Promise((resolve) => {
          releaseFirst = resolve;
        }),
      )
      .mockResolvedValueOnce({ hits: [hit('h2', 'second query result')] });
    render(
      <SessionSearchOverlay
        projectAvailable={false}
        notice={null}
        onSelect={vi.fn().mockResolvedValue(true)}
        onClose={vi.fn()}
      />,
    );
    await typeAndSettle('first');
    await typeAndSettle('second');
    expect(screen.getByText('second query result')).toBeInTheDocument();

    // The first response arrives late — it must NOT replace the newer
    // results.
    await act(async () => {
      releaseFirst({ hits: [hit('h1', 'stale first result')] });
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(screen.queryByText('stale first result')).not.toBeInTheDocument();
    expect(screen.getByText('second query result')).toBeInTheDocument();
  });

  it('unsearchable queries never reach the backend', async () => {
    render(
      <SessionSearchOverlay
        projectAvailable={false}
        notice={null}
        onSelect={vi.fn().mockResolvedValue(true)}
        onClose={vi.fn()}
      />,
    );
    await typeAndSettle('*** ---');
    expect(api.searchSessions).not.toHaveBeenCalled();
    await typeAndSettle('   ');
    expect(api.searchSessions).not.toHaveBeenCalled();
  });

  it('selecting a hit routes scope and id; success closes the overlay', async () => {
    api.searchSessions.mockImplementation(({ scope }: { scope: string }) =>
      Promise.resolve({
        hits: scope === 'project' ? [hit('p1', 'the project chat')] : [],
      }),
    );
    const onSelect = vi.fn().mockResolvedValue(true);
    const onClose = vi.fn();
    render(
      <SessionSearchOverlay
        projectAvailable
        notice={null}
        onSelect={onSelect}
        onClose={onClose}
      />,
    );
    await typeAndSettle('project');
    fireEvent.mouseDown(screen.getByText('the project chat'));
    await act(async () => {
      await Promise.resolve();
    });
    expect(onSelect).toHaveBeenCalledWith('project', 'p1');
    expect(onClose).toHaveBeenCalled();
  });

  it('a refused selection keeps the overlay open and shows the notice', async () => {
    api.searchSessions.mockResolvedValue({ hits: [hit('l1', 'busy chat')] });
    const onSelect = vi.fn().mockResolvedValue(false);
    const onClose = vi.fn();
    render(
      <SessionSearchOverlay
        projectAvailable={false}
        notice="A reply is still streaming."
        onSelect={onSelect}
        onClose={onClose}
      />,
    );
    await typeAndSettle('busy');
    fireEvent.mouseDown(screen.getByText('busy chat'));
    await act(async () => {
      await Promise.resolve();
    });
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByRole('alert')).toHaveTextContent('A reply is still streaming.');
  });

  it('Escape closes; arrow keys move the active option and Enter selects it', async () => {
    api.searchSessions.mockResolvedValue({
      hits: [hit('l1', 'first row'), hit('l2', 'second row')],
    });
    const onSelect = vi.fn().mockResolvedValue(true);
    const onClose = vi.fn();
    render(
      <SessionSearchOverlay
        projectAvailable={false}
        notice={null}
        onSelect={onSelect}
        onClose={onClose}
      />,
    );
    await typeAndSettle('row');
    const input = screen.getByRole('combobox');
    fireEvent.keyDown(input, { key: 'ArrowDown' });
    fireEvent.keyDown(input, { key: 'Enter' });
    await act(async () => {
      await Promise.resolve();
    });
    expect(onSelect).toHaveBeenCalledWith('local', 'l2');

    fireEvent.keyDown(input, { key: 'Escape' });
    expect(onClose).toHaveBeenCalled();
  });

  it('renders snippet highlights from the markers and an archived badge', async () => {
    api.searchSessions.mockResolvedValue({
      hits: [
        hit('l1', 'old archived chat', {
          archivedAtMs: 123,
          matchKind: 'content',
          snippet: 'about \uE000gradient\uE001 clipping',
        }),
      ],
    });
    render(
      <SessionSearchOverlay
        projectAvailable={false}
        notice={null}
        onSelect={vi.fn().mockResolvedValue(true)}
        onClose={vi.fn()}
      />,
    );
    await typeAndSettle('gradient');
    const marked = screen.getByText('gradient');
    expect(marked.tagName).toBe('MARK');
    // Markers never render as text.
    expect(document.body.textContent).not.toContain('\uE000');
    expect(document.body.textContent).not.toContain('\uE001');
    expect(screen.getByText('archived')).toBeInTheDocument();
  });

  it('an IPC failure surfaces as a visible error', async () => {
    api.searchSessions.mockRejectedValue(new Error('index unavailable'));
    render(
      <SessionSearchOverlay
        projectAvailable={false}
        notice={null}
        onSelect={vi.fn().mockResolvedValue(true)}
        onClose={vi.fn()}
      />,
    );
    await typeAndSettle('anything');
    expect(screen.getByRole('alert')).toHaveTextContent('index unavailable');
  });
});

describe('useSearchShortcut', () => {
  it('fires on plain Cmd+K only', () => {
    const open = vi.fn();
    renderHook(() => useSearchShortcut(open));

    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'k', metaKey: true }));
    expect(open).toHaveBeenCalledTimes(1);

    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'k' }));
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'k', ctrlKey: true }));
    window.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'k', metaKey: true, shiftKey: true }),
    );
    expect(open).toHaveBeenCalledTimes(1);
  });
});

describe('snippet helpers', () => {
  it('splits marker-wrapped runs into highlighted parts', () => {
    expect(snippetParts('a \uE000b\uE001 c \uE000d\uE001')).toEqual([
      { text: 'a ', highlighted: false },
      { text: 'b', highlighted: true },
      { text: ' c ', highlighted: false },
      { text: 'd', highlighted: true },
    ]);
  });

  it('an unpaired marker degrades to plain text instead of losing it', () => {
    expect(snippetParts('a \uE000broken')).toEqual([
      { text: 'a ', highlighted: false },
      { text: 'broken', highlighted: false },
    ]);
  });

  it('hasSearchableText mirrors the backend rule', () => {
    expect(hasSearchableText('hello')).toBe(true);
    expect(hasSearchableText('héllo')).toBe(true);
    expect(hasSearchableText('42')).toBe(true);
    expect(hasSearchableText('*** ---')).toBe(false);
    expect(hasSearchableText('   ')).toBe(false);
  });
});
