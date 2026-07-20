import { useEffect, useState } from 'react';

import { ipcErrorMessage, isIpcError } from '../../lib/api/errors';
import {
  loadResearchArtifact,
  type ResearchLoadArtifactResponse,
} from '../../lib/api/research';
import type { ResearchArtifactRef } from './researchTranscript';
import { SafeMarkdownPreview } from './SafeMarkdownPreview';

export function ResearchArtifactEntry({
  reference,
  onOpenSource,
}: {
  reference: ResearchArtifactRef;
  onOpenSource?: (url: string) => void;
}) {
  const [artifact, setArtifact] = useState<ResearchLoadArtifactResponse | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let current = true;
    setArtifact(null);
    setError(null);
    void loadResearchArtifact({
      owner: reference.owner,
      artifactId: reference.artifactId,
      version: reference.version,
    })
      .then((loaded) => {
        if (current) setArtifact(loaded);
      })
      .catch((cause: unknown) => {
        if (current) setError(productError(cause));
      });
    return () => {
      current = false;
    };
  }, [reference.artifactId, reference.owner.scope, reference.owner.sessionId, reference.version]);

  if (error !== null) {
    return (
      <li className="plume-chat-entry plume-chat-entry-error" role="alert">
        <span className="plume-chat-entry-role">Plume</span>
        <p className="plume-chat-entry-content">{error}</p>
      </li>
    );
  }
  if (artifact === null) {
    return (
      <li className="plume-chat-entry plume-chat-entry-assistant" aria-label="Loading research reply">
        <span className="plume-chat-entry-role">Plume</span>
        <p className="plume-chat-entry-content">Opening research…</p>
      </li>
    );
  }

  return (
    <li className="plume-chat-entry plume-chat-entry-assistant" aria-label="assistant research message">
      <span className="plume-chat-entry-role">Plume</span>
      <SafeMarkdownPreview markdown={artifact.markdown} />
      {artifact.sources.length > 0 ? (
        <footer className="plume-research-transcript-sources" aria-label="Sources">
          {artifact.sources.map((source) => (
            <button
              key={source.sourceId}
              type="button"
              className="plume-research-source-link"
              disabled={onOpenSource === undefined || !isSafeWebUrl(source.sourceUrl)}
              onClick={() => onOpenSource?.(source.sourceUrl)}
            >
              {source.title?.trim() || source.sourceUrl}
            </button>
          ))}
        </footer>
      ) : null}
    </li>
  );
}

export function ResearchExportEntry({
  fileName,
  onOpen,
}: {
  fileName: string;
  onOpen?: () => void;
}) {
  return (
    <li className="plume-chat-entry plume-chat-entry-assistant" aria-label="assistant export message">
      <span className="plume-chat-entry-role">Plume</span>
      <button type="button" className="plume-research-export-link" onClick={onOpen} disabled={!onOpen}>
        {fileName}
      </button>
    </li>
  );
}

function isSafeWebUrl(value: string): boolean {
  try {
    const protocol = new URL(value).protocol;
    return protocol === 'https:' || protocol === 'http:';
  } catch {
    return false;
  }
}

function productError(error: unknown): string {
  if (isIpcError(error)) return ipcErrorMessage(error);
  if (error instanceof Error) return error.message;
  return 'This research reply could not be opened.';
}
