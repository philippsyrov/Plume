import { useEffect, useRef, useState } from 'react';

import { ipcErrorMessage, isIpcError } from '../../lib/api/errors';
import type {
  ResearchExportOutcome,
  ResearchLoadArtifactResponse,
} from '../../lib/api/research';
import { Disclosure } from '../project-shell/Disclosure';
import { SafeMarkdownPreview } from './SafeMarkdownPreview';

type ResearchArtifactCardProps = {
  artifact: ResearchLoadArtifactResponse;
  onExport?: () => Promise<ResearchExportOutcome>;
};

export function ResearchArtifactCard({ artifact, onExport }: ResearchArtifactCardProps) {
  const [view, setView] = useState<'preview' | 'sources' | null>(null);
  const [exporting, setExporting] = useState(false);
  const [exportNotice, setExportNotice] = useState<string | null>(null);
  const [exportError, setExportError] = useState<string | null>(null);
  const exportButtonRef = useRef<HTMLButtonElement | null>(null);
  const restoreFocusRef = useRef(false);
  const verified = artifact.artifact.citationStatus === 'verified';

  useEffect(() => {
    if (!exporting && restoreFocusRef.current) {
      restoreFocusRef.current = false;
      exportButtonRef.current?.focus();
    }
  }, [exporting]);

  const runExport = async () => {
    if (onExport === undefined || exporting) return;
    setExporting(true);
    setExportNotice(null);
    setExportError(null);
    try {
      const outcome = await onExport();
      if (outcome.status === 'saved') setExportNotice(`Saved ${outcome.fileName}`);
    } catch (error) {
      setExportError(formatExportError(error));
    } finally {
      restoreFocusRef.current = true;
      setExporting(false);
    }
  };

  return (
    <section className="plume-research-artifact" aria-label="Research note">
      <div className="plume-research-artifact-header">
        <div>
          <strong>{verified ? 'Sources linked' : 'Draft — check citations'}</strong>
          <p>Links point to saved sources. Check relevance and accuracy yourself.</p>
        </div>
        <div className="plume-research-artifact-actions">
          <button
            type="button"
            className="ink-button"
            aria-expanded={view === 'preview'}
            onClick={() => setView((current) => current === 'preview' ? null : 'preview')}
          >
            {view === 'preview' ? 'Close note' : 'Open note'}
          </button>
          <button type="button" className="ink-button" aria-pressed={view === 'sources'} onClick={() => setView('sources')}>Sources</button>
          <button
            ref={exportButtonRef}
            type="button"
            className="ink-button"
            disabled={onExport === undefined || exporting}
            onClick={() => void runExport()}
          >
            {exporting ? 'Exporting…' : 'Export Markdown'}
          </button>
        </div>
      </div>
      {exportNotice !== null ? <p className="plume-research-export-notice" role="status">{exportNotice}</p> : null}
      {exportError !== null ? <p className="plume-research-export-error" role="alert">{exportError}</p> : null}
      {view === 'preview' ? (
        <SafeMarkdownPreview markdown={artifact.markdown} />
      ) : view === 'sources' ? (
        <ol className="plume-research-sources">
          {artifact.sources.map((source) => (
            <li key={source.sourceId}>
              <strong>{source.title ?? source.sourceId}</strong>
              <span>{source.sourceUrl}</span>
            </li>
          ))}
        </ol>
      ) : null}
      <Disclosure summary="Details" className="plume-research-details">
        <p>{artifact.logicalTurns} logical turns · {artifact.providerCalls} model calls · {artifact.durationMs} ms</p>
        {artifact.sources.map((source) => (
          <p key={source.sourceId}>{source.sourceId}: {source.sha256} · {source.bytes} bytes · {source.redactionCount} redactions{source.truncated ? ' · truncated' : ''}</p>
        ))}
      </Disclosure>
    </section>
  );
}

function formatExportError(error: unknown): string {
  if (isIpcError(error)) return ipcErrorMessage(error);
  if (error instanceof Error) return error.message;
  return 'The research note could not be exported.';
}
