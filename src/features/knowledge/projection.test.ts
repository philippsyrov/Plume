import { describe, expect, it } from 'vitest';

import type {
  MemoryEntry,
  MemoryIndex,
  MemoryTopicFile,
  MemoryTopicKind,
  MemoryTopics,
} from '../../lib/api/memory';
import { buildKnowledgeProjection, filterKnowledgeMemories } from './projection';

function entry(
  id: string,
  createdMs: number,
  links: string[],
  text = `Memory ${id}`,
): MemoryEntry {
  return { id, createdMs, text, redactionCount: 0, links };
}

function memoryIndex(entries: MemoryEntry[]): MemoryIndex {
  return {
    entries,
    limits: { maxEntries: 100, maxBytesPerEntry: 1024, maxBytesTotal: 65_536 },
    totalBytes: 0,
  };
}

function topicFile(name: string, exists = true, kind: MemoryTopicKind = 'topic'): MemoryTopicFile {
  return { name, kind, exists, bytes: 0, truncated: false, content: '' };
}

function memoryTopics(topicNames: string[], core: MemoryTopicFile[] = []): MemoryTopics {
  return {
    core,
    topics: topicNames.map((name) => topicFile(name)),
    topicsTruncated: false,
    limits: { maxCoreBytes: 16_384, maxTopicBytes: 32_768, maxTopics: 50 },
  };
}

