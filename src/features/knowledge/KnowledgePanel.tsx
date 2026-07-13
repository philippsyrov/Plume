import { useEffect, useMemo, useState } from 'react';

import type { MemoryIndex, MemoryTopicFile, MemoryTopics } from '../../lib/api/memory';
import { KnowledgeMemoryCard } from './KnowledgeMemoryCard';
import {
  buildKnowledgeProjection,
  filterKnowledgeMemories,
  type KnowledgeMemory,
  type KnowledgeProjection,
} from './projection';
import { useKnowledgeData, type KnowledgeSourceState } from './useKnowledgeData';
import type { ContextSourceRef } from '../../lib/api/chat';
import type { AddContextSourceResult } from '../chat/contextSources';
import { ContextDragAction } from '../chat/ContextDragAction';

type KnowledgeSelection =
  | { kind: 'all' }
  | { kind: 'unlinked' }
  | { kind: 'stale' }
  | { kind: 'topic'; ref: string };

export function KnowledgePanel({
  onUseInChat,
  onContextDragActiveChange,
}: {
  onUseInChat?: (source: ContextSourceRef) => Promise<AddContextSourceResult>;
  onContextDragActiveChange?: (active: boolean) => void;
}) {
  const data = useKnowledgeData();
  const [selection, setSelection] = useState<KnowledgeSelection>({ kind: 'all' });
  const [query, setQuery] = useState('');
  const [contextNotice, setContextNotice] = useState<string | null>(null);
  const projection = useMemo(
    () =>
      data.memory.kind === 'ready' && data.topics.kind === 'ready'
        ? buildKnowledgeProjection(data.memory.data, data.topics.data)
        : null,
    [data.memory, data.topics],
  );

  useEffect(() => {
    if (selection.kind !== 'topic' && selection.kind !== 'stale') return;
    if (data.topics.kind !== 'ready') {
      setSelection({ kind: 'all' });
      return;
    }
    if (
      selection.kind === 'topic' &&
      !availableTopicFiles(data.topics.data).some((file) => file.name === selection.ref)
    ) {
      setSelection({ kind: 'all' });
    }
  }, [data.topics, selection]);

  const useInChat = onUseInChat
    ? async (source: ContextSourceRef): Promise<AddContextSourceResult> => {
        setContextNotice(null);
        try {
          const result = await onUseInChat(source);
          if (result === 'full') {
            setContextNotice('Context is full. Remove an item in chat, then try again.');
          } else if (result === 'unavailable') {
            setContextNotice('Project chat is unavailable right now.');
          }
          return result;
        } catch (error) {
          setContextNotice(error instanceof Error ? error.message : 'Could not add context.');
          return 'unavailable';
        }
      }
    : undefined;

  return (
    <section className="plume-knowledge" aria-label="Project knowledge">
      <KnowledgeHeader query={query} onQueryChange={setQuery} onRefresh={data.refreshAll} />
      {contextNotice ? <p role="alert">{contextNotice}</p> : null}
      <div className="plume-knowledge-grid">
        <KnowledgeNavigation
          memory={data.memory}
          topics={data.topics}
          projection={projection}
          selection={selection}
          onSelect={setSelection}
          onRetry={data.retryTopics}
        />
        <KnowledgeContent
          memory={data.memory}
          topics={data.topics}
          projection={projection}
          selection={selection}
          query={query}
          onRetryMemory={data.retryMemory}
          {...(useInChat ? { onUseInChat: useInChat } : {})}
          {...(onContextDragActiveChange ? { onContextDragActiveChange } : {})}
        />
      </div>
    </section>
  );
}

type KnowledgeHeaderProps = {
  query: string;
  onQueryChange: (query: string) => void;
  onRefresh: () => void;
};

function KnowledgeHeader({ query, onQueryChange, onRefresh }: KnowledgeHeaderProps) {
  return (
    <header className="plume-knowledge-header">
      <div>
        <h2>Knowledge</h2>
        <p>Read-only project memory and curated topic files.</p>
      </div>
      <label>
        Search memories
        <input
          type="search"
          value={query}
          onChange={(event) => onQueryChange(event.currentTarget.value)}
          placeholder="Search loaded memory text"
        />
      </label>
      <button type="button" onClick={onRefresh}>
        Refresh knowledge
      </button>
    </header>
  );
}

type KnowledgeNavigationProps = {
  memory: KnowledgeSourceState<MemoryIndex>;
  topics: KnowledgeSourceState<MemoryTopics>;
  projection: KnowledgeProjection | null;
  selection: KnowledgeSelection;
  onSelect: (selection: KnowledgeSelection) => void;
  onRetry: () => void;
};

