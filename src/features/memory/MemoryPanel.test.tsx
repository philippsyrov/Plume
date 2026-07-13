import { act, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { MemoryPanel } from './MemoryPanel';
import type {
  MemoryDistillApplyResponse,
  MemoryDistillPreview,
  MemoryIndex,
} from '../../lib/api/memory';

// Mock the memory IPC surface. The panel imports these as plain
// functions, so a module mock with vi.hoisted spies is enough — no
// Tauri bridge needed.
const mocks = vi.hoisted(() => ({
  getMemoryIndex: vi.fn(),
  getMemoryDistillPreview: vi.fn(),
  getMemoryDistillLog: vi.fn(),
  getMemoryTopics: vi.fn(),
  setMemoryLinks: vi.fn(),
  applyMemoryDistill: vi.fn(),
  rememberMemory: vi.fn(),
  forgetMemory: vi.fn(),
  updateMemory: vi.fn(),
  searchMemory: vi.fn(),
}));

vi.mock('../../lib/api/memory', () => ({
  getMemoryIndex: mocks.getMemoryIndex,
  getMemoryDistillPreview: mocks.getMemoryDistillPreview,
  getMemoryDistillLog: mocks.getMemoryDistillLog,
  getMemoryTopics: mocks.getMemoryTopics,
  setMemoryLinks: mocks.setMemoryLinks,
  applyMemoryDistill: mocks.applyMemoryDistill,
  rememberMemory: mocks.rememberMemory,
  forgetMemory: mocks.forgetMemory,
  updateMemory: mocks.updateMemory,
  searchMemory: mocks.searchMemory,
  MEMORY_SEARCH_MAX_QUERY_BYTES: 256,
  MEMORY_SEARCH_MAX_LIMIT: 50,
}));

vi.mock('./memoryRevision', () => ({ bumpMemoryRevision: vi.fn() }));

function makeIndex(): MemoryIndex {
  const entry = (id: string, text: string) => ({
    id,
    createdMs: 1_700_000_000_000,
    text,
    redactionCount: 0,
    links: [],
  });
  return {
    entries: [
      entry('m_a0000000000000000000000000000000', 'same fact'),
      entry('m_b0000000000000000000000000000000', 'same fact'),
      entry('m_c0000000000000000000000000000000', 'other dup'),
      entry('m_d0000000000000000000000000000000', 'other dup'),
    ],
    limits: { maxEntries: 100, maxBytesPerEntry: 1024, maxBytesTotal: 65536 },
    totalBytes: 256,
  };
}

function makePreview(): MemoryDistillPreview {
  return {
    totalEntries: 4,
    wouldRemove: 2,
    duplicateGroups: [
      {
        id: 'dup_aaa_2',
        removableCount: 1,
        entries: [
          { id: 'm_b0000000000000000000000000000000', createdMs: 2, text: 'same fact', redactionCount: 0, links: [] },
          { id: 'm_a0000000000000000000000000000000', createdMs: 1, text: 'same fact', redactionCount: 0, links: [] },
        ],
      },
      {
        id: 'dup_bbb_2',
        removableCount: 1,
        entries: [
          { id: 'm_d0000000000000000000000000000000', createdMs: 4, text: 'other dup', redactionCount: 0, links: [] },
          { id: 'm_c0000000000000000000000000000000', createdMs: 3, text: 'other dup', redactionCount: 0, links: [] },
        ],
      },
    ],
  };
}

describe('MemoryPanel — D66 selective compact', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.getMemoryIndex.mockResolvedValue(makeIndex());
    mocks.getMemoryDistillPreview.mockResolvedValue(makePreview());
    mocks.applyMemoryDistill.mockResolvedValue({
      ok: true,
      removedEntryCount: 1,
      remainingEntryCount: 3,
      unmatchedGroupIds: [],
      auditLogged: true,
    } satisfies MemoryDistillApplyResponse);
    mocks.searchMemory.mockResolvedValue({ ok: true, hits: [], truncated: false, query: '' });
    mocks.updateMemory.mockResolvedValue({
      ok: true,
      entry: {
        id: 'm_a0000000000000000000000000000000',
        createdMs: 1,
        text: 'edited fact',
        redactionCount: 0,
        links: [],
      },
    });
    mocks.getMemoryDistillLog.mockResolvedValue([]);
    mocks.getMemoryTopics.mockResolvedValue({
      core: [
        { name: 'INDEX.md', kind: 'index', exists: false, bytes: 0, truncated: false, content: '' },
        { name: 'USER.md', kind: 'user', exists: false, bytes: 0, truncated: false, content: '' },
        { name: 'SOUL.md', kind: 'soul', exists: false, bytes: 0, truncated: false, content: '' },
      ],
      topics: [],
      topicsTruncated: false,
      limits: { maxCoreBytes: 2048, maxTopicBytes: 8192, maxTopics: 32 },
    });
  });

  it('applies only the checked duplicate groups', async () => {
    render(<MemoryPanel />);

    // Wait for the index to load.
    await screen.findByText(/4 of 100 entries/);

    // Open the "Find duplicates" disclosure; it fetches the preview.
    await userEvent.click(screen.getByRole('button', { name: 'Find duplicates' }));
    await screen.findByText(/2 duplicate groups/);

    // Both groups default to checked.
    const checkboxes = screen.getAllByRole<HTMLInputElement>('checkbox');
    expect(checkboxes).toHaveLength(2);
    expect(checkboxes[0].checked).toBe(true);
    expect(checkboxes[1].checked).toBe(true);

    // Uncheck the first group (dup_aaa_2). The Compact button label
    // drops to a single removable duplicate.
    await userEvent.click(checkboxes[0]);
    const compact = await screen.findByRole('button', { name: /Compact 1 duplicate$/ });

    await userEvent.click(compact);

    // Apply is called with ONLY the still-checked group id.
    expect(mocks.applyMemoryDistill).toHaveBeenCalledTimes(1);
    expect(mocks.applyMemoryDistill).toHaveBeenCalledWith(['dup_bbb_2']);
  });

  // D75 (review H1): the success notice must survive the post-apply
  // refetch — `fetchDistill` must not clear it.
  it('keeps the "Removed N" notice visible after a compaction', async () => {
    render(<MemoryPanel />);
    await screen.findByText(/4 of 100 entries/);
    await userEvent.click(screen.getByRole('button', { name: 'Find duplicates' }));
    await screen.findByText(/2 duplicate groups/);

    await userEvent.click(await screen.findByRole('button', { name: /Compact 2 duplicates/ }));

    // The mock reports removedEntryCount: 1; the confirmation must
    // still be on screen after refresh() + fetchDistill() resolve.
    expect(await screen.findByText('Removed 1 duplicate.')).toBeInTheDocument();
  });

  // D81 (Codex review): an unrecorded compaction is surfaced, not hidden.
  it('flags a compaction the audit log did not record', async () => {
    mocks.applyMemoryDistill.mockResolvedValue({
      ok: true,
      removedEntryCount: 1,
      remainingEntryCount: 3,
      unmatchedGroupIds: [],
      auditLogged: false,
    });

    render(<MemoryPanel />);
    await screen.findByText(/4 of 100 entries/);
    await userEvent.click(screen.getByRole('button', { name: 'Find duplicates' }));
    await screen.findByText(/2 duplicate groups/);
    await userEvent.click(await screen.findByRole('button', { name: /Compact 2 duplicates/ }));

    expect(
      await screen.findByText('Removed 1 duplicate. (not recorded in the audit log)'),
    ).toBeInTheDocument();
  });

  // D75 (review H2): a failure of the secondary audit-log read must not
  // sink the essential duplicate preview.
  it('still shows the preview when the audit-log read fails', async () => {
    mocks.getMemoryDistillLog.mockRejectedValue(new Error('corrupt distill-log.jsonl'));

    render(<MemoryPanel />);
    await screen.findByText(/4 of 100 entries/);
    await userEvent.click(screen.getByRole('button', { name: 'Find duplicates' }));

    // Preview groups render despite the log read rejecting.
    expect(await screen.findByText(/2 duplicate groups/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Compact 2 duplicates/ })).toBeInTheDocument();
  });

  it('renders the recent-compactions audit log when present', async () => {
    mocks.getMemoryDistillLog.mockResolvedValue([
      {
        tsMs: Date.now() - 5_000,
        rule: 'dedupeExact',
        removedIds: ['m_a0000000000000000000000000000000'],
        keptIds: ['m_b0000000000000000000000000000000'],
      },
    ]);

    render(<MemoryPanel />);
    await screen.findByText(/4 of 100 entries/);
    await userEvent.click(screen.getByRole('button', { name: 'Find duplicates' }));

    expect(await screen.findByText('Recent compactions')).toBeInTheDocument();
    expect(screen.getByText(/1 duplicate removed · just now/)).toBeInTheDocument();
  });

  it('lists topic files and reveals content on expand', async () => {
    mocks.getMemoryTopics.mockResolvedValue({
      core: [
        {
          name: 'INDEX.md',
          kind: 'index',
          exists: true,
          bytes: 12,
          truncated: false,
          content: '# Index here',
        },
        { name: 'USER.md', kind: 'user', exists: false, bytes: 0, truncated: false, content: '' },
        { name: 'SOUL.md', kind: 'soul', exists: false, bytes: 0, truncated: false, content: '' },
      ],
      topics: [],
      topicsTruncated: false,
      limits: { maxCoreBytes: 2048, maxTopicBytes: 8192, maxTopics: 32 },
    });

    render(<MemoryPanel />);
    await screen.findByText(/4 of 100 entries/);

    // Expand the "Topic files" disclosure; it fetches memory.topics.
    await userEvent.click(screen.getByRole('button', { name: 'Topic files' }));

    // The existing core file is an expandable row; the missing ones
    // render "not created".
    const indexRow = await screen.findByRole('button', { name: /INDEX\.md/ });
    expect(screen.getByText('SOUL.md')).toBeInTheDocument();

    // Content is hidden until the row is expanded.
    expect(screen.queryByText('# Index here')).not.toBeInTheDocument();
    await userEvent.click(indexRow);
    expect(screen.getByText('# Index here')).toBeInTheDocument();
  });

  it('disables Compact when no groups are selected', async () => {
    render(<MemoryPanel />);
    await screen.findByText(/4 of 100 entries/);
    await userEvent.click(screen.getByRole('button', { name: 'Find duplicates' }));
    await screen.findByText(/2 duplicate groups/);

    // "Clear all" deselects every group.
    await userEvent.click(screen.getByRole('button', { name: 'Clear all' }));

    const compact = screen.getByRole('button', { name: 'Select groups to compact' });
    expect(compact).toBeDisabled();
    expect(mocks.applyMemoryDistill).not.toHaveBeenCalled();
  });

  // D80: in-place edit.
  it('edits a memory entry in place', async () => {
    render(<MemoryPanel />);
    await screen.findByText(/4 of 100 entries/);

    // Enter edit mode on the first entry, change its text, save.
    await userEvent.click(screen.getAllByRole('button', { name: 'Edit' })[0]);
    const textarea = screen.getByLabelText('Edit memory entry');
    await userEvent.clear(textarea);
    await userEvent.type(textarea, 'edited fact');
    await userEvent.click(screen.getByRole('button', { name: 'Save' }));

    expect(mocks.updateMemory).toHaveBeenCalledWith(
      'm_a0000000000000000000000000000000',
      'edited fact',
    );
    // The row leaves edit mode on success (textarea gone).
    await waitFor(() =>
      expect(screen.queryByLabelText('Edit memory entry')).not.toBeInTheDocument(),
    );
  });
});

