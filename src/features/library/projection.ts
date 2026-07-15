import type {
  MemoryEntry,
  MemoryIndex,
  MemoryTopicFile,
  MemoryTopics,
  UserMemoryEntry,
} from '../../lib/api/memory';

export type LibraryProjectMemory = {
  entry: MemoryEntry;
  staleLinks: string[];
  unresolvedLinks: string[];
};

export type LibraryTopic = { file: MemoryTopicFile; backlinks: LibraryProjectMemory[] };

export type LibraryProjection = {
  entries: LibraryProjectMemory[];
  topics: LibraryTopic[];
  unlinked: LibraryProjectMemory[];
  staleLinked: LibraryProjectMemory[];
};

function compareEntries(left: MemoryEntry, right: MemoryEntry): number {
  return right.createdMs - left.createdMs || left.id.localeCompare(right.id);
}

/** Exact adaptation of the shipped Knowledge projection. */
export function buildLibraryProjection(
  index: MemoryIndex,
  topicData: MemoryTopics,
): LibraryProjection {
  const files = [
    ...topicData.core.filter((file) => file.exists),
    ...topicData.topics
      .filter((file) => file.exists)
      .sort((a, b) => a.name.localeCompare(b.name)),
  ];
  const liveRefs = new Set(
    files.map((file) => file.name).filter((name) => isCanonicalTopicRef(name)),
  );
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
    backlinks: isCanonicalTopicRef(file.name)
      ? entries.filter(({ entry }) => entry.links.includes(file.name))
      : [],
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

export function filterLibraryEntries<T extends { text: string }>(entries: T[], query: string): T[] {
  const needle = query.trim().toLocaleLowerCase();
  if (needle === '') return entries;
  return entries.filter((entry) => entry.text.toLocaleLowerCase().includes(needle));
}

export function filterUserMemory(entries: UserMemoryEntry[], query: string): UserMemoryEntry[] {
  return filterLibraryEntries(entries, query);
}
