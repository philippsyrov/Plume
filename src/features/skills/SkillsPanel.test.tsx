import { act, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { SkillsPanel } from './SkillsPanel';
import { applySkill, listSkills, loadSkill, previewSkill } from '../../lib/api/skills';

vi.mock('../../lib/api/skills', () => ({
  listSkills: vi.fn(),
  loadSkill: vi.fn(),
  previewSkill: vi.fn(),
  applySkill: vi.fn(),
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
