import type { KnowledgeMemory } from './projection';

type KnowledgeMemoryCardProps = KnowledgeMemory;

const dateFormatter = new Intl.DateTimeFormat(undefined, {
  dateStyle: 'medium',
  timeStyle: 'short',
});

export function KnowledgeMemoryCard({ entry, staleLinks }: KnowledgeMemoryCardProps) {
  const createdAt = new Date(entry.createdMs);

  return (
    <article className="plume-knowledge-memory" aria-label={`Memory ${entry.id}`}>
      <p>{entry.text}</p>
      <div className="plume-knowledge-memory-meta">
        <time dateTime={createdAt.toISOString()}>{dateFormatter.format(createdAt)}</time>
        <code>{entry.id}</code>
        {entry.redactionCount > 0 ? <span>{entry.redactionCount} redacted</span> : null}
      </div>
      <ul aria-label="Topic links">
        {entry.links.map((link) => {
          const isStale = staleLinks.includes(link);
          return (
            <li key={link} className={isStale ? 'is-stale' : undefined}>
              {link}
              {isStale ? ' · missing topic' : ''}
            </li>
          );
        })}
      </ul>
    </article>
  );
}
