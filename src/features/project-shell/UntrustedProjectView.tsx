import { useState } from 'react';

import type { ProjectMeta } from '../../lib/api/project';
import { BrowserPanel } from '../browser/BrowserPanel';
import { ProjectMetaPanel } from './ProjectMetaPanel';

export function UntrustedProjectView({
  meta,
  onTrust,
  onClose,
}: {
  meta: ProjectMeta;
  onTrust: (root: string) => void;
  onClose: () => void;
}) {
  const [activeView, setActiveView] = useState<'project-safety' | 'browser'>(
    'project-safety',
  );
  return (
    <section className="plume-project">
      <header className="plume-unified-topbar">
        <div className="plume-unified-brand">
          <h2 className="plume-unified-title">Plume</h2>
          <span className="plume-unified-subtitle">
            {activeView === 'browser' ? 'Browser' : 'Project safety'}
          </span>
        </div>
        <button
          type="button"
          className="ink-button"
          onClick={() =>
            setActiveView((current) =>
              current === 'browser' ? 'project-safety' : 'browser',
            )
          }
        >
          {activeView === 'browser' ? 'Project safety' : 'Open Browser'}
        </button>
      </header>
      {activeView === 'browser' ? (
        <BrowserPanel />
      ) : (
        <>
          <TrustBanner root={meta.root} onTrust={onTrust} />
          <ProjectMetaPanel meta={meta} onClose={onClose} />
        </>
      )}
    </section>
  );
}

function TrustBanner({
  root,
  onTrust,
}: {
  root: string;
  onTrust: (root: string) => void;
}) {
  return (
    <div className="plume-trust-banner ink-panel" role="alert">
      <div>
        <strong>Plume hasn&apos;t seen this project before.</strong>
        <p>
          File browsing and git status are gated until you trust this project. Trust is
          stored per-machine and keyed on the canonical path; renaming or moving the
          folder re-prompts.
        </p>
      </div>
      <button type="button" className="ink-button" onClick={() => onTrust(root)}>
        Trust this project
      </button>
    </div>
  );
}
