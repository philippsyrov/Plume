import type { ProjectMeta } from '../../lib/api/project';
import { Disclosure } from './Disclosure';
import { ProjectMetaPanel } from './ProjectMetaPanel';
import { lastSegment } from './projectName';

export function UntrustedProjectView({
  meta,
  onTrust,
  onClose,
}: {
  meta: ProjectMeta;
  onTrust: (root: string) => void;
  onClose: () => void;
}) {
  const projectName = lastSegment(meta.root);

  return (
    <section className="plume-project plume-project-untrusted">
      <header
        className="plume-unified-topbar"
        data-tauri-drag-region="true"
        aria-hidden="true"
      />
      <main className="plume-trust-stage">
        <div className="plume-trust-decision ink-panel">
          <div className="plume-trust-heading">
            <h1>Open {projectName}?</h1>
            <code title={meta.root}>{meta.root}</code>
          </div>

          <p className="plume-trust-intro">
            Plume needs your trust before it can read this folder.
          </p>

          <Disclosure summary="What does trust allow?" className="plume-trust-details">
            <p>
              Trust lets Plume read eligible files in this folder and use its project-scoped
              memory and instructions.
            </p>
            <p>
              Changes still require you to choose Apply. Moving or renaming the folder asks
              again.
            </p>
          </Disclosure>

          <ProjectMetaPanel meta={meta} />

          <div className="plume-trust-actions">
            <button type="button" className="ink-button" onClick={onClose}>
              Cancel
            </button>
            <button
              type="button"
              className="ink-button plume-trust-primary"
              onClick={() => onTrust(meta.root)}
            >
              Trust and open
            </button>
          </div>
        </div>
      </main>
    </section>
  );
}
