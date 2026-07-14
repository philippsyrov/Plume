import type { LibraryData, LibrarySection } from './libraryTypes';

export function LibraryTree({
  data,
  projectIdentity,
  section,
  onSelect,
}: {
  data: LibraryData;
  projectIdentity: string | null;
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
        count={data.userMemory.kind === 'ready' ? data.userMemory.data.entries.length : null}
        selected={section === 'user-memory'}
        onClick={() => onSelect('user-memory')}
      />
      <SourceButton
        label="This project"
        state={data.projectMemory}
        count={data.projectMemory.kind === 'ready' ? data.projectMemory.data.entries.length : null}
        selected={section === 'project-memory'}
        onClick={() => onSelect('project-memory')}
      />
      <p className="plume-library-tree-group">Notes</p>
      <SourceButton
        label="Topics"
        state={data.topics}
        count={data.topics.kind === 'ready'
          ? [...data.topics.data.core, ...data.topics.data.topics]
            .filter((file) => file.exists).length
          : null}
        selected={section === 'topics'}
        onClick={() => onSelect('topics')}
      />
      <p className="plume-library-scope">
        {projectIdentity === null ? 'No project open' : 'Trusted project'}
      </p>
    </nav>
  );
}

function SourceButton({
  label,
  state,
  count,
  selected,
  onClick,
}: {
  label: string;
  state: { kind: string };
  count: number | null;
  selected: boolean;
  onClick: () => void;
}) {
  const unavailable = state.kind === 'unavailable';
  const suffix = state.kind === 'ready'
    ? ` ${count ?? 0}`
    : state.kind === 'loading'
      ? ' loading'
      : ' unavailable';
  return (
    <button
      type="button"
      disabled={unavailable}
      aria-current={selected ? 'page' : undefined}
      onClick={onClick}
    >
      {label}{suffix}
    </button>
  );
}
