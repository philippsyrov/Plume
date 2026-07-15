import { useEffect, useMemo, useRef, useState } from 'react';

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
  onContextDragActiveChange,
}: {
  projectIdentity: string | null;
  onUseInChat?: (item: LibraryChatItem) => Promise<LibraryUseInChatResult>;
  onContextDragActiveChange?: (active: boolean) => void;
}) {
  const data = useLibraryData({ projectIdentity });
  const projectIdentityRef = useRef(projectIdentity);
  const handoffGeneration = useRef(0);
  if (projectIdentityRef.current !== projectIdentity) {
    projectIdentityRef.current = projectIdentity;
    handoffGeneration.current += 1;
  }
  const [viewIdentity, setViewIdentity] = useState(projectIdentity);
  const [section, setSection] = useState<LibrarySection>('overview');
  const [selectionState, setSelectionState] = useState<{
    projectIdentity: string | null;
    selection: LibrarySelection;
  }>({ projectIdentity, selection: { kind: 'overview' } });
  const [query, setQuery] = useState('');
  const [noticeState, setNoticeState] = useState<{
    projectIdentity: string | null;
    message: string | null;
  }>({ projectIdentity, message: null });
  const scopedView = viewIdentity === projectIdentity;
  const visibleSection = scopedView ? section : 'overview';
  const visibleQuery = scopedView ? query : '';
  const selection = selectionState.projectIdentity === projectIdentity
    ? selectionState.selection
    : { kind: 'overview' as const };
  const notice = noticeState.projectIdentity === projectIdentity
    ? noticeState.message
    : null;
  const projection = useMemo(
    () => data.projectMemory.kind === 'ready' && data.topics.kind === 'ready'
      ? buildLibraryProjection(data.projectMemory.data, data.topics.data)
      : null,
    [data.projectMemory, data.topics],
  );

  useEffect(() => {
    setViewIdentity(projectIdentity);
    setSection('overview');
    setSelectionState({ projectIdentity, selection: { kind: 'overview' } });
    setQuery('');
    setNoticeState({ projectIdentity, message: null });
  }, [projectIdentity]);

  useEffect(() => {
    setSelectionState((current) => {
      if (current.projectIdentity !== projectIdentity) return current;
      const next = refreshSelection(current.selection, data);
      return next === current.selection
        ? current
        : { projectIdentity, selection: next };
    });
  }, [data.projectMemory, data.topics, data.userMemory, projectIdentity]);

  const selectSection = (next: LibrarySection) => {
    setSection(next);
    setSelectionState({ projectIdentity, selection: { kind: 'overview' } });
    setQuery('');
    setNoticeState({ projectIdentity, message: null });
  };
  const useInChat = onUseInChat
    ? async (item: LibraryChatItem) => {
        const identity = projectIdentity;
        const generation = ++handoffGeneration.current;
        setNoticeState({ projectIdentity: identity, message: null });
        try {
          const result = await onUseInChat(item);
          if (
            projectIdentityRef.current !== identity ||
            handoffGeneration.current !== generation
          ) return;
          if (result === 'full') setNoticeState({ projectIdentity: identity, message: 'Chat context is full. Remove something, then try again.' });
          if (result === 'unavailable') setNoticeState({ projectIdentity: identity, message: 'That chat is unavailable right now.' });
          if (result === 'duplicate') setNoticeState({ projectIdentity: identity, message: 'Already in chat context.' });
        } catch (error) {
          if (
            projectIdentityRef.current === identity &&
            handoffGeneration.current === generation
          ) {
            setNoticeState({
              projectIdentity: identity,
              message: error instanceof Error ? error.message : 'Could not add this to chat.',
            });
          }
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
        {visibleSection !== 'overview' ? (
          <label>
            Search {sectionTitle(visibleSection)}
            <input
              type="search"
              aria-label={`Search ${sectionTitle(visibleSection)}`}
              value={visibleQuery}
              onChange={(event) => setQuery(event.currentTarget.value)}
            />
          </label>
        ) : null}
        <button type="button" onClick={data.refreshAll}>Refresh Library</button>
      </header>
      {visibleQuery.trim() !== '' ? <p>Searching {sectionTitle(visibleSection)} only.</p> : null}
      {notice ? <p role="status">{notice}</p> : null}
      <div className="plume-library-grid">
        <LibraryTree
          data={data}
          projectIdentity={projectIdentity}
          section={visibleSection}
          onSelect={selectSection}
        />
        <main className="plume-library-main">
          {visibleSection === 'overview' ? (
            <LibraryOverview data={data} projectIdentity={projectIdentity} />
          ) : (
            <div className="plume-library-browser">
              <section className="plume-library-index" aria-label={`${sectionTitle(visibleSection)} list`}>
                <LibraryIndex
                  data={data}
                  query={visibleQuery}
                  section={visibleSection}
                  onRetry={() => retryForSection(data, visibleSection)}
                  onSelect={(next) => setSelectionState({ projectIdentity, selection: next })}
                  {...(useInChat ? { onUseInChat: (item: LibraryChatItem) => void useInChat(item) } : {})}
                  {...(onContextDragActiveChange ? { onContextDragActiveChange } : {})}
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
  if (section === 'connections') return 'Connections';
  return 'Library';
}

function retryForSection(data: ReturnType<typeof useLibraryData>, section: LibrarySection): void {
  if (section === 'user-memory') data.retryUserMemory();
  if (section === 'project-memory') data.retryProjectMemory();
  if (section === 'topics') data.retryTopics();
  if (section === 'connections') {
    data.retryProjectMemory();
    data.retryTopics();
  }
}

function refreshSelection(selection: LibrarySelection, data: ReturnType<typeof useLibraryData>): LibrarySelection {
  if (selection.kind === 'overview') return selection;
  if (selection.kind === 'user-memory') {
    if (data.userMemory.kind !== 'ready') return selection;
    const entry = data.userMemory.data.entries.find(({ id }) => id === selection.entry.id);
    return entry === undefined
      ? { kind: 'overview' }
      : entry === selection.entry
        ? selection
        : { kind: 'user-memory', entry };
  }
  if (selection.kind === 'project-memory') {
    if (data.projectMemory.kind !== 'ready') return selection;
    const entry = data.projectMemory.data.entries.find(({ id }) => id === selection.entry.id);
    return entry === undefined
      ? { kind: 'overview' }
      : entry === selection.entry
        ? selection
        : { kind: 'project-memory', entry };
  }
  if (data.topics.kind !== 'ready') return selection;
  const file = [...data.topics.data.core, ...data.topics.data.topics]
    .find(({ name }) => name === selection.file.name);
  return file === undefined
    ? { kind: 'overview' }
    : file === selection.file
      ? selection
      : { kind: 'topic', file };
}
