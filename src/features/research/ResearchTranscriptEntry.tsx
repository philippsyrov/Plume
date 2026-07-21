import { useEffect, useRef, useState } from 'react';

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
  onOpenSource?: (url: string) => void | Promise<void>;
}) {
  const [artifact, setArtifact] = useState<ResearchLoadArtifactResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [sourceError, setSourceError] = useState<string | null>(null);

  useEffect(() => {
    let current = true;
    setArtifact(null);
    setError(null);
    setSourceError(null);
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
      <SafeMarkdownPreview markdown={transcriptMarkdown(artifact.markdown)} />
      {artifact.artifact.citationStatus === 'needsReview' ? (
        <p className="plume-chat-entry-meta">Draft — check citations.</p>
      ) : null}
      {artifact.sources.length > 0 ? (
        <footer className="plume-research-transcript-sources" aria-label="Sources">
          {artifact.sources.map((source) => {
            const label = source.title?.trim() || source.sourceUrl;
            if (onOpenSource === undefined || !isSafeWebUrl(source.sourceUrl)) {
              return <span key={source.sourceId}>{label}</span>;
            }
            return (
              <button
                key={source.sourceId}
                type="button"
                className="plume-research-source-link"
                onClick={() => {
                  setSourceError(null);
                  Promise.resolve(onOpenSource(source.sourceUrl)).catch((cause: unknown) => {
                    setSourceError(productError(cause));
                  });
                }}
              >
                {label}
              </button>
            );
          })}
          {sourceError !== null ? <span role="alert">{sourceError}</span> : null}
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
  onOpen?: () => void | Promise<void>;
}) {
  const [error, setError] = useState<string | null>(null);
  const mountedRef = useRef(true);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  return (
    <li className="plume-chat-entry plume-chat-entry-assistant" aria-label="assistant export message">
      <span className="plume-chat-entry-role">Plume</span>
      <button
        type="button"
        className="plume-research-export-link"
        onClick={() => {
          if (!onOpen) return;
          setError(null);
          Promise.resolve(onOpen()).catch((cause: unknown) => {
            if (mountedRef.current) setError(productError(cause));
          });
        }}
        disabled={!onOpen}
      >
        {fileName}
      </button>
      {error !== null ? <p className="plume-chat-entry-meta" role="alert">{error}</p> : null}
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

function transcriptMarkdown(markdown: string): string {
  const lines = markdown.replace(/\r\n?/g, '\n').split('\n');
  const sourcesIndex = lines.findIndex((line) => /^#{1,6}\s+sources\s*$/i.test(line));
  return lines
    .slice(0, sourcesIndex === -1 ? undefined : sourcesIndex)
    .join('\n')
    .replace(/\s*\[\^S\d+\]/g, '')
    .trim();
}

function productError(error: unknown): string {
  if (isIpcError(error)) return ipcErrorMessage(error);
  if (error instanceof Error) return error.message;
  return 'This research reply could not be opened.';
}