function KnowledgeNavigation({
  memory,
  topics,
  projection,
  selection,
  onSelect,
  onRetry,
}: KnowledgeNavigationProps) {
  const entries = memory.kind === 'ready' ? memory.data.entries : [];
  const allCount = memory.kind === 'ready' ? entries.length : null;
  const unlinkedCount =
    memory.kind === 'ready'
      ? entries.filter((entry) => entry.links.length === 0).length
      : null;

  return (
    <nav className="plume-knowledge-navigation" aria-label="Knowledge views">
      <h3>Memory views</h3>
      <button
        type="button"
        aria-current={selection.kind === 'all' ? 'page' : undefined}
        onClick={() => onSelect({ kind: 'all' })}
      >
        All memories{countSuffix(allCount)}
      </button>
      <button
        type="button"
        aria-current={selection.kind === 'unlinked' ? 'page' : undefined}
        onClick={() => onSelect({ kind: 'unlinked' })}
      >
        Unlinked{countSuffix(unlinkedCount)}
      </button>
      <button
        type="button"
        disabled={projection === null}
        aria-current={selection.kind === 'stale' ? 'page' : undefined}
        onClick={() => onSelect({ kind: 'stale' })}
      >
        {projection === null
          ? 'Stale links unavailable'
          : `Stale links ${projection.staleLinked.length}`}
      </button>

      <h3>Topic files</h3>
      {topics.kind === 'ready' && topics.data.topicsTruncated ? (
        <p>
          Topic coverage is partial: only the first {topics.data.limits.maxTopics} topic files are
          shown.
        </p>
      ) : null}
      <TopicNavigationState
        topics={topics}
        projection={projection}
        selection={selection}
        onSelect={onSelect}
        onRetry={onRetry}
      />
    </nav>
  );
}

type TopicNavigationStateProps = Pick<
  KnowledgeNavigationProps,
  'topics' | 'projection' | 'selection' | 'onSelect' | 'onRetry'
>;

function TopicNavigationState({
  topics,
  projection,
  selection,
  onSelect,
  onRetry,
}: TopicNavigationStateProps) {
  if (topics.kind === 'loading') {
    return <p role="status">Loading memory topics…</p>;
  }
  if (topics.kind === 'error') {
    return (
      <div>
        <p role="alert">{topics.message}</p>
        <button type="button" onClick={onRetry}>
          Retry memory topics
        </button>
      </div>
    );
  }

  const files = availableTopicFiles(topics.data);
  if (files.length === 0) {
    return <p>No topic files yet.</p>;
  }

  return (
    <ul>
      {files.map((file) => {
        const backlinkCount = projection?.topics.find((topic) => topic.file.name === file.name)
          ?.backlinks.length;
        const countLabel = backlinkCount === undefined
          ? 'backlinks unavailable'
          : `${backlinkCount} ${backlinkCount === 1 ? 'backlink' : 'backlinks'}`;
        return (
          <li key={file.name}>
            <button
              type="button"
              aria-current={
                selection.kind === 'topic' && selection.ref === file.name ? 'page' : undefined
              }
              onClick={() => onSelect({ kind: 'topic', ref: file.name })}
            >
              {file.name} {countLabel}
            </button>
          </li>
        );
      })}
    </ul>
  );
}

type KnowledgeContentProps = {
  memory: KnowledgeSourceState<MemoryIndex>;
  topics: KnowledgeSourceState<MemoryTopics>;
  projection: KnowledgeProjection | null;
  selection: KnowledgeSelection;
  query: string;
  onRetryMemory: () => void;
  onUseInChat?: (source: ContextSourceRef) => Promise<AddContextSourceResult>;
  onContextDragActiveChange?: (active: boolean) => void;
};

function KnowledgeContent({
  memory,
  topics,
  projection,
  selection,
  query,
  onRetryMemory,
  onUseInChat,
  onContextDragActiveChange,
}: KnowledgeContentProps) {
  const selectedTopicFiles = topicFilesForSelection(topics, selection);
  const trimmedQuery = query.trim();

  return (
    <div className="plume-knowledge-content">
      {selectedTopicFiles.map((file) => (
        <TopicFile
          key={file.name}
          file={file}
          {...(onUseInChat ? { onUseInChat } : {})}
          {...(onContextDragActiveChange ? { onContextDragActiveChange } : {})}
        />
      ))}
      <MemoryContent
        memory={memory}
        projection={projection}
        selection={selection}
        query={trimmedQuery}
        onRetry={onRetryMemory}
        {...(onUseInChat ? { onUseInChat } : {})}
        {...(onContextDragActiveChange ? { onContextDragActiveChange } : {})}
      />
    </div>
  );
}

