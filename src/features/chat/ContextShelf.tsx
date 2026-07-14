import type {
  ContextSourceManifestItem,
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
          const browserReady = ready?.kind === 'browserTextEvidence' ? ready : null;
          const displayLabel = browserReady
            ? browserEvidenceLabel(browserReady)
            : contextSourceLabel(source);
          const emphasized = contextSourceKey(source) === emphasizedContextKey;
          return (
            <li
              key={contextSourceKey(source)}
              className={`ink-badge plume-context-shelf-item${blocked ? ' plume-context-shelf-item-blocked' : ''}${emphasized ? ' plume-context-shelf-item-emphasized' : ''}`}
              title={blocked ? outcome.message : browserReady?.sourceUrl ?? displayLabel}
            >
              <span>{contextSourceKindLabel(source)}</span>
              <span className="plume-context-shelf-name">{displayLabel}</span>
              <span className="plume-context-shelf-meta">
                {blocked
                  ? 'blocked'
                  : browserReady
                    ? browserEvidenceMeta(browserReady)
                    : ready
                    ? formatBytes(ready.bytes)
                    : loading
                      ? 'checking…'
                      : 'not checked'}
              </span>
              {browserReady ? (
                <span className="plume-context-shelf-preview">{browserReady.preview}</span>
              ) : null}
              <button
                type="button"
                className="plume-context-shelf-remove"
                onClick={() => onRemove(source)}
                disabled={disabled}
                aria-label={`Remove ${displayLabel} from context`}
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

function browserEvidenceLabel(
  source: Extract<ContextSourceManifestItem, { kind: 'browserTextEvidence' }>,
): string {
  const kind = source.captureKind === 'selection' ? 'Selection' : 'Page';
  const title = source.title?.trim();
  const host = safeHost(source.sourceUrl);
  return [kind, title, host].filter(Boolean).join(' · ');
}

function browserEvidenceMeta(
  source: Extract<ContextSourceManifestItem, { kind: 'browserTextEvidence' }>,
): string {
  const parts = [formatBytes(source.bytes)];
  if (source.redactionCount > 0) parts.push(`${source.redactionCount} redacted`);
  if (source.truncated) parts.push('shortened');
  return parts.join(' · ');
}

function safeHost(sourceUrl: string): string | null {
  try {
    return new URL(sourceUrl).host;
  } catch {
    return null;
  }
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
