import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { UserMemoryIndex } from '../../lib/api/memory';

const mocks = vi.hoisted(() => ({
  forgetUserMemory: vi.fn(),
  getUserMemoryIndex: vi.fn(),
  rememberUserMemory: vi.fn(),
  updateUserMemory: vi.fn(),
}));

vi.mock('../../lib/api/memory', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../lib/api/memory')>()),
  forgetUserMemory: mocks.forgetUserMemory,
  getUserMemoryIndex: mocks.getUserMemoryIndex,
  rememberUserMemory: mocks.rememberUserMemory,
  updateUserMemory: mocks.updateUserMemory,
}));

vi.mock('../memory/MemoryPanel', () => ({
  MemoryPanel: () => <section aria-label="Project memory controls">Project memory controls</section>,
}));

import { LibrarySettingsPanel } from './LibrarySettingsPanel';

const limits = { maxEntries: 100, maxBytesPerEntry: 1024, maxBytesTotal: 65_536 };

function index(text = 'Prefers concise answers'): UserMemoryIndex {
  return {
    entries: [{ id: 'm_user_one', createdMs: 1, text, redactionCount: 0 }],
    limits,
    totalBytes: text.length,
  };
}

beforeEach(() => {
  vi.resetAllMocks();
  mocks.getUserMemoryIndex.mockResolvedValue(index());
  mocks.rememberUserMemory.mockResolvedValue({
    ok: true,
    entry: { id: 'm_user_two', createdMs: 2, text: 'Likes diagrams', redactionCount: 0 },
  });
  mocks.updateUserMemory.mockResolvedValue({
    ok: true,
    entry: { id: 'm_user_one', createdMs: 1, text: 'Prefers examples', redactionCount: 0 },
  });
  mocks.forgetUserMemory.mockResolvedValue({ ok: true, removed: true });
});

describe('LibrarySettingsPanel', () => {
  it('keeps About you available without a project and explains the missing project scope', async () => {
    render(<LibrarySettingsPanel projectAvailable={false} />);

    expect(await screen.findByRole('heading', { name: 'About you' })).toBeInTheDocument();
    expect(screen.getByText('Prefers concise answers')).toBeInTheDocument();
    expect(screen.getByText("Stored on this Mac, separate from every project's memory."))
      .toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'This project' })).toBeInTheDocument();
    expect(screen.getByText('Open and trust a project to manage its memory.')).toBeInTheDocument();
    expect(screen.queryByRole('region', { name: 'Project memory controls' })).not.toBeInTheDocument();
  });

  it('offers plain user-memory create, edit, and remove controls with no links or distillation', async () => {
    const user = userEvent.setup();
    mocks.getUserMemoryIndex
      .mockResolvedValueOnce(index())
      .mockResolvedValueOnce(index('Likes diagrams'))
      .mockResolvedValueOnce(index('Prefers examples'))
      .mockResolvedValueOnce({ ...index(), entries: [] });
    render(<LibrarySettingsPanel projectAvailable />);

    await screen.findByText('Prefers concise answers');
    await user.type(screen.getByRole('textbox', { name: 'Add something about you' }), 'Likes diagrams');
    await user.click(screen.getByRole('button', { name: 'Remember about you' }));
    await waitFor(() => expect(mocks.rememberUserMemory).toHaveBeenCalledWith('Likes diagrams'));

    await user.click(screen.getByRole('button', { name: 'Edit Prefers concise answers' }));
    const editor = screen.getByRole('textbox', { name: 'Edit About you memory' });
    await user.clear(editor);
    await user.type(editor, 'Prefers examples');
    await user.click(screen.getByRole('button', { name: 'Save About you memory' }));
    await waitFor(() => expect(mocks.updateUserMemory).toHaveBeenCalledWith(
      'm_user_one',
      'Prefers examples',
    ));

    await user.click(screen.getByRole('button', { name: 'Remove Prefers examples' }));
    await waitFor(() => expect(mocks.forgetUserMemory).toHaveBeenCalledWith('m_user_one'));

    const aboutYou = screen.getByRole('heading', { name: 'About you' }).closest('section');
    expect(aboutYou).not.toBeNull();
    expect(within(aboutYou as HTMLElement).queryByText(/links|topics|distill|compact/i))
      .not.toBeInTheDocument();
    expect(screen.getByRole('region', { name: 'Project memory controls' })).toBeInTheDocument();
  });

  it('surfaces in-band mutation failures and keeps the current entry visible', async () => {
    const user = userEvent.setup();
    mocks.rememberUserMemory.mockResolvedValueOnce({
      ok: false,
      reason: 'capacityReached',
      message: 'Memory is full',
    });
    render(<LibrarySettingsPanel projectAvailable={false} />);

    await screen.findByText('Prefers concise answers');
    await user.type(screen.getByRole('textbox', { name: 'Add something about you' }), 'One more');
    await user.click(screen.getByRole('button', { name: 'Remember about you' }));

    expect(await screen.findByRole('alert')).toHaveTextContent('Memory is full');
    expect(screen.getByText('Prefers concise answers')).toBeInTheDocument();
  });

  it('keeps loaded and newly remembered user memory newest-first', async () => {
    mocks.getUserMemoryIndex.mockResolvedValue({
      ...index(),
      entries: [
        { id: 'm_old', createdMs: 1, text: 'Old preference', redactionCount: 0 },
        { id: 'm_middle', createdMs: 2, text: 'Middle preference', redactionCount: 0 },
      ],
    });
    mocks.rememberUserMemory.mockResolvedValue({
      ok: true,
      entry: { id: 'm_new', createdMs: 3, text: 'New preference', redactionCount: 0 },
    });
    const user = userEvent.setup();
    render(<LibrarySettingsPanel projectAvailable={false} />);
    await screen.findByText('Middle preference');

    expect(screen.getAllByRole('listitem').map((row) => row.textContent)).toEqual([
      expect.stringContaining('Middle preference'),
      expect.stringContaining('Old preference'),
    ]);

    await user.type(screen.getByRole('textbox', { name: 'Add something about you' }), 'New preference');
    await user.click(screen.getByRole('button', { name: 'Remember about you' }));
    await screen.findByText('New preference');
    expect(screen.getAllByRole('listitem').map((row) => row.textContent)).toEqual([
      expect.stringContaining('New preference'),
      expect.stringContaining('Middle preference'),
      expect.stringContaining('Old preference'),
    ]);
  });
});
