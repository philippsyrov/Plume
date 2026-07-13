import { describe, expect, it } from 'vitest';

import type { ContextSourceRef } from '../../lib/api/chat';
import {
  PLUME_CONTEXT_MIME,
  readContextDrop,
  writeContextDrag,
} from './contextDragPayload';

function fakeTransfer(initial: Record<string, string> = {}): DataTransfer {
  const values = new Map(Object.entries(initial));
  return {
    effectAllowed: 'uninitialized',
    get types() {
      return [...values.keys()];
    },
    getData: (type: string) => values.get(type) ?? '',
    setData: (type: string, value: string) => {
      values.set(type, value);
    },
  } as unknown as DataTransfer;
}

function roundTrip(source: ContextSourceRef): ContextSourceRef | null {
  const dataTransfer = fakeTransfer();
  writeContextDrag(dataTransfer, source);
  return readContextDrop(dataTransfer);
}

describe('context drag payload', () => {
  it('round-trips each shipped opaque context reference without plain text', () => {
    const sources: ContextSourceRef[] = [
      { kind: 'projectFile', relPath: 'src/App.tsx' },
      { kind: 'projectFile', relPath: 'src/App.tsx', startLine: 4, endLine: 9 },
      { kind: 'memoryEntry', entryId: `m_${'a'.repeat(32)}` },
      { kind: 'topicFile', name: 'topics/product vision.md' },
    ];

    for (const source of sources) expect(roundTrip(source)).toEqual(source);

    const dataTransfer = fakeTransfer();
    writeContextDrag(dataTransfer, sources[0]!);
    expect(dataTransfer.types).toEqual([PLUME_CONTEXT_MIME]);
    expect(dataTransfer.getData('text/plain')).toBe('');
    expect(dataTransfer.effectAllowed).toBe('copy');
  });

  it('ignores foreign, malformed, and unknown payloads', () => {
    expect(readContextDrop(fakeTransfer({ 'text/plain': 'hello' }))).toBeNull();
    expect(readContextDrop(fakeTransfer({ [PLUME_CONTEXT_MIME]: '{' }))).toBeNull();
    expect(
      readContextDrop(
        fakeTransfer({ [PLUME_CONTEXT_MIME]: JSON.stringify({ kind: 'browserPage' }) }),
      ),
    ).toBeNull();
  });

  it('rejects invalid memory ids and non-canonical topics', () => {
    const invalid = [
      { kind: 'memoryEntry', entryId: 'm_short' },
      { kind: 'memoryEntry', entryId: `m_${'g'.repeat(32)}` },
      { kind: 'topicFile', name: 'INDEX.md' },
      { kind: 'topicFile', name: 'topics/.hidden.md' },
      { kind: 'topicFile', name: 'topics/nested/file.md' },
      { kind: 'topicFile', name: 'topics\\file.md' },
      { kind: 'topicFile', name: 'topics/.md' },
    ];

    for (const value of invalid) {
      expect(
        readContextDrop(
          fakeTransfer({ [PLUME_CONTEXT_MIME]: JSON.stringify(value) }),
        ),
      ).toBeNull();
    }
  });

  it('rejects unsafe project paths and incomplete or invalid line ranges', () => {
    const invalid = [
      { kind: 'projectFile', relPath: '' },
      { kind: 'projectFile', relPath: '/etc/passwd' },
      { kind: 'projectFile', relPath: '..\\secret' },
      { kind: 'projectFile', relPath: 'src/../secret' },
      { kind: 'projectFile', relPath: 'src/\0secret' },
      { kind: 'projectFile', relPath: 'x'.repeat(1025) },
      { kind: 'projectFile', relPath: 'src/App.tsx', startLine: 1 },
      { kind: 'projectFile', relPath: 'src/App.tsx', endLine: 2 },
      { kind: 'projectFile', relPath: 'src/App.tsx', startLine: 0, endLine: 2 },
      { kind: 'projectFile', relPath: 'src/App.tsx', startLine: 1.5, endLine: 2 },
      { kind: 'projectFile', relPath: 'src/App.tsx', startLine: 8, endLine: 4 },
    ];

    for (const value of invalid) {
      expect(
        readContextDrop(
          fakeTransfer({ [PLUME_CONTEXT_MIME]: JSON.stringify(value) }),
        ),
      ).toBeNull();
    }
  });
});
