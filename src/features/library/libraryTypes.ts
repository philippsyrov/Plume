import type {
  MemoryEntry,
  MemoryIndex,
  MemoryTopicFile,
  MemoryTopics,
  UserMemoryEntry,
  UserMemoryIndex,
} from '../../lib/api/memory';

export type LibrarySourceState<T> =
  | { kind: 'loading' }
  | { kind: 'ready'; data: T }
  | { kind: 'unavailable' }
  | { kind: 'error'; message: string };

export type LibraryData = {
  userMemory: LibrarySourceState<UserMemoryIndex>;
  projectMemory: LibrarySourceState<MemoryIndex>;
  topics: LibrarySourceState<MemoryTopics>;
  retryUserMemory: () => void;
  retryProjectMemory: () => void;
  retryTopics: () => void;
  refreshAll: () => void;
};

export type LibrarySection = 'overview' | 'user-memory' | 'project-memory' | 'topics';

export type LibrarySelection =
  | { kind: 'overview' }
  | { kind: 'user-memory'; entry: UserMemoryEntry }
  | { kind: 'project-memory'; entry: MemoryEntry }
  | { kind: 'topic'; file: MemoryTopicFile };

/**
 * Library-owned handoff intent. The app integration translates this opaque
 * identity into whichever exact chat source kinds are available at that time.
 */
export type LibraryChatItem =
  | { kind: 'userMemory'; entryId: string }
  | { kind: 'projectMemory'; entryId: string }
  | { kind: 'topic'; name: string };

export type LibraryUseInChatResult = 'added' | 'duplicate' | 'full' | 'unavailable';
