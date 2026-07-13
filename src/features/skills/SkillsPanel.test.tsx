import { act, fireEvent, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { SkillsPanel } from './SkillsPanel';
import {
  applySkill,
  listSkills,
  loadSkill,
  loadSkillPromotionContext,
  previewSkill,
  previewSkillPromotion,
} from '../../lib/api/skills';
import { listSessions } from '../../lib/api/sessions';

vi.mock('../../lib/api/skills', () => ({
  listSkills: vi.fn(),
  loadSkill: vi.fn(),
  previewSkill: vi.fn(),
  applySkill: vi.fn(),
  previewSkillPromotion: vi.fn(),
  loadSkillPromotionContext: vi.fn(),
}));

vi.mock('../../lib/api/sessions', () => ({
  listSessions: vi.fn(),
}));

const skill = {
  slug: 'review-patch',
  name: 'Review patch',
  description: 'Check a proposed patch before applying it.',
};

const canonical = `---\nname: "Review patch"\ndescription: "Check a proposed patch before applying it."\n---\n\nRead the diff carefully.\n`;

describe('SkillsPanel', () => {
  beforeEach(() => {
    vi.mocked(listSkills).mockReset().mockResolvedValue({
      skills: [skill],
      invalid: [],
    });
    vi.mocked(loadSkill).mockReset().mockResolvedValue({
      ...skill,
      body: 'Read the diff carefully.',
      content: canonical,
    });
    vi.mocked(previewSkill).mockReset().mockResolvedValue({
      slug: 'new-skill',
      content: canonical,
      exists: false,
    });
    vi.mocked(applySkill).mockReset().mockResolvedValue({ ok: true, skill });
    vi.mocked(listSessions).mockReset().mockResolvedValue({
      sessions: [sessionSummary('s_one', 'Refactor notes')],
    });
    vi.mocked(loadSkillPromotionContext).mockReset().mockResolvedValue({
      sessionId: 's_one',
      title: 'Refactor notes',
      snapshotToken: 'snapshot-one',
      entries: [
        { index: 0, role: 'user', content: 'Extract the safe review steps.' },
        { index: 2, role: 'assistant', content: 'Check paths, then verify.' },
      ],
      excludedCount: 2,
    });
    vi.mocked(previewSkillPromotion).mockReset().mockResolvedValue({
      draft: {
        slug: 'refactor-review',
        name: 'Refactor review',
        description: 'Review a refactor safely.',
        body: 'Check paths, then verify.',
      },
      source: { sessionId: 's_one', title: 'Refactor notes', entryIndexes: [0, 2] },
      redactionCount: 0,
    });
  });

  it('loads metadata first and lazy-loads exact content only after selection', async () => {
    const user = userEvent.setup();
    render(<SkillsPanel />);

    expect(await screen.findByRole('button', { name: /Review patch/ })).toBeInTheDocument();
    expect(loadSkill).not.toHaveBeenCalled();

    await user.click(screen.getByRole('button', { name: /Review patch/ }));

    expect(loadSkill).toHaveBeenCalledWith('review-patch');
    const file = await screen.findByRole('region', { name: 'Review patch file' });
    expect(file.querySelector('pre')?.textContent).toBe(canonical);
  });

  it('ignores an older lazy load that resolves after the latest selection', async () => {
    const first = deferred<Awaited<ReturnType<typeof loadSkill>>>();
    const second = deferred<Awaited<ReturnType<typeof loadSkill>>>();
    vi.mocked(listSkills).mockResolvedValue({
      skills: [
        skill,
        { slug: 'write-tests', name: 'Write tests', description: 'Add focused tests.' },
      ],
      invalid: [],
    });
    vi.mocked(loadSkill).mockImplementation((slug) =>
      slug === 'review-patch' ? first.promise : second.promise,
    );
    const user = userEvent.setup();
    render(<SkillsPanel />);

    await user.click(await screen.findByRole('button', { name: /Review patch/ }));
    await user.click(screen.getByRole('button', { name: /Write tests/ }));
    await act(async () => {
      second.resolve({
        slug: 'write-tests',
        name: 'Write tests',
        description: 'Add focused tests.',
        body: 'Test the change.',
        content: 'second exact file',
      });
    });
    expect(screen.getByText('second exact file')).toBeInTheDocument();

    await act(async () => {
      first.resolve({ ...skill, body: 'Read the diff carefully.', content: canonical });
    });
    const file = screen.getByRole('region', { name: /file$/ });
    expect(file.querySelector('pre')?.textContent).toBe('second exact file');
  });

  it('keeps loading while the latest selection is pending', async () => {
    const first = deferred<Awaited<ReturnType<typeof loadSkill>>>();
    const second = deferred<Awaited<ReturnType<typeof loadSkill>>>();
    vi.mocked(listSkills).mockResolvedValue({
      skills: [
        skill,
        { slug: 'write-tests', name: 'Write tests', description: 'Add focused tests.' },
      ],
      invalid: [],
    });
    vi.mocked(loadSkill).mockImplementation((slug) =>
      slug === 'review-patch' ? first.promise : second.promise,
    );
    const user = userEvent.setup();
    render(<SkillsPanel />);

    await user.click(await screen.findByRole('button', { name: /Review patch/ }));
    await user.click(screen.getByRole('button', { name: /Write tests/ }));
    await act(async () => {
      first.resolve({ ...skill, body: 'Read the diff carefully.', content: canonical });
    });
    expect(screen.getByRole('status')).toHaveTextContent('Loading skill…');
    await act(async () => {
      second.resolve({
        slug: 'write-tests',
        name: 'Write tests',
        description: 'Add focused tests.',
        body: 'Test the change.',
        content: 'second exact file',
      });
    });
    expect(screen.getByText('second exact file')).toBeInTheDocument();
  });

  it('requires an exact preview before explicit apply', async () => {
    const user = userEvent.setup();
    render(<SkillsPanel />);
    await screen.findByRole('button', { name: /Review patch/ });

    await fillDraft(user);
    expect(screen.queryByRole('button', { name: 'Apply skill' })).not.toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Preview skill' }));

    const review = await screen.findByRole('region', { name: 'Skill preview' });
    expect(review.querySelector('pre')?.textContent).toBe(canonical);
    await user.click(within(review).getByRole('button', { name: 'Apply skill' }));

    expect(applySkill).toHaveBeenCalledWith({
      slug: 'new-skill',
      name: 'New skill',
      description: 'A safe local procedure.',
      body: 'Do the checked steps.',
    });
    expect(await screen.findByText('Skill saved.')).toBeInTheDocument();
    expect(listSkills).toHaveBeenCalledTimes(2);
    expect(screen.getByLabelText('Skill slug')).toHaveValue('');
  });

  it('invalidates and closes a preview when the draft changes', async () => {
    const user = userEvent.setup();
    render(<SkillsPanel />);
    await screen.findByRole('button', { name: /Review patch/ });
    await fillDraft(user);
    await user.click(screen.getByRole('button', { name: 'Preview skill' }));
    expect(await screen.findByRole('region', { name: 'Skill preview' })).toBeInTheDocument();

    await user.type(screen.getByLabelText('Skill instructions'), ' Changed.');

    expect(screen.queryByRole('region', { name: 'Skill preview' })).not.toBeInTheDocument();
  });

  it('keeps apply disabled when preview reports an existing skill', async () => {
    vi.mocked(previewSkill).mockResolvedValue({
      slug: 'new-skill',
      content: canonical,
      exists: true,
    });
    const user = userEvent.setup();
    render(<SkillsPanel />);
    await screen.findByRole('button', { name: /Review patch/ });
    await fillDraft(user);
    await user.click(screen.getByRole('button', { name: 'Preview skill' }));

    expect(await screen.findByText('already exists')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Apply skill' })).toBeDisabled();
  });

  it('preserves the draft and exact existing file when apply reports a conflict', async () => {
    vi.mocked(applySkill).mockResolvedValue({
      ok: false,
      reason: 'alreadyExists',
      message: 'The skill already exists.',
    });
    const user = userEvent.setup();
    render(<SkillsPanel />);
    await screen.findByRole('button', { name: /Review patch/ });
    await fillDraft(user);
    await user.click(screen.getByRole('button', { name: 'Preview skill' }));
    await user.click(await screen.findByRole('button', { name: 'Apply skill' }));

    expect(await screen.findByRole('alert')).toHaveTextContent('The skill already exists.');
    const existing = screen.getByRole('region', { name: 'Existing skill file' });
    expect(existing.querySelector('pre')?.textContent).toBe(canonical);
    expect(screen.getByLabelText('Skill slug')).toHaveValue('new-skill');
    expect(screen.getByLabelText('Skill instructions')).toHaveValue('Do the checked steps.');
  });

  it('shows invalid entries without pretending they can be loaded', async () => {
    vi.mocked(listSkills).mockResolvedValue({
      skills: [],
      invalid: [{ slug: 'broken-skill', reason: 'Missing frontmatter.' }],
    });
    render(<SkillsPanel />);

    expect(await screen.findByText('broken-skill')).toBeInTheDocument();
    expect(screen.getByText('Missing frontmatter.')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /broken-skill/ })).not.toBeInTheDocument();
  });

  it('surfaces list and lazy-load failures', async () => {
    vi.mocked(listSkills).mockRejectedValueOnce(new Error('list unavailable'));
    const first = render(<SkillsPanel />);
    expect(await screen.findByRole('alert')).toHaveTextContent('list unavailable');
    first.unmount();

    vi.mocked(listSkills).mockResolvedValue({ skills: [skill], invalid: [] });
    vi.mocked(loadSkill).mockRejectedValueOnce(new Error('read unavailable'));
    const user = userEvent.setup();
    render(<SkillsPanel />);
    await user.click(await screen.findByRole('button', { name: /Review patch/ }));
    expect(await screen.findByRole('alert')).toHaveTextContent('read unavailable');
  });

  it('shows metadata loading without starting a document read', () => {
    vi.mocked(listSkills).mockReturnValue(new Promise(() => {}));
    render(<SkillsPanel />);

    expect(screen.getByRole('status')).toHaveTextContent('Loading skills…');
    expect(loadSkill).not.toHaveBeenCalled();
  });

  it('states the safety boundary plainly', async () => {
    render(<SkillsPanel />);
    expect(
      screen.getByText(/project-local procedures.*grant no permissions.*run nothing/i),
    ).toBeInTheDocument();
  });

  it('loads project session metadata only when the chat disclosure opens', async () => {
    const user = userEvent.setup();
    render(<SkillsPanel />);

    expect(listSessions).not.toHaveBeenCalled();
    await user.click(screen.getByRole('button', { name: 'Start from project chat' }));

    expect(await screen.findByRole('option', { name: 'Refactor notes' })).toBeInTheDocument();
    expect(listSessions).toHaveBeenCalledWith({ scope: 'project', includeArchived: false });
    expect(loadSkillPromotionContext).not.toHaveBeenCalled();
  });

  it('lazy-loads the selected transcript and keeps original message indexes', async () => {
    const user = userEvent.setup();
    render(<SkillsPanel />);
    await openPromotion(user);
    await user.selectOptions(screen.getByLabelText('Source project chat'), 's_one');

    expect(loadSkillPromotionContext).toHaveBeenCalledWith('s_one');
    expect(await screen.findByRole('checkbox', { name: /User.*Extract the safe review steps/i })).toBeInTheDocument();
    expect(screen.getByRole('checkbox', { name: /Assistant.*Check paths, then verify/i })).toBeInTheDocument();
    expect(screen.getByText('2 cancelled or error entries are excluded.')).toBeInTheDocument();

    await user.click(screen.getByRole('checkbox', { name: /Assistant/i }));
    await user.click(screen.getByRole('checkbox', { name: /User/i }));
    await user.click(screen.getByRole('button', { name: 'Create editable draft' }));

    expect(previewSkillPromotion).toHaveBeenCalledWith({ sessionId: 's_one', snapshotToken: 'snapshot-one', entryIndexes: [0, 2] });
  });

  it('caps selection at twenty messages', async () => {
    vi.mocked(loadSkillPromotionContext).mockResolvedValue({
      sessionId: 's_one',
      title: 'Long chat',
      snapshotToken: 'snapshot-long',
      entries: Array.from({ length: 21 }, (_, index) => ({ index, role: 'user' as const, content: `Step ${index}` })),
      excludedCount: 0,
    });
    const user = userEvent.setup();
    render(<SkillsPanel />);
    await openPromotion(user);
    await user.selectOptions(screen.getByLabelText('Source project chat'), 's_one');
    const boxes = await screen.findAllByRole('checkbox');
    for (const checkbox of boxes.slice(0, 20)) await user.click(checkbox);

    expect(boxes[20]).toBeDisabled();
    expect(screen.getByText('20 of 20 selected')).toBeInTheDocument();
  });

  it('fills an editable draft without previewing or writing the skill', async () => {
    const user = userEvent.setup();
    render(<SkillsPanel />);
    await fillDraft(user);
    await user.click(screen.getByRole('button', { name: 'Preview skill' }));
    expect(await screen.findByRole('region', { name: 'Skill preview' })).toBeInTheDocument();
    await openPromotion(user);
    await user.selectOptions(screen.getByLabelText('Source project chat'), 's_one');
    await user.click(await screen.findByRole('checkbox', { name: /User/i }));
    await user.click(screen.getByRole('button', { name: 'Create editable draft' }));

    expect(await screen.findByText('Draft filled — review it, then preview the exact SKILL.md.')).toBeInTheDocument();
    expect(screen.getByLabelText('Skill slug')).toHaveValue('refactor-review');
    expect(screen.queryByRole('region', { name: 'Skill preview' })).not.toBeInTheDocument();
    expect(previewSkill).toHaveBeenCalledTimes(1);
    expect(applySkill).not.toHaveBeenCalled();
    expect(screen.getByText(/Refactor notes.*2 selected entries/i)).toBeInTheDocument();
  });

  it('reports redaction and preserves the current draft on promotion failure', async () => {
    vi.mocked(previewSkillPromotion)
      .mockResolvedValueOnce({
        draft: { slug: 'safe', name: 'Safe', description: 'Safe notes', body: '[REDACTED:api-key]' },
        source: { sessionId: 's_one', title: 'Refactor notes', entryIndexes: [0] },
        redactionCount: 1,
      })
      .mockRejectedValueOnce(new Error('promotion unavailable'));
    const user = userEvent.setup();
    render(<SkillsPanel />);
    await openPromotion(user);
    await user.selectOptions(screen.getByLabelText('Source project chat'), 's_one');
    await user.click(await screen.findByRole('checkbox', { name: /User/i }));
    await user.click(screen.getByRole('button', { name: 'Create editable draft' }));
    expect(await screen.findByText(/1 secret-like value was redacted/i)).toBeInTheDocument();

    await openPromotion(user);
    await user.selectOptions(screen.getByLabelText('Source project chat'), 's_one');
    await user.click(await screen.findByRole('checkbox', { name: /User/i }));
    await user.click(screen.getByRole('button', { name: 'Create editable draft' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('promotion unavailable');
    expect(screen.getByLabelText('Skill slug')).toHaveValue('safe');
  });

  it('ignores a stale transcript load after a newer chat is selected', async () => {
    const oldLoad = deferred<Awaited<ReturnType<typeof loadSkillPromotionContext>>>();
    const newLoad = deferred<Awaited<ReturnType<typeof loadSkillPromotionContext>>>();
    vi.mocked(listSessions).mockResolvedValue({
      sessions: [sessionSummary('old', 'Old chat'), sessionSummary('new', 'New chat')],
    });
    vi.mocked(loadSkillPromotionContext).mockImplementation((sessionId) => sessionId === 'old' ? oldLoad.promise : newLoad.promise);
    const user = userEvent.setup();
    render(<SkillsPanel />);
    await user.click(screen.getByRole('button', { name: 'Start from project chat' }));
    await screen.findByRole('option', { name: 'Old chat' });
    await user.selectOptions(screen.getByLabelText('Source project chat'), 'old');
    await user.selectOptions(screen.getByLabelText('Source project chat'), 'new');
    await act(async () => newLoad.resolve({ sessionId: 'new', title: 'New chat', snapshotToken: 'new-token', entries: [{ index: 4, role: 'user', content: 'New transcript' }], excludedCount: 0 }));
    expect(screen.getByRole('checkbox', { name: /New transcript/ })).toBeInTheDocument();
    await act(async () => oldLoad.resolve({ sessionId: 'old', title: 'Old chat', snapshotToken: 'old-token', entries: [{ index: 1, role: 'user', content: 'Old transcript' }], excludedCount: 0 }));
    expect(screen.queryByText('Old transcript')).not.toBeInTheDocument();
  });

  it('ignores a promotion result after the selector is cancelled', async () => {
    const promotion = deferred<Awaited<ReturnType<typeof previewSkillPromotion>>>();
    vi.mocked(previewSkillPromotion).mockReturnValue(promotion.promise);
    const user = userEvent.setup();
    render(<SkillsPanel />);
    await openPromotion(user);
    await user.selectOptions(screen.getByLabelText('Source project chat'), 's_one');
    await user.click(await screen.findByRole('checkbox', { name: /User/i }));
    await user.click(screen.getByRole('button', { name: 'Create editable draft' }));
    await user.click(screen.getByRole('button', { name: 'Start from project chat' }));
    await act(async () => promotion.resolve({
      draft: { slug: 'stale', name: 'Stale', description: 'Stale result', body: 'Ignore me' },
      source: { sessionId: 's_one', title: 'Refactor notes', entryIndexes: [0] },
      redactionCount: 0,
    }));

    expect(screen.getByLabelText('Skill slug')).toHaveValue('');
    expect(screen.queryByText(/Draft filled/)).not.toBeInTheDocument();
  });

  it('clears transcript loading when the source selection returns to blank', async () => {
    vi.mocked(loadSkillPromotionContext).mockReturnValue(new Promise(() => {}));
    const user = userEvent.setup();
    render(<SkillsPanel />);
    await openPromotion(user);
    await user.selectOptions(screen.getByLabelText('Source project chat'), 's_one');
    expect(screen.getByRole('status')).toHaveTextContent('Loading transcript…');

    await user.selectOptions(screen.getByLabelText('Source project chat'), '');

    expect(screen.queryByText('Loading transcript…')).not.toBeInTheDocument();
  });

  it('locks transcript selection while promotion is in flight', async () => {
    vi.mocked(previewSkillPromotion).mockReturnValue(new Promise(() => {}));
    const user = userEvent.setup();
    render(<SkillsPanel />);
    await openPromotion(user);
    await user.selectOptions(screen.getByLabelText('Source project chat'), 's_one');
    const userEntry = await screen.findByRole('checkbox', { name: /User/i });
    const assistantEntry = screen.getByRole('checkbox', { name: /Assistant/i });
    await user.click(userEntry);
    await user.click(screen.getByRole('button', { name: 'Create editable draft' }));

    expect(userEntry).toBeDisabled();
    expect(assistantEntry).toBeDisabled();
  });

  it('ignores an old preview that resolves after promotion starts', async () => {
    const oldPreview = deferred<Awaited<ReturnType<typeof previewSkill>>>();
    vi.mocked(previewSkill).mockReturnValue(oldPreview.promise);
    const user = userEvent.setup();
    render(<SkillsPanel />);
    await fillDraft(user);
    await openPromotion(user);
    await user.selectOptions(screen.getByLabelText('Source project chat'), 's_one');
    await user.click(await screen.findByRole('checkbox', { name: /User/i }));

    act(() => {
      fireEvent.click(screen.getByRole('button', { name: 'Preview skill' }));
      fireEvent.click(screen.getByRole('button', { name: 'Create editable draft' }));
    });
    expect(await screen.findByText('Draft filled — review it, then preview the exact SKILL.md.')).toBeInTheDocument();
    await act(async () => oldPreview.resolve({ slug: 'old', content: 'stale exact file', exists: false }));

    expect(screen.queryByText('stale exact file')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Apply skill' })).not.toBeInTheDocument();
  });

  it('locks picker controls during parent preview and apply requests', async () => {
    const previewRequest = deferred<Awaited<ReturnType<typeof previewSkill>>>();
    vi.mocked(previewSkill).mockReturnValueOnce(previewRequest.promise);
    const user = userEvent.setup();
    render(<SkillsPanel />);
    await fillDraft(user);
    await openPromotion(user);
    await user.selectOptions(screen.getByLabelText('Source project chat'), 's_one');
    const checkbox = await screen.findByRole('checkbox', { name: /User/i });
    await user.click(screen.getByRole('button', { name: 'Preview skill' }));
    expect(screen.getByLabelText('Source project chat')).toBeDisabled();
    expect(checkbox).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Create editable draft' })).toBeDisabled();
    await act(async () => previewRequest.resolve({ slug: 'new-skill', content: canonical, exists: false }));

    const applyRequest = deferred<Awaited<ReturnType<typeof applySkill>>>();
    vi.mocked(applySkill).mockReturnValueOnce(applyRequest.promise);
    await user.click(await screen.findByRole('button', { name: 'Apply skill' }));
    expect(screen.getByLabelText('Source project chat')).toBeDisabled();
    expect(checkbox).toBeDisabled();
    await act(async () => applyRequest.resolve({ ok: true, skill }));
  });
});

async function fillDraft(user: ReturnType<typeof userEvent.setup>) {
  await user.type(screen.getByLabelText('Skill slug'), 'new-skill');
  await user.type(screen.getByLabelText('Skill name'), 'New skill');
  await user.type(screen.getByLabelText('Skill description'), 'A safe local procedure.');
  await user.type(screen.getByLabelText('Skill instructions'), 'Do the checked steps.');
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

function sessionSummary(id: string, title: string) {
  return {
    id,
    title,
    createdAtMs: 1,
    updatedAtMs: 2,
    archivedAtMs: null,
    forkedFromSessionId: null,
    forkedThroughEntryId: null,
  };
}

async function openPromotion(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole('button', { name: 'Start from project chat' }));
  await screen.findByRole('option', { name: 'Refactor notes' });
}
