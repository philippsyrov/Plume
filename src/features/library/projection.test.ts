import { describe, expect, it } from 'vitest';

import type { MemoryEntry, MemoryIndex, MemoryTopicFile, MemoryTopics } from '../../lib/api/memory';
import { buildLibraryProjection } from './projection';

const limits = { maxEntries: 100, maxBytesPerEntry: 1024, maxBytesTotal: 65_536 };
const topicLimits = { maxCoreBytes: 2048, maxTopicBytes: 8192, maxTopics: 32 };

function entry(id: string, links: string[]): MemoryEntry {
  return { id, createdMs: 1, text: id, redactionCount: 0, links, revision: 0 };
}

function file(name: string, exists = true): MemoryTopicFile {
  return { name, kind: 'topic', exists, bytes: 0, truncated: false, content: '' };
}

function index(entries: MemoryEntry[]): MemoryIndex {
  return { entries, limits, totalBytes: 0 };
}

function topics(files: MemoryTopicFile[], topicsTruncated = false): MemoryTopics {
  return { core: [], topics: files, topicsTruncated, limits: topicLimits };
}

describe('buildLibraryProjection', () => {
  it('keeps backlinks exact and noncanonical or absent refs stale', () => {
    const projection = buildLibraryProjection(
      index([
        entry('m_exact', ['topics/alpha.md']),
        entry('m_basename', ['alpha.md']),
        entry('m_missing', ['topics/missing.md']),
      ]),
      topics([file('topics/alpha.md')]),
    );

    expect(projection.topics[0]?.backlinks.map(({ entry }) => entry.id)).toEqual(['m_exact']);
    expect(projection.staleLinked.map(({ entry }) => entry.id)).toEqual([
      'm_basename',
      'm_missing',
    ]);
  });

  it('does not call a capped-out canonical ref stale when topic coverage is partial', () => {
    const projection = buildLibraryProjection(
      index([entry('m_capped', ['topics/zeta.md'])]),
      topics([file('topics/alpha.md')], true),
    );

    expect(projection.staleLinked).toEqual([]);
    expect(projection.entries[0]?.unresolvedLinks).toEqual(['topics/zeta.md']);
  });
});
