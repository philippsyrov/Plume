import type { LibraryProjection } from './projection';
import type { LibrarySelection } from './libraryTypes';

const dateFormatter = new Intl.DateTimeFormat(undefined, {
  dateStyle: 'medium',
  timeStyle: 'short',
});

export function LibraryDetail({
  selection,
  projection,
}: {
  selection: LibrarySelection;
  projection: LibraryProjection | null;
}) {
  if (selection.kind === 'overview') return null;
  if (selection.kind === 'topic') {
    const backlinks = projection?.topics.find(({ file }) => file.name === selection.file.name)
      ?.backlinks ?? [];
    return (
      <article className="plume-library-detail" aria-label={`Topic ${selection.file.name}`}>
        <h3>{selection.file.name}</h3>
        <pre>{selection.file.content}</pre>
        {selection.file.truncated ? <p>Only the stored preview is shown.</p> : null}
        <section aria-label="Connections">
          <h4>Connections</h4>
          <p>Connections organize information. They do not choose what goes into chat.</p>
          {backlinks.length === 0 ? <p>No exact backlinks.</p> : (
            <ul>{backlinks.map(({ entry }) => <li key={entry.id}>{entry.text}</li>)}</ul>
          )}
        </section>
        <details>
          <summary>Details</summary>
          <p>{selection.file.bytes} bytes · {selection.file.kind}</p>
        </details>
      </article>
    );
  }
  if (selection.kind === 'user-memory') {
    const { entry } = selection;
    return (
      <article className="plume-library-detail" aria-label={`Memory ${entry.id}`}>
        <p>{entry.text}</p>
        <MemoryDetails
          id={entry.id}
          createdMs={entry.createdMs}
          redactionCount={entry.redactionCount}
        />
      </article>
    );
  }
  const { entry } = selection;
  const projectMemory = projection?.entries.find(
    ({ entry: candidate }) => candidate.id === entry.id,
  );
  return (
    <article className="plume-library-detail" aria-label={`Memory ${entry.id}`}>
      <p>{entry.text}</p>
      <section aria-label="Connections">
        <h4>Connections</h4>
        <p>Connections organize information. They do not choose what goes into chat.</p>
        {entry.links.length === 0 ? <p>No topic links.</p> : (
          <ul>
            {entry.links.map((link) => (
              <li key={link}>
                {link}
                {projectMemory?.staleLinks.includes(link) ? ' · missing topic' : ''}
                {projectMemory?.unresolvedLinks.includes(link)
                  ? ' · not verified (topic list capped)'
                  : ''}
              </li>
            ))}
          </ul>
        )}
      </section>
      <MemoryDetails
        id={entry.id}
        createdMs={entry.createdMs}
        redactionCount={entry.redactionCount}
      />
    </article>
  );
}

function MemoryDetails({
  id,
  createdMs,
  redactionCount,
}: {
  id: string;
  createdMs: number;
  redactionCount: number;
}) {
  return (
    <details>
      <summary>Details</summary>
      <time dateTime={new Date(createdMs).toISOString()}>
        {dateFormatter.format(new Date(createdMs))}
      </time>
      <code>{id}</code>
      {redactionCount > 0 ? <p>{redactionCount} redacted</p> : null}
    </details>
  );
}