type MemoryContentProps = Pick<
  KnowledgeContentProps,
  'memory' | 'projection' | 'selection' | 'query'
> & { onRetry: () => void };

function MemoryContent({
  memory,
  projection,
  selection,
  query,
  onRetry,
  onUseInChat,
  onContextDragActiveChange,
}: MemoryContentProps &
  Pick<KnowledgeContentProps, 'onUseInChat' | 'onContextDragActiveChange'>) {
  if (memory.kind === 'loading') {
    return <p role="status">Loading memory entries…</p>;
  }
  if (memory.kind === 'error') {
    return (
      <div>
        <p role="alert">{memory.message}</p>
        <button type="button" onClick={onRetry}>
          Retry memory entries
        </button>
      </div>
    );
  }

  const knownMemories = projection?.entries ?? memory.data.entries.map((entry) => ({
    entry,
    staleLinks: [],
    unresolvedLinks: [],
  }));
  const selectedMemories = memoriesForSelection(knownMemories, projection, selection);
  const shownMemories =
    query === '' ? selectedMemories : filterKnowledgeMemories(knownMemories, query);

  return (
    <section aria-label="Memory entries">
      {query !== '' ? <p>Lexical matches in loaded memory text</p> : null}
      {shownMemories.length === 0 ? (
        <p>
          {query === ''
            ? emptyMemoryCopy(selection)
            : 'No lexical matches in loaded memory text.'}
        </p>
      ) : (
        shownMemories.map((memoryEntry) => (
          <KnowledgeMemoryCard
            key={memoryEntry.entry.id}
            {...memoryEntry}
            {...(onUseInChat
              ? {
                  onUseInChat: (entryId: string) =>
                    void onUseInChat({ kind: 'memoryEntry', entryId }),
                }
              : {})}
            {...(onContextDragActiveChange ? { onContextDragActiveChange } : {})}
          />
        ))
      )}
    </section>
  );
}

function TopicFile({
  file,
  onUseInChat,
  onContextDragActiveChange,
}: {
  file: MemoryTopicFile;
  onUseInChat?: (source: ContextSourceRef) => Promise<AddContextSourceResult>;
  onContextDragActiveChange?: (active: boolean) => void;
}) {
  return (
    <article className="plume-knowledge-topic" aria-label={`${file.name} topic file`}>
      <h3>{file.name}</h3>
      {file.kind === 'topic' && onUseInChat && onContextDragActiveChange ? (
        <ContextDragAction
          source={{ kind: 'topicFile', name: file.name }}
          onActivate={onUseInChat}
          onDragActiveChange={onContextDragActiveChange}
        >
          Use in chat
        </ContextDragAction>
      ) : file.kind === 'topic' && onUseInChat ? (
        <button
          type="button"
          onClick={() => void onUseInChat({ kind: 'topicFile', name: file.name })}
        >
          Use in chat
        </button>
      ) : null}
      <pre>{file.content}</pre>
      {file.truncated ? <p>Content truncated by the backend.</p> : null}
    </article>
  );
}

function availableTopicFiles(topics: MemoryTopics): MemoryTopicFile[] {
  return [
    ...topics.core.filter((file) => file.exists),
    ...topics.topics.filter((file) => file.exists),
  ];
}

function topicFilesForSelection(
  topics: KnowledgeSourceState<MemoryTopics>,
  selection: KnowledgeSelection,
): MemoryTopicFile[] {
  if (topics.kind !== 'ready') return [];
  const files = availableTopicFiles(topics.data);
  if (selection.kind === 'topic') return files.filter((file) => file.name === selection.ref);
  return selection.kind === 'all' ? files : [];
}

function memoriesForSelection(
  entries: KnowledgeMemory[],
  projection: KnowledgeProjection | null,
  selection: KnowledgeSelection,
): KnowledgeMemory[] {
  if (selection.kind === 'all') return entries;
  if (selection.kind === 'unlinked') {
    return projection?.unlinked ?? entries.filter(({ entry }) => entry.links.length === 0);
  }
  if (selection.kind === 'stale') return projection?.staleLinked ?? [];
  return projection?.topics.find((topic) => topic.file.name === selection.ref)?.backlinks ?? [];
}

function emptyMemoryCopy(selection: KnowledgeSelection): string {
  if (selection.kind === 'topic') return 'No memories link to this exact topic ref.';
  if (selection.kind === 'unlinked') return 'No unlinked memory entries.';
  if (selection.kind === 'stale') return 'No stale topic links.';
  return 'No memory entries yet.';
}

function countSuffix(count: number | null): string {
  return count === null ? ' unavailable' : ` ${count}`;
}
