import { describe, expect, it } from 'vitest';

import {
  attachmentLabelOf,
  historicalRunNote,
  runStatusLabel,
  truncatePrompt,
  type RunRecord,
} from './runHistory';

function record(over: Partial<RunRecord> = {}): RunRecord {
  return {
    id: '1',
    prompt: 'do it',
    attachmentLabel: null,
    events: [],
    applicableDiff: null,
    applyState: 'idle',
    revertState: 'idle',
    checkpoint: null,
    ...over,
  };
}

describe('runStatusLabel', () => {
  it('shadows in priority: reverted > applied > apply failed > diff > no diff', () => {
    expect(runStatusLabel(record({ revertState: 'reverted', applyState: 'applied' }))).toBe(
      'reverted',
    );
    expect(runStatusLabel(record({ applyState: 'applied', applicableDiff: 'x' }))).toBe('applied');
    expect(runStatusLabel(record({ applyState: 'failed', applicableDiff: 'x' }))).toBe(
      'apply failed',
    );
    expect(runStatusLabel(record({ applicableDiff: 'x' }))).toBe('diff ready');
    expect(runStatusLabel(record({ events: [{} as never] }))).toBe('no diff');
    expect(runStatusLabel(record())).toBe('—');
  });
});

describe('historicalRunNote', () => {
  it('summarizes the frozen outcome and never offers to apply', () => {
    expect(historicalRunNote(record({ revertState: 'reverted', applyState: 'applied' }))).toMatch(
      /reverted/,
    );
    expect(historicalRunNote(record({ applyState: 'applied', checkpoint: 'abcd1234ef' }))).toContain(
      'checkpoint abcd1234',
    );
    expect(historicalRunNote(record({ applyState: 'failed' }))).toMatch(/Apply failed/);
    expect(historicalRunNote(record())).toMatch(/Not applied/);
  });
});

describe('attachmentLabelOf', () => {
  it('is null without a chip', () => {
    expect(attachmentLabelOf(null)).toBeNull();
  });
  it('is the bare path without a line range', () => {
    expect(attachmentLabelOf({ relPath: 'src/a.ts', lineRange: null })).toBe('src/a.ts');
  });
  it('appends the line range when present', () => {
    expect(
      attachmentLabelOf({ relPath: 'src/a.ts', lineRange: { startLine: 2, endLine: 7 } }),
    ).toBe('src/a.ts:2-7');
  });
});

describe('truncatePrompt', () => {
  it('keeps short prompts intact', () => {
    expect(truncatePrompt('short one')).toBe('short one');
  });
  it('ellipsizes long prompts to the cap', () => {
    const out = truncatePrompt('x'.repeat(50), 10);
    expect(out).toHaveLength(10);
    expect(out.endsWith('…')).toBe(true);
  });
});
