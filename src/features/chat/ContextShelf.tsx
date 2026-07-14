import type {
  ContextSourcePreviewItem,
  ContextSourceRef,
} from '../../lib/api/chat';
import { contextSourceKey } from './contextSources';
import { formatBytes } from './formatters';

export function ContextShelf({
  sources,
  preview,
  loading,
  disabled,
  emphasizedContextKey,
  onRemove,
}: {
  sources: ContextSourceRef[];
  preview: ContextSourcePreviewItem[];
  loading: boolean;
  disabled: boolean;
  emphasizedContextKey?: string | null;
  onRemove: (source: ContextSourceRef) => void;
}) {
  if (sources.length === 0) return null;
  return (
    <section className="plume-context-shelf" aria-label="Context for this chat">
      <span className="plume-context-shelf-label">Context</span>
      <ol className="plume-context-shelf-list">
        {sources.map((source, index) => {
          const outcome = preview[index];
          const blocked = outcome?.status === 'blocked';
          const ready = outcome?.status === 'ready' ? outcome.source : null;
          const emphasized = contextSourceKey(source) === emphasizedContextKey;
          return (
            <li
              key={contextSourceKey(source)}
              className={`ink-badge plume-context-shelf-item${blocked ? ' plume-context-shelf-item-blocked' : ''}${emphasized ? ' plume-context-shelf-item-emphasized' : ''}`}
              title={blocked ? outcome.message : contextSourceLabel(source)}
            >
              <span>{contextSourceKindLabel(source)}</span>
              <span className="plume-context-shelf-name">{contextSourceLabel(source)}</span>
              <span className="plume-context-shelf-meta">
                {blocked
                  ? 'blocked'
                  : ready
                    ? formatBytes(ready.bytes)
                    : loading
                      ? 'checking…'
                      : 'not checked'}
              </span>
              <button
                type="button"
                className="plume-context-shelf-remove"
                onClick={() => onRemove(source)}
                disabled={disabled}
                aria-label={`Remove ${contextSourceLabel(source)} from context`}
              >
                ×
              </button>
            </li>
          );
        })}
      </ol>
    </section>
  );
}

export function contextSourceLabel(source: ContextSourceRef): string {
  if (source.kind === 'memoryEntry') return source.entryId;
  if (source.kind === 'topicFile') return source.name;
  if (source.kind === 'browserTextEvidence') return 'Captured page text';
  if (source.startLine === undefined || source.endLine === undefined) {
    return source.relPath;
  }
  return source.startLine === source.endLine
    ? `${source.relPath}:${source.startLine}`
    : `${source.relPath}:${source.startLine}–${source.endLine}`;
}

function contextSourceKindLabel(source: ContextSourceRef): string {
  switch (source.kind) {
    case 'projectFile':
      return 'File';
    case 'memoryEntry':
      return 'Memory';
    case 'topicFile':
      return 'Topic';
    case 'browserTextEvidence':
      return 'Web';
  }
}
