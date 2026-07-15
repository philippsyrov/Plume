import type { MemoryEntry, UserMemoryEntry } from '../../lib/api/memory';
import type { ContextSourceRef } from '../../lib/api/chat';
import { ContextDragAction } from '../chat/ContextDragAction';
import { buildLibraryProjection, filterLibraryEntries } from './projection';
import type {
  LibraryChatItem,
  LibraryData,
  LibrarySection,
  LibrarySelection,
} from './libraryTypes';
import { topicDisplayName } from './topicDisplayName';

export function LibraryIndex({
  data,
  query,
  section,
  onRetry,
  onSelect,
  onUseInChat,
  onContextDragActiveChange,
}: {
  data: LibraryData;
  query: string;
  section: LibrarySection;
  onRetry: () => void;
  onSelect: (selection: LibrarySelection) => void;
  onUseInChat?: (item: LibraryChatItem) => void;
  onContextDragActiveChange?: (active: boolean) => void;
}) {
  if (section === 'overview') return null;
  if (section === 'connections') {
    return (
      <ConnectionsIndex
        data={data}
        query={query}
        onRetry={onRetry}
        onSelect={onSelect}
      />
    );
  }
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
        contextSourceOf={(entry) => ({ kind: 'userMemoryEntry', entryId: entry.id })}
        {...(onContextDragActiveChange ? { onContextDragActiveChange } : {})}
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
        contextSourceOf={(entry) => ({ kind: 'memoryEntry', entryId: entry.id })}
        {...(onContextDragActiveChange ? { onContextDragActiveChange } : {})}
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
            {topicDisplayName(file)}
          </button>
          {file.kind === 'topic' && onUseInChat ? (
            onContextDragActiveChange ? (
              <ContextDragAction
                source={{ kind: 'topicFile', name: file.name }}
                onActivate={() => onUseInChat({ kind: 'topic', name: file.name })}
                onDragActiveChange={onContextDragActiveChange}
                className="plume-library-use"
              >
                Use in chat
              </ContextDragAction>
            ) : (
              <button
                type="button"
                className="plume-library-use"
                onClick={() => onUseInChat({ kind: 'topic', name: file.name })}
              >
                Use in chat
              </button>
            )
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
  contextSourceOf,
  onContextDragActiveChange,
}: {
  entries: T[];
  empty: string;
  onSelect: (entry: T) => void;
  onUseInChat?: (entry: T) => void;
  contextSourceOf: (entry: T) => ContextSourceRef;
  onContextDragActiveChange?: (active: boolean) => void;
}) {
  if (entries.length === 0) return <p>{empty}</p>;
  return (
    <ul className="plume-library-index-list" aria-label="Memory entries">
      {entries.map((entry) => (
        <li key={entry.id}>
          <button type="button" onClick={() => onSelect(entry)}>{entry.text}</button>
          {onUseInChat ? (
            onContextDragActiveChange ? (
              <ContextDragAction
                source={contextSourceOf(entry)}
                onActivate={() => onUseInChat(entry)}
                onDragActiveChange={onContextDragActiveChange}
                className="plume-library-use"
              >
                Use in chat
              </ContextDragAction>
            ) : (
              <button
                type="button"
                className="plume-library-use"
                onClick={() => onUseInChat(entry)}
              >
                Use in chat
              </button>
            )
          ) : null}
        </li>
      ))}
    </ul>
  );
}

function sectionLabel(section: Exclude<LibrarySection, 'overview'>): string {
  if (section === 'user-memory') return 'About you';
  if (section === 'project-memory') return 'project memory';
  if (section === 'connections') return 'connections';
  return 'topics';
}

function ConnectionsIndex({
  data,
  query,
  onRetry,
  onSelect,
}: {
  data: LibraryData;
  query: string;
  onRetry: () => void;
  onSelect: (selection: LibrarySelection) => void;
}) {
  if (data.projectMemory.kind === 'loading' || data.topics.kind === 'loading') {
    return <p role="status">Loading connections…</p>;
  }
  if (data.projectMemory.kind === 'unavailable' || data.topics.kind === 'unavailable') {
    return <p>Open a trusted project to see this source.</p>;
  }
  if (data.projectMemory.kind === 'error' || data.topics.kind === 'error') {
    const message = data.projectMemory.kind === 'error'
      ? data.projectMemory.message
      : data.topics.kind === 'error'
        ? data.topics.message
        : 'Connections are unavailable.';
    return (
      <div className="plume-library-source-error">
        <p role="alert">{message}</p>
        <button type="button" onClick={onRetry}>Retry connections</button>
      </div>
    );
  }
  const projection = buildLibraryProjection(data.projectMemory.data, data.topics.data);
  const linked = projection.entries.filter(({ entry }) =>
    entry.links.length > 0 && filterLibraryEntries([entry], query).length > 0
  );
  return (
    <section aria-label="Connections index">
      <p>Connections organize information. They do not choose what goes into chat.</p>
      {linked.length === 0 ? <p>No matching connections.</p> : (
        <ul className="plume-library-index-list">
          {linked.map(({ entry }) => (
            <li key={entry.id}>
              <button
                type="button"
                onClick={() => onSelect({ kind: 'project-memory', entry })}
              >
                {entry.text}
              </button>
              <span>{entry.links.length} topic {entry.links.length === 1 ? 'link' : 'links'}</span>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

function unavailableCopy(section: Exclude<LibrarySection, 'overview'>): string {
  return section === 'user-memory'
    ? 'About you is unavailable right now.'
    : 'Open a trusted project to see this source.';
}
