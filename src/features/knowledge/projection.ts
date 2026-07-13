import type {
  MemoryEntry,
  MemoryIndex,
  MemoryTopicFile,
  MemoryTopics,
} from '../../lib/api/memory';

export type KnowledgeMemory = {
  entry: MemoryEntry;
  staleLinks: string[];
  unresolvedLinks: string[];
};
export type KnowledgeTopic = { file: MemoryTopicFile; backlinks: KnowledgeMemory[] };
export type KnowledgeProjection = {
  entries: KnowledgeMemory[];
  topics: KnowledgeTopic[];
  unlinked: KnowledgeMemory[];
  staleLinked: KnowledgeMemory[];
};

function compareEntries(left: MemoryEntry, right: MemoryEntry): number {
  return right.createdMs - left.createdMs || left.id.localeCompare(right.id);
}

export function buildKnowledgeProjection(
  index: MemoryIndex,
  topicData: MemoryTopics,
): KnowledgeProjection {
  const files = [
    ...topicData.core.filter((file) => file.exists),
    ...topicData.topics
      .filter((file) => file.exists)
      .sort((a, b) => a.name.localeCompare(b.name)),
  ];
  const liveRefs = new Set(files.map((file) => file.name));
  const knownMissingRefs = new Set(
    [...topicData.core, ...topicData.topics]
      .filter((file) => !file.exists)
      .map((file) => file.name),
  );
  const entries = [...index.entries]
    .sort(compareEntries)
    .map((entry) => {
      const absentLinks = entry.links.filter((link) => !liveRefs.has(link));
      const unresolvedLinks = topicData.topicsTruncated
        ? absentLinks.filter(
            (link) => isCanonicalTopicRef(link) && !knownMissingRefs.has(link),
          )
        : [];
      return {
        entry,
        staleLinks: absentLinks.filter((link) => !unresolvedLinks.includes(link)),
        unresolvedLinks,
      };
    });
  const topics = files.map((file) => ({
    file,
    backlinks: entries.filter(({ entry }) => entry.links.includes(file.name)),
  }));
  return {
    entries,
    topics,
    unlinked: entries.filter(({ entry }) => entry.links.length === 0),
    staleLinked: entries.filter(({ staleLinks }) => staleLinks.length > 0),
  };
}

function isCanonicalTopicRef(link: string): boolean {
  const filename = link.startsWith('topics/') ? link.slice('topics/'.length) : '';
  return (
    filename !== '' &&
    !filename.startsWith('.') &&
    !filename.includes('/') &&
    !filename.includes('\\') &&
    filename.endsWith('.md') &&
    filename !== '.md'
  );
}

export function filterKnowledgeMemories(
  memories: KnowledgeMemory[],
  query: string,
): KnowledgeMemory[] {
  const needle = query.trim().toLocaleLowerCase();
  if (needle === '') return memories;
  return memories.filter(({ entry }) => entry.text.toLocaleLowerCase().includes(needle));
}