function topic(name: string) {
  return {
    name,
    kind: 'topic' as const,
    exists: true,
    bytes: 12,
    truncated: false,
    content: `# ${name}`,
  };
}

function topics(names: string[]) {
  return {
    core: [],
    topics: names.map(topic),
    topicsTruncated: false,
    limits: { maxCoreBytes: 2048, maxTopicBytes: 8192, maxTopics: 32 },
  };
}

describe('MemoryPanel — memory topic links', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.getMemoryIndex.mockResolvedValue(makeIndex());
    mocks.getMemoryTopics.mockResolvedValue(topics(['topics/alpha.md', 'topics/beta.md']));
    mocks.setMemoryLinks.mockImplementation(async (id: string, links: string[]) => ({
      ok: true,
      entry: { ...makeIndex().entries.find((entry) => entry.id === id)!, links },
    }));
    mocks.getMemoryDistillLog.mockResolvedValue([]);
    mocks.searchMemory.mockResolvedValue({ ok: true, hits: [], truncated: false, query: '' });
  });

  it('does not read topics until a Links action is used', async () => {
    render(<MemoryPanel />);
    await screen.findByText(/4 of 100 entries/);
    expect(mocks.getMemoryTopics).not.toHaveBeenCalled();

    await userEvent.click(screen.getAllByRole('button', { name: 'Links 0' })[0]);
    expect(mocks.getMemoryTopics).toHaveBeenCalledTimes(1);
    expect(await screen.findByRole('region', { name: 'Links 0' })).toBeInTheDocument();
  });

  it('loads current links checked and explains that links do not change chat context', async () => {
    const index = makeIndex();
    index.entries[0].links = ['topics/beta.md'];
    mocks.getMemoryIndex.mockResolvedValue(index);
    render(<MemoryPanel />);
    await userEvent.click((await screen.findAllByRole('button', { name: 'Links 1' }))[0]);

    expect(await screen.findByRole('checkbox', { name: 'topics/beta.md' })).toBeChecked();
    expect(screen.getByRole('checkbox', { name: 'topics/alpha.md' })).not.toBeChecked();
    expect(
      screen.getByText('Links organize memory only. Linked topic notes are not loaded into chat yet.'),
    ).toBeInTheDocument();
  });

  it('caps selection at five and saves the exact sorted selection', async () => {
    mocks.getMemoryTopics.mockResolvedValue(
      topics(['topics/z.md', 'topics/e.md', 'topics/d.md', 'topics/c.md', 'topics/b.md', 'topics/a.md']),
    );
    render(<MemoryPanel />);
    await userEvent.click((await screen.findAllByRole('button', { name: 'Links 0' }))[0]);
    for (const name of ['topics/z.md', 'topics/e.md', 'topics/d.md', 'topics/c.md', 'topics/b.md']) {
      await userEvent.click(await screen.findByRole('checkbox', { name }));
    }
    expect(screen.getByRole('checkbox', { name: 'topics/a.md' })).toBeDisabled();
    expect(screen.getByText('5 of 5 links')).toBeInTheDocument();

    await userEvent.click(screen.getByRole('button', { name: 'Save links' }));
    expect(mocks.setMemoryLinks).toHaveBeenCalledWith(
      'm_a0000000000000000000000000000000',
      ['topics/b.md', 'topics/c.md', 'topics/d.md', 'topics/e.md', 'topics/z.md'],
    );
  });

  it('clears all links with an explicit save', async () => {
    const index = makeIndex();
    index.entries[0].links = ['topics/alpha.md'];
    mocks.getMemoryIndex.mockResolvedValue(index);
    render(<MemoryPanel />);
    await userEvent.click((await screen.findAllByRole('button', { name: 'Links 1' }))[0]);
    await userEvent.click(await screen.findByRole('checkbox', { name: 'topics/alpha.md' }));
    await userEvent.click(screen.getByRole('button', { name: 'Save links' }));
    expect(mocks.setMemoryLinks).toHaveBeenCalledWith(
      'm_a0000000000000000000000000000000',
      [],
    );
  });

  it('surfaces stale linked topic names and lets the user clear them', async () => {
    const index = makeIndex();
    index.entries[0].links = ['topics/gone.md'];
    mocks.getMemoryIndex.mockResolvedValue(index);
    render(<MemoryPanel />);
    await userEvent.click((await screen.findAllByRole('button', { name: 'Links 1' }))[0]);
    const missing = await screen.findByRole('checkbox', { name: 'Missing topic: topics/gone.md' });
    expect(missing).toBeChecked();
    await userEvent.click(missing);
    expect(screen.queryByText('Missing topic: topics/gone.md')).not.toBeInTheDocument();
  });

  it('preserves the editor and selection when saving fails', async () => {
    mocks.setMemoryLinks.mockResolvedValue({ ok: false, reason: 'storeFailed', message: 'disk full' });
    render(<MemoryPanel />);
    await userEvent.click((await screen.findAllByRole('button', { name: 'Links 0' }))[0]);
    const alpha = await screen.findByRole('checkbox', { name: 'topics/alpha.md' });
    await userEvent.click(alpha);
    await userEvent.click(screen.getByRole('button', { name: 'Save links' }));

    expect(await screen.findByRole('alert')).toHaveTextContent('disk full');
    expect(screen.getByRole('region', { name: 'Links 0' })).toBeInTheDocument();
    expect(screen.getByRole('checkbox', { name: 'topics/alpha.md' })).toBeChecked();
  });

  it('ignores a stale topics result after switching entries', async () => {
    let resolveFirst!: (value: ReturnType<typeof topics>) => void;
    const first = new Promise<ReturnType<typeof topics>>((resolve) => {
      resolveFirst = resolve;
    });
    mocks.getMemoryTopics
      .mockReturnValueOnce(first)
      .mockResolvedValueOnce(topics(['topics/current.md']));
    render(<MemoryPanel />);
    const links = await screen.findAllByRole('button', { name: 'Links 0' });
    await userEvent.click(links[0]);
    await userEvent.click(links[1]);
    expect(await screen.findByRole('checkbox', { name: 'topics/current.md' })).toBeInTheDocument();

    await act(async () => resolveFirst(topics(['topics/stale.md'])));
    expect(screen.queryByRole('checkbox', { name: 'topics/stale.md' })).not.toBeInTheDocument();
    expect(screen.getByRole('checkbox', { name: 'topics/current.md' })).toBeInTheDocument();
  });

  it('reconciles a stale save result without replacing the newer editor', async () => {
    let resolveSave!: (value: {
      ok: true;
      entry: MemoryIndex['entries'][number];
    }) => void;
    const save = new Promise<{ ok: true; entry: MemoryIndex['entries'][number] }>((resolve) => {
      resolveSave = resolve;
    });
    mocks.setMemoryLinks.mockReturnValueOnce(save);
    render(<MemoryPanel />);
    const links = await screen.findAllByRole('button', { name: 'Links 0' });
    await userEvent.click(links[0]);
    await userEvent.click(await screen.findByRole('checkbox', { name: 'topics/alpha.md' }));
    await userEvent.click(screen.getByRole('button', { name: 'Save links' }));
    await userEvent.click(links[1]);
    expect(await screen.findByRole('region', { name: 'Links 0' })).toBeInTheDocument();

    await act(async () =>
      resolveSave({
        ok: true,
        entry: { ...makeIndex().entries[0], links: ['topics/alpha.md'] },
      }),
    );
    expect(screen.getByRole('region', { name: 'Links 0' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Links 1' })).toBeInTheDocument();
    expect(screen.queryByText('Memory links saved.')).not.toBeInTheDocument();
  });

  it('refreshes the row count and closes with a success notice', async () => {
    render(<MemoryPanel />);
    await userEvent.click((await screen.findAllByRole('button', { name: 'Links 0' }))[0]);
    await userEvent.click(await screen.findByRole('checkbox', { name: 'topics/alpha.md' }));
    await userEvent.click(screen.getByRole('button', { name: 'Save links' }));

    expect(await screen.findByRole('button', { name: 'Links 1' })).toBeInTheDocument();
    expect(screen.queryByRole('region', { name: 'Links 1' })).not.toBeInTheDocument();
    expect(screen.getByRole('status')).toHaveTextContent('Memory links saved.');
  });
});
