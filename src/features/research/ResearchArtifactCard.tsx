import type { ResearchLoadArtifactResponse } from '../../lib/api/research';
import { Disclosure } from '../project-shell/Disclosure';
import { SafeMarkdownPreview } from './SafeMarkdownPreview';
import { useState } from 'react';

type ResearchArtifactCardProps = {
  artifact: ResearchLoadArtifactResponse;
  onExport?: () => void;
};

export function ResearchArtifactCard({ artifact, onExport }: ResearchArtifactCardProps) {
  const [view, setView] = useState<'preview' | 'sources'>('preview');
  const verified = artifact.artifact.citationStatus === 'verified';
  return (
    <section className="plume-research-artifact" aria-label="Research note">
      <div className="plume-research-artifact-header">
        <div>
          <strong>{verified ? 'Citations verified' : 'Draft — citations need review'}</strong>
          <p>Citation checks confirm source provenance. This does not verify relevance or factual accuracy.</p>
        </div>
        <div className="plume-research-artifact-actions">
          <button type="button" className="ink-button" aria-pressed={view === 'preview'} onClick={() => setView('preview')}>Preview</button>
          <button type="button" className="ink-button" aria-pressed={view === 'sources'} onClick={() => setView('sources')}>Sources</button>
          <button type="button" className="ink-button" disabled={onExport === undefined} onClick={onExport}>Export Markdown</button>
        </div>
      </div>
      {view === 'preview' ? (
        <SafeMarkdownPreview markdown={artifact.markdown} />
      ) : (
        <ol className="plume-research-sources">
          {artifact.sources.map((source) => (
            <li key={source.sourceId}>
              <strong>{source.title ?? source.sourceId}</strong>
              <span>{source.sourceUrl}</span>
            </li>
          ))}
        </ol>
      )}
      <Disclosure summary="Details" className="plume-research-details">
        <p>{artifact.logicalTurns} logical turns · {artifact.providerCalls} model calls · {artifact.durationMs} ms</p>
        {artifact.sources.map((source) => (
          <p key={source.sourceId}>{source.sourceId}: {source.sha256} · {source.bytes} bytes · {source.redactionCount} redactions{source.truncated ? ' · truncated' : ''}</p>
        ))}
      </Disclosure>
    </section>
  );
}