describe('buildKnowledgeProjection', () => {
  it('derives backlinks only from exact live refs and keeps stale refs visible', () => {
    const projection = buildKnowledgeProjection(
      memoryIndex([
        entry('m_b', 20, ['topics/alpha.md', 'topics/removed.md']),
        entry('m_a', 20, ['topics/beta.md']),
        entry('m_old', 10, []),
      ]),
      memoryTopics(['topics/alpha.md', 'topics/beta.md']),
    );

    expect(projection.entries.map(({ entry }) => entry.id)).toEqual(['m_a', 'm_b', 'm_old']);
    expect(projection.topics[0]?.backlinks.map(({ entry }) => entry.id)).toEqual(['m_b']);
    expect(projection.staleLinked[0]?.staleLinks).toEqual(['topics/removed.md']);
    expect(projection.unlinked.map(({ entry }) => entry.id)).toEqual(['m_old']);
  });

  it('uses exact refs rather than basename or fuzzy matches', () => {
    const projection = buildKnowledgeProjection(
      memoryIndex([entry('m_1', 1, ['alpha.md'])]),
      memoryTopics(['topics/alpha.md']),
    );

    expect(projection.topics[0]?.backlinks).toEqual([]);
    expect(projection.staleLinked[0]?.staleLinks).toEqual(['alpha.md']);
  });

  it('keeps surfaced core and noncanonical topic refs out of backlinks', () => {
    const projection = buildKnowledgeProjection(
      memoryIndex([
        entry('m_core', 2, ['INDEX.md']),
        entry('m_hidden', 1, ['topics/.hidden.md']),
      ]),
      {
        ...memoryTopics(['topics/.hidden.md']),
        core: [topicFile('INDEX.md', true, 'index')],
      },
    );

    expect(projection.topics.map(({ file }) => file.name)).toEqual([
      'INDEX.md',
      'topics/.hidden.md',
    ]);
    expect(projection.topics.map(({ backlinks }) => backlinks)).toEqual([[], []]);
    expect(projection.entries.map(({ staleLinks }) => staleLinks)).toEqual([
      ['INDEX.md'],
      ['topics/.hidden.md'],
    ]);
    expect(projection.staleLinked.map(({ entry }) => entry.id)).toEqual([
      'm_core',
      'm_hidden',
    ]);
  });

  it('excludes missing files and puts existing core files before sorted topics', () => {
    const projection = buildKnowledgeProjection(
      memoryIndex([]),
      {
        ...memoryTopics([]),
        core: [
          topicFile('INDEX.md', true, 'index'),
          topicFile('USER.md', false, 'user'),
          topicFile('SOUL.md', true, 'soul'),
        ],
        topics: [
          topicFile('topics/zeta.md'),
          topicFile('topics/missing.md', false),
          topicFile('topics/alpha.md'),
        ],
      },
    );

    expect(projection.topics.map(({ file }) => file.name)).toEqual([
      'INDEX.md',
      'SOUL.md',
      'topics/alpha.md',
      'topics/zeta.md',
    ]);
  });

  it('does not mutate memory link arrays', () => {
    const links = ['topics/removed.md', 'topics/alpha.md'];
    const projection = buildKnowledgeProjection(
      memoryIndex([entry('m_1', 1, links)]),
      memoryTopics(['topics/alpha.md']),
    );

    expect(links).toEqual(['topics/removed.md', 'topics/alpha.md']);
    expect(projection.entries[0]?.entry.links).toBe(links);
  });

  it('keeps a memory with live and stale links in backlinks and staleLinked', () => {
    const projection = buildKnowledgeProjection(
      memoryIndex([entry('m_1', 1, ['topics/alpha.md', 'topics/removed.md'])]),
      memoryTopics(['topics/alpha.md']),
    );

    expect(projection.topics[0]?.backlinks.map(({ entry }) => entry.id)).toEqual(['m_1']);
    expect(projection.staleLinked.map(({ entry }) => entry.id)).toEqual(['m_1']);
  });

  it('keeps canonical refs outside a truncated topic list unresolved instead of stale', () => {
    const topics = memoryTopics(['topics/alpha.md']);
    topics.topicsTruncated = true;
    const links = ['topics/alpha.md', 'topics/zeta.md'];

    const projection = buildKnowledgeProjection(
      memoryIndex([entry('m_1', 1, links)]),
      topics,
    );

    expect(projection.entries[0]?.staleLinks).toEqual([]);
    expect(projection.entries[0]?.unresolvedLinks).toEqual(['topics/zeta.md']);
    expect(projection.staleLinked).toEqual([]);
    expect(links).toEqual(['topics/alpha.md', 'topics/zeta.md']);
  });

  it('keeps noncanonical and missing core refs definitively stale when topics are truncated', () => {
    const topics = memoryTopics([], [topicFile('INDEX.md', false, 'index')]);
    topics.topicsTruncated = true;

    const projection = buildKnowledgeProjection(
      memoryIndex([entry('m_1', 1, ['INDEX.md', 'alpha.md', 'topics/not-markdown.txt'])]),
      topics,
    );

    expect(projection.entries[0]?.staleLinks).toEqual([
      'INDEX.md',
      'alpha.md',
      'topics/not-markdown.txt',
    ]);
    expect(projection.entries[0]?.unresolvedLinks).toEqual([]);
    expect(projection.staleLinked.map(({ entry }) => entry.id)).toEqual(['m_1']);
  });
});

describe('filterKnowledgeMemories', () => {
  it('filters memory text with honest case-insensitive substring matching', () => {
    const projection = buildKnowledgeProjection(
      memoryIndex([
        entry('m_1', 2, [], 'Prefer Rust'),
        entry('m_2', 1, [], 'Use TypeScript'),
      ]),
      memoryTopics([]),
    );

    expect(
      filterKnowledgeMemories(projection.entries, ' RUST ').map(({ entry }) => entry.id),
    ).toEqual(['m_1']);
  });

  it('returns all entries for an empty or whitespace-only query', () => {
    const projection = buildKnowledgeProjection(
      memoryIndex([entry('m_1', 1, []), entry('m_2', 2, [])]),
      memoryTopics([]),
    );

    expect(filterKnowledgeMemories(projection.entries, '')).toBe(projection.entries);
    expect(filterKnowledgeMemories(projection.entries, '   ')).toBe(projection.entries);
  });
});
