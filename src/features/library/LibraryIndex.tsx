import type { MemoryEntry, UserMemoryEntry } from '../../lib/api/memory';
import { filterLibraryEntries } from './projection';
import type {
  LibraryChatItem,
  LibraryData,
  LibrarySection,
  LibrarySelection,
} from './libraryTypes';

export function LibraryIndex({
  data,
  query,
  section,
  onRetry,
  onSelect,
  onUseInChat,
}: {
  data: LibraryData;
  query: string;
  section: LibrarySection;
  onRetry: () => void;
  onSelect: (selection: LibrarySelection) => void;
  onUseInChat?: (item: LibraryChatItem) => void;
}) {
  if (section === 'overview') return null;
  const source = section === 'user-memory'
    ? data.userMemory
    : section === 'project-memory'
      ? data.projectMemory
      : data.topics;
  if (source.kind === 'loading') return <p role="status">Loading {sectionLabel(section)}…</p>;
  if (source.kind === 'unavailable') return <p>{unavailableCopy(section)}</p>;
  if (source.kind === 'error') {
    return (
      <div className="plume-library-source-error">
        <p role="alert">{source.message}</p>
        <button type="button" onClick={onRetry}>Retry {sectionLabel(section)}</button>
      </div>
    );
  }
  if (section === 'user-memory') {
    if (data.userMemory.kind !== 'ready') return null;
    return (
      <MemoryRows<UserMemoryEntry>
        entries={filterLibraryEntries(data.userMemory.data.entries, query)}
        empty="Nothing saved about you yet."
        onSelect={(entry) => onSelect({ kind: 'user-memory', entry })}
        {...(onUseInChat
          ? { onUseInChat: (entry) => onUseInChat({ kind: 'userMemory', entryId: entry.id }) }
          : {})}
      />
    );
  }
  if (section === 'project-memory') {
    if (data.projectMemory.kind !== 'ready') return null;
    return (
      <MemoryRows<MemoryEntry>
        entries={filterLibraryEntries(data.projectMemory.data.entries, query)}
        empty="No memory saved for this project yet."
        onSelect={(entry) => onSelect({ kind: 'project-memory', entry })}
        {...(onUseInChat
          ? { onUseInChat: (entry) => onUseInChat({ kind: 'projectMemory', entryId: entry.id }) }
          : {})}
      />
    );
  }
  if (data.topics.kind !== 'ready') return null;
  const files = [...data.topics.data.core, ...data.topics.data.topics]
    .filter((file) => file.exists)
    .filter((file) => file.name.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()));
  if (files.length === 0) return <p>No matching topic notes.</p>;
  return (
    <ul className="plume-library-index-list" aria-label="Topic notes">
      {files.map((file) => (
        <li key={file.name}>
          <button type="button" onClick={() => onSelect({ kind: 'topic', file })}>
            {file.name}
          </button>
          {file.kind === 'topic' && onUseInChat ? (
            <button
              type="button"
              className="plume-library-use"
              onClick={() => onUseInChat({ kind: 'topic', name: file.name })}
            >
              Use in chat
            </button>
          ) : null}
        </li>
      ))}
    </ul>
  );
}

function MemoryRows<T extends UserMemoryEntry | MemoryEntry>({
  entries,
  empty,
  onSelect,
  onUseInChat,
}: {
  entries: T[];
  empty: string;
  onSelect: (entry: T) => void;
  onUseInChat?: (entry: T) => void;
}) {
  if (entries.length === 0) return <p>{empty}</p>;
  return (
    <ul className="plume-library-index-list" aria-label="Memory entries">
      {entries.map((entry) => (
        <li key={entry.id}>
          <button type="button" onClick={() => onSelect(entry)}>{entry.text}</button>
          {onUseInChat ? (
            <button
              type="button"
              className="plume-library-use"
              onClick={() => onUseInChat(entry)}
            >
              Use in chat
            </button>
          ) : null}
        </li>
      ))}
    </ul>
  );
}

function sectionLabel(section: Exclude<LibrarySection, 'overview'>): string {
  if (section === 'user-memory') return 'About you';
  if (section === 'project-memory') return 'project memory';
  return 'topics';
}

function unavailableCopy(section: Exclude<LibrarySection, 'overview'>): string {
  return section === 'user-memory'
    ? 'About you is unavailable right now.'
    : 'Open a trusted project to see this source.';
}
