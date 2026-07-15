import type {
  ContextSourceManifestItem,
  ContextSourcePreviewItem,
  ContextSourceRef,
} from '../../lib/api/chat';
import { contextSourceKey } from './contextSources';
import { formatBytes } from './formatters';
import { Disclosure } from '../project-shell/Disclosure';
import { Icon, type IconName } from '../project-shell/Icon';

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
          const displayLabel = readableContextTitle(source, ready);
          const emphasized = contextSourceKey(source) === emphasizedContextKey;
          return (
            <li
              key={contextSourceKey(source)}
              className={`ink-badge plume-context-shelf-item${blocked ? ' plume-context-shelf-item-blocked' : ''}${emphasized ? ' plume-context-shelf-item-emphasized' : ''}`}
              title={blocked ? outcome.message : undefined}
            >
              <span className="plume-context-shelf-kind">
                <Icon name={contextSourceIcon(source)} size={13} />
                {contextSourceKindLabel(source)}
              </span>
              <span className="plume-context-shelf-name">{displayLabel}</span>
              {blocked ? <span className="plume-context-shelf-meta">blocked</span> : null}
              {browserReady ? (
                <span className="plume-context-shelf-preview">{browserReady.preview}</span>
              ) : null}
              <Disclosure summary="Details" className="plume-context-shelf-details">
                <span className="plume-context-shelf-detail-ref">
                  {exactContextReference(source, ready)}
                </span>
                <span className="plume-context-shelf-meta">
                  {blocked
                    ? outcome.message
                    : ready
                      ? readyContextMeta(ready)
                      : loading
                        ? 'Checking context…'
                        : 'Not checked'}
                </span>
              </Disclosure>
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

function readableContextTitle(
  source: ContextSourceRef,
  ready: ContextSourceManifestItem | null,
): string {
  if (ready?.kind === 'memoryEntry') return ready.preview;
  if (ready?.kind === 'userMemoryEntry') return ready.preview;
  if (ready?.kind === 'browserTextEvidence') return browserEvidenceLabel(ready);
  if (ready?.kind === 'browserScreenshotEvidence') return screenshotEvidenceLabel(ready);
  if (source.kind === 'projectFile') {
    const name = basename(source.relPath);
    if (source.startLine === undefined || source.endLine === undefined) return name;
    const lines = source.startLine === source.endLine
      ? `line ${source.startLine}`
      : `lines ${source.startLine}–${source.endLine}`;
    return `${name} · ${lines}`;
  }
  if (source.kind === 'memoryEntry') return 'Saved memory';
  if (source.kind === 'userMemoryEntry') return 'Saved user memory';
  if (source.kind === 'topicFile') return basename(source.name);
  if (source.kind === 'browserTextEvidence') return 'Captured page text';
  return 'Captured screenshot';
}

function exactContextReference(
  source: ContextSourceRef,
  ready: ContextSourceManifestItem | null,
): string {
  if (ready?.kind === 'browserTextEvidence' || ready?.kind === 'browserScreenshotEvidence') {
    return ready.sourceUrl;
  }
  return contextSourceLabel(source);
}

function readyContextMeta(ready: ContextSourceManifestItem): string {
  if (ready.kind === 'browserTextEvidence') return browserEvidenceMeta(ready);
  if (ready.kind === 'browserScreenshotEvidence') {
    return `${ready.width}×${ready.height} · ${formatBytes(ready.bytes)}`;
  }
  const parts = [formatBytes(ready.bytes)];
  if (ready.kind === 'projectFile' && ready.redactionCount > 0) {
    parts.push(`${ready.redactionCount} redacted`);
  }
  return parts.join(' · ');
}

function contextSourceIcon(source: ContextSourceRef): IconName {
  switch (source.kind) {
    case 'projectFile':
      return 'files';
    case 'memoryEntry':
      return 'knowledge';
    case 'userMemoryEntry':
      return 'knowledge';
    case 'topicFile':
      return 'library';
    case 'browserTextEvidence':
    case 'browserScreenshotEvidence':
      return 'browser';
  }
}

function basename(path: string): string {
  return path.split('/').filter(Boolean).at(-1) ?? path;
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
  if (source.kind === 'userMemoryEntry') return source.entryId;
  if (source.kind === 'topicFile') return source.name;
  if (source.kind === 'browserTextEvidence') return 'Captured page text';
  if (source.kind === 'browserScreenshotEvidence') return 'Captured screenshot';
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
    case 'userMemoryEntry':
      return 'User memory';
    case 'topicFile':
      return 'Topic';
    case 'browserTextEvidence':
      return 'Web';
    case 'browserScreenshotEvidence':
      return 'Image';
  }
}

function screenshotEvidenceLabel(
  source: Extract<ContextSourceManifestItem, { kind: 'browserScreenshotEvidence' }>,
): string {
  return ['Screenshot', source.title?.trim(), safeHost(source.sourceUrl)]
    .filter(Boolean)
    .join(' · ');
}
