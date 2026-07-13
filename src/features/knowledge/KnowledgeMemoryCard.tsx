import type { KnowledgeMemory } from './projection';
import { ContextDragAction } from '../chat/ContextDragAction';

type KnowledgeMemoryCardProps = KnowledgeMemory;
type UseMemoryProps = {
  onUseInChat?: (entryId: string) => void;
  onContextDragActiveChange?: (active: boolean) => void;
};

const dateFormatter = new Intl.DateTimeFormat(undefined, {
  dateStyle: 'medium',
  timeStyle: 'short',
});

export function KnowledgeMemoryCard({
  entry,
  staleLinks,
  unresolvedLinks,
  onUseInChat,
  onContextDragActiveChange,
}: KnowledgeMemoryCardProps & UseMemoryProps) {
  const createdAt = new Date(entry.createdMs);

  return (
    <article className="plume-knowledge-memory" aria-label={`Memory ${entry.id}`}>
      <p>{entry.text}</p>
      {onUseInChat && onContextDragActiveChange ? (
        <ContextDragAction
          source={{ kind: 'memoryEntry', entryId: entry.id }}
          onActivate={() => onUseInChat(entry.id)}
          onDragActiveChange={onContextDragActiveChange}
        >
          Use in chat
        </ContextDragAction>
      ) : onUseInChat ? (
        <button type="button" onClick={() => onUseInChat(entry.id)}>
          Use in chat
        </button>
      ) : null}
      <div className="plume-knowledge-memory-meta">
        <time dateTime={createdAt.toISOString()}>{dateFormatter.format(createdAt)}</time>
        <code>{entry.id}</code>
        {entry.redactionCount > 0 ? <span>{entry.redactionCount} redacted</span> : null}
      </div>
      <ul aria-label="Topic links">
        {entry.links.map((link) => {
          const isStale = staleLinks.includes(link);
          const isUnresolved = unresolvedLinks.includes(link);
          return (
            <li
              key={link}
              className={isStale ? 'is-stale' : isUnresolved ? 'is-unresolved' : undefined}
            >
              {link}
              {isStale
                ? ' · missing topic'
                : isUnresolved
                  ? ' · not verified (topic list capped)'
                  : ''}
            </li>
          );
        })}
      </ul>
    </article>
  );
}
