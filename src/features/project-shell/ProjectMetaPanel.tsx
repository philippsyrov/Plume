import type { ProjectMeta } from '../../lib/api/project';
import { Disclosure } from './Disclosure';

export function ProjectMetaPanel({
  meta,
}: {
  meta: ProjectMeta;
}) {
  return (
    <div className="plume-project-meta">
      <Disclosure summary="Technical details">
        <dl className="plume-meta-grid">
          <dt>Trust</dt>
          <dd>
            <span className={`ink-badge plume-trust-${meta.trust}`}>{meta.trust}</span>
          </dd>

          <dt>AGENTS.md</dt>
          <dd>{meta.hasAgentsMd ? 'present' : 'missing'}</dd>

          <dt>CLAUDE.md</dt>
          <dd>{meta.hasClaudeMd ? 'present' : 'missing'}</dd>

          <dt>Package managers</dt>
          <dd>
            {meta.packageManagers.length === 0
              ? '—'
              : meta.packageManagers.map((pm) => (
                  <span key={pm} className="ink-badge plume-pm-badge">
                    {pm}
                  </span>
                ))}
          </dd>

          <dt>Git</dt>
          <dd>
            {meta.git === null
              ? meta.trust === 'unknown'
                ? 'available after trust'
                : 'not a git repo'
              : `${meta.git.branch ?? '(detached)'}${
                  meta.git.dirtyCount > 0
                    ? ` · ${meta.git.dirtyCount} change${meta.git.dirtyCount === 1 ? '' : 's'}`
                    : ' · clean'
                }`}
          </dd>
        </dl>
      </Disclosure>
    </div>
  );
}
