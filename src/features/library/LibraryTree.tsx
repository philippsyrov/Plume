import type { LibraryData, LibrarySection } from './libraryTypes';

export function LibraryTree({
  data,
  section,
  onSelect,
}: {
  data: LibraryData;
  section: LibrarySection;
  onSelect: (section: LibrarySection) => void;
}) {
  return (
    <nav className="plume-library-tree" aria-label="Library sources">
      <button
        type="button"
        aria-current={section === 'overview' ? 'page' : undefined}
        onClick={() => onSelect('overview')}
      >
        Overview
      </button>
      <p className="plume-library-tree-group">Memory</p>
      <SourceButton
        label="About you"
        state={data.userMemory}
        selected={section === 'user-memory'}
        onClick={() => onSelect('user-memory')}
      />
      <SourceButton
        label="This project"
        state={data.projectMemory}
        selected={section === 'project-memory'}
        onClick={() => onSelect('project-memory')}
      />
      <p className="plume-library-tree-group">Notes</p>
      <SourceButton
        label="Topics"
        state={data.topics}
        selected={section === 'topics'}
        onClick={() => onSelect('topics')}
      />
      <SourceButton
        label="Connections"
        state={connectionsState(data)}
        selected={section === 'connections'}
        onClick={() => onSelect('connections')}
      />
    </nav>
  );
}

function connectionsState(data: LibraryData): { kind: string } {
  if (data.projectMemory.kind === 'unavailable' || data.topics.kind === 'unavailable') {
    return { kind: 'unavailable' };
  }
  if (data.projectMemory.kind === 'error' || data.topics.kind === 'error') {
    return { kind: 'error' };
  }
  if (data.projectMemory.kind === 'loading' || data.topics.kind === 'loading') {
    return { kind: 'loading' };
  }
  return { kind: 'ready' };
}

function SourceButton({
  label,
  state,
  selected,
  onClick,
}: {
  label: string;
  state: { kind: string };
  selected: boolean;
  onClick: () => void;
}) {
  const unavailable = state.kind === 'unavailable';
  const accessibleLabel = state.kind === 'loading'
    ? `${label} loading`
    : state.kind === 'ready'
      ? label
      : `${label} unavailable`;
  return (
    <button
      type="button"
      disabled={unavailable}
      aria-label={accessibleLabel}
      aria-current={selected ? 'page' : undefined}
      onClick={onClick}
    >
      {label}
    </button>
  );
}
