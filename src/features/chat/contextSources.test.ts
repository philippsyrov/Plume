import { describe, expect, it } from 'vitest';

import {
  MAX_CONTEXT_SOURCES,
  addContextSourceToList,
  removeContextSourceFromList,
} from './contextSources';
import type { ContextSourceRef } from '../../lib/api/chat';

describe('typed context source identity', () => {
  it('preserves first insertion, dedupes exact identity, and keeps ranges distinct', () => {
    const whole: ContextSourceRef = { kind: 'projectFile', relPath: 'src/main.rs' };
    const range: ContextSourceRef = {
      kind: 'projectFile',
      relPath: 'src/main.rs',
      startLine: 1,
      endLine: 3,
    };
    const first = addContextSourceToList([], whole);
    expect(first.result).toBe('added');
    expect(addContextSourceToList(first.sources, whole)).toEqual({
      result: 'duplicate',
      sources: first.sources,
    });
    expect(addContextSourceToList(first.sources, range).sources).toEqual([whole, range]);
  });

  it('caps distinct sources and removes only the exact identity', () => {
    const full = Array.from({ length: MAX_CONTEXT_SOURCES }, (_, index) => ({
      kind: 'topicFile' as const,
      name: `topics/${index}.md`,
    }));
    expect(
      addContextSourceToList(full, { kind: 'topicFile', name: 'topics/overflow.md' }).result,
    ).toBe('full');
    expect(removeContextSourceFromList(full, full[3])).toEqual([
      ...full.slice(0, 3),
      ...full.slice(4),
    ]);
  });
});
