// Preview/conflict honesty for the distillation disclosure: a group
// whose duplicates carry links the survivor lacks shows the resulting
// merged link set, while a group whose union would exceed the per-entry
// cap is shown as a conflict, is not selectable, and is excluded from
// the compact count. Presentational component, driven purely by props.

import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import type { MemoryDistillPreview, MemoryEntry } from '../../lib/api/memory';
import { DistillPreviewDisclosure, type DistillState } from './MemoryDistill';

function entry(id: string, text: string, links: string[] = []): MemoryEntry {
  return { id, createdMs: 1, text, redactionCount: 0, links };
}

// One compactable group whose survivor inherits links, and one blocked
// by an over-cap link conflict.
const preview: MemoryDistillPreview = {
  totalEntries: 4,
  // Only the compactable group contributes; the conflict group does not.
  wouldRemove: 1,
  duplicateGroups: [
    {
      id: 'dup_merge',
      removableCount: 1,
      mergedLinks: ['topics/a.md', 'topics/b.md'],
      linkCapExceeded: false,
      entries: [entry('m_b', 'keep me', ['topics/a.md']), entry('m_a', 'keep me', ['topics/b.md'])],
    },
    {
      id: 'dup_conflict',
      removableCount: 1,
      mergedLinks: [
        'topics/1.md',
        'topics/2.md',
        'topics/3.md',
        'topics/4.md',
        'topics/5.md',
        'topics/6.md',
      ],
      linkCapExceeded: true,
      entries: [entry('m_d', 'conflicting'), entry('m_c', 'conflicting')],
    },
  ],
};

function renderDisclosure() {
  const onApply = vi.fn();
  render(
    <DistillPreviewDisclosure
      expanded
      state={{ kind: 'ready', preview } satisfies DistillState}
      log={[]}
      applyBusy={false}
      notice={null}
      onToggle={vi.fn()}
      onRefresh={vi.fn()}
      onApply={onApply}
    />,
  );
  return { onApply };
}

describe('DistillPreviewDisclosure link merge + conflict honesty', () => {
  it('shows the survivor’s merged links for a compactable group', () => {
    renderDisclosure();
    expect(
      screen.getByText('Survivor keeps 2 topic links: topics/a.md, topics/b.md'),
    ).toBeInTheDocument();
  });

  it('surfaces an over-cap group as a conflict and excludes it from compaction', () => {
    const { onApply } = renderDisclosure();

    // Per-group conflict alert names the offending count and the cap.
    expect(
      screen.getByText(/Link conflict:.*6 topic links, over the 5-link limit/),
    ).toBeInTheDocument();
    // Summary line names how many groups are blocked.
    expect(
      screen.getByText(/1 group is blocked by a topic-link conflict/),
    ).toBeInTheDocument();

    // Exactly one group is selectable; the conflict group's checkbox is
    // disabled so it can never be sent to an apply.
    const checkboxes = screen.getAllByRole<HTMLInputElement>('checkbox');
    expect(checkboxes).toHaveLength(2);
    expect(checkboxes.filter((box) => box.disabled)).toHaveLength(1);

    // The compact button counts only the one compactable duplicate, and
    // an apply never carries the conflicted group id.
    const compact = screen.getByRole('button', { name: /Compact 1 duplicate/ });
    compact.click();
    expect(onApply).toHaveBeenCalledWith(['dup_merge']);
    expect(onApply).not.toHaveBeenCalledWith(expect.arrayContaining(['dup_conflict']));
  });
});
