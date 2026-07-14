import { useEffect, useMemo, useState } from 'react';

import { LibraryDetail } from './LibraryDetail';
import { LibraryIndex } from './LibraryIndex';
import { LibraryTree } from './LibraryTree';
import { buildLibraryProjection } from './projection';
import type {
  LibraryChatItem,
  LibrarySection,
  LibrarySelection,
  LibraryUseInChatResult,
} from './libraryTypes';
import { useLibraryData } from './useLibraryData';

export function LibraryPanel({
  projectIdentity,
  onUseInChat,
}: {
  projectIdentity: string | null;
  onUseInChat?: (item: LibraryChatItem) => Promise<LibraryUseInChatResult>;
}) {
  const data = useLibraryData({ projectIdentity });
  const [section, setSection] = useState<LibrarySection>('overview');
  const [selection, setSelection] = useState<LibrarySelection>({ kind: 'overview' });
  const [query, setQuery] = useState('');
  const [notice, setNotice] = useState<string | null>(null);
  const projection = useMemo(
    () => data.projectMemory.kind === 'ready' && data.topics.kind === 'ready'
      ? buildLibraryProjection(data.projectMemory.data, data.topics.data)
      : null,
    [data.projectMemory, data.topics],
  );

  useEffect(() => {
    setSection('overview');
    setSelection({ kind: 'overview' });
    setQuery('');
    setNotice(null);
  }, [projectIdentity]);

  const selectSection = (next: LibrarySection) => {
    setSection(next);
    setSelection({ kind: 'overview' });
    setQuery('');
    setNotice(null);
  };
  const useInChat = onUseInChat
    ? async (item: LibraryChatItem) => {
        setNotice(null);
        try {
          const result = await onUseInChat(item);
          if (result === 'full') setNotice('Chat context is full. Remove something, then try again.');
          if (result === 'unavailable') setNotice('That chat is unavailable right now.');
          if (result === 'duplicate') setNotice('Already in chat context.');
        } catch (error) {
          setNotice(error instanceof Error ? error.message : 'Could not add this to chat.');
        }
      }
    : undefined;

  return (
    <section className="plume-library" aria-label="Library">
      <header className="plume-library-header">
        <div>
          <h2>Library</h2>
          <p>Your memory and this project's organized notes.</p>
        </div>
        {section !== 'overview' ? (
          <label>
            Search {sectionTitle(section)}
            <input
              type="search"
              aria-label={`Search ${sectionTitle(section)}`}
              value={query}
              onChange={(event) => setQuery(event.currentTarget.value)}
            />
          </label>
        ) : null}
        <button type="button" onClick={data.refreshAll}>Refresh Library</button>
      </header>
      {query.trim() !== '' ? <p>Searching {sectionTitle(section)} only.</p> : null}
      {notice ? <p role="status">{notice}</p> : null}
      <div className="plume-library-grid">
        <LibraryTree
          data={data}
          projectIdentity={projectIdentity}
          section={section}
          onSelect={selectSection}
        />
        <main className="plume-library-main">
          {section === 'overview' ? (
            <LibraryOverview data={data} projectIdentity={projectIdentity} />
          ) : (
            <div className="plume-library-browser">
              <section className="plume-library-index" aria-label={`${sectionTitle(section)} list`}>
                <LibraryIndex
                  data={data}
                  query={query}
                  section={section}
                  onRetry={() => retryForSection(data, section)}
                  onSelect={setSelection}
                  {...(useInChat ? { onUseInChat: (item: LibraryChatItem) => void useInChat(item) } : {})}
                />
              </section>
              <section className="plume-library-canvas" aria-label="Library detail">
                {selection.kind === 'overview'
                  ? <p>Select an item to read it.</p>
                  : <LibraryDetail selection={selection} projection={projection} />}
              </section>
            </div>
          )}
        </main>
      </div>
    </section>
  );
}

function LibraryOverview({
  data,
  projectIdentity,
}: {
  data: ReturnType<typeof useLibraryData>;
  projectIdentity: string | null;
}) {
  return (
    <section className="plume-library-overview" aria-label="Library overview">
      <h3>About you</h3>
      <p>Stored on this Mac and available without opening a project.</p>
      <p>{sourceCount(data.userMemory, 'memory')}</p>
      <h3>This project</h3>
      <p>Stored only for this trusted project.</p>
      {projectIdentity === null ? (
        <p>Open a trusted project to see its memory and topics.</p>
      ) : (
        <p>{sourceCount(data.projectMemory, 'memory')} · {sourceCount(data.topics, 'topic')}</p>
      )}
    </section>
  );
}

function sourceCount(
  source: ReturnType<typeof useLibraryData>['userMemory'] | ReturnType<typeof useLibraryData>['topics'],
  noun: 'memory' | 'topic',
): string {
  if (source.kind === 'loading') return `Loading ${noun}…`;
  if (source.kind === 'error') return `${noun} unavailable`;
  if (source.kind === 'unavailable') return `${noun} unavailable`;
  const count = 'entries' in source.data
    ? source.data.entries.length
    : [...source.data.core, ...source.data.topics].filter((file) => file.exists).length;
  return `${count} ${noun} ${count === 1 ? 'item' : 'items'}`;
}

function sectionTitle(section: LibrarySection): string {
  if (section === 'user-memory') return 'About you';
  if (section === 'project-memory') return 'This project';
  if (section === 'topics') return 'Topics';
  return 'Library';
}

function retryForSection(data: ReturnType<typeof useLibraryData>, section: LibrarySection): void {
  if (section === 'user-memory') data.retryUserMemory();
  if (section === 'project-memory') data.retryProjectMemory();
  if (section === 'topics') data.retryTopics();
}
