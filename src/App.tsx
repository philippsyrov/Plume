import { useCallback, useState } from 'react';

import {
  openProject,
  trustProject,
  type ProjectMeta,
} from './lib/api/project';
import { ipcErrorMessage, isIpcError } from './lib/api/errors';

type View =
  | { kind: 'idle'; path: string }
  | { kind: 'busy'; path: string }
  | { kind: 'open'; meta: ProjectMeta };

export function App() {
  const [view, setView] = useState<View>({ kind: 'idle', path: '' });
  const [error, setError] = useState<string | null>(null);

  const onOpen = useCallback(async (path: string) => {
    setError(null);
    setView({ kind: 'busy', path });
    try {
      const meta = await openProject(path);
      setView({ kind: 'open', meta });
    } catch (err) {
      setError(formatError(err));
      setView({ kind: 'idle', path });
    }
  }, []);

  const onTrust = useCallback(async (root: string) => {
    setError(null);
    try {
      const meta = await trustProject(root);
      setView({ kind: 'open', meta });
    } catch (err) {
      setError(formatError(err));
    }
  }, []);

  const onClose = useCallback(() => {
    setView({ kind: 'idle', path: '' });
    setError(null);
  }, []);

  return (
    <main className="plume-shell">
      <header className="plume-header">
        <h1>Plume</h1>
        <p>A quiet local AI coding editor — early scaffold.</p>
      </header>

      {view.kind === 'open' ? (
        <ProjectView meta={view.meta} onTrust={onTrust} onClose={onClose} />
      ) : (
        <OpenForm
          path={view.path}
          busy={view.kind === 'busy'}
          onOpen={onOpen}
          onChange={(path) => setView({ kind: 'idle', path })}
        />
      )}

      {error ? (
        <p className="plume-error" role="alert">
          {error}
        </p>
      ) : null}
    </main>
  );
}

type OpenFormProps = {
  path: string;
  busy: boolean;
  onOpen: (path: string) => void;
  onChange: (path: string) => void;
};

function OpenForm({ path, busy, onOpen, onChange }: OpenFormProps) {
  const trimmed = path.trim();
  const canOpen = trimmed.length > 0 && !busy;
  return (
    <section className="plume-empty ink-panel">
      <p>
        Open a project folder to begin. Type or paste an absolute path —
        the file picker dialog plugin lands in a later slice.
      </p>
      <form
        className="plume-open-form"
        onSubmit={(e) => {
          e.preventDefault();
          if (canOpen) onOpen(trimmed);
        }}
      >
        <label className="plume-open-form-label">
          Project path
          <input
            type="text"
            className="plume-open-form-input"
            value={path}
            placeholder="/Users/you/code/some-project"
            spellCheck={false}
            autoCapitalize="off"
            autoCorrect="off"
            onChange={(e) => onChange(e.target.value)}
            disabled={busy}
          />
        </label>
        <button type="submit" className="ink-button" disabled={!canOpen}>
          {busy ? 'Opening…' : 'Open'}
        </button>
      </form>
    </section>
  );
}

type ProjectViewProps = {
  meta: ProjectMeta;
  onTrust: (root: string) => void;
  onClose: () => void;
};

function ProjectView({ meta, onTrust, onClose }: ProjectViewProps) {
  return (
    <section className="plume-project">
      {meta.trust === 'unknown' ? (
        <TrustBanner root={meta.root} onTrust={onTrust} />
      ) : null}

      <div className="plume-project-meta ink-panel">
        <header className="plume-project-meta-header">
          <h2>{lastSegment(meta.root)}</h2>
          <button type="button" className="ink-button" onClick={onClose}>
            Close
          </button>
        </header>

        <dl className="plume-meta-grid">
          <dt>Root</dt>
          <dd>
            <code>{meta.root}</code>
          </dd>

          <dt>Trust</dt>
          <dd>
            <span className={`ink-badge plume-trust-${meta.trust}`}>
              {meta.trust}
            </span>
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
              ? 'not a git repo'
              : `${meta.git.branch ?? '(detached)'}${
                  meta.git.dirtyCount > 0
                    ? ` · ${meta.git.dirtyCount} change${
                        meta.git.dirtyCount === 1 ? '' : 's'
                      }`
                    : ' · clean'
                }`}
          </dd>
        </dl>
      </div>
    </section>
  );
}

type TrustBannerProps = {
  root: string;
  onTrust: (root: string) => void;
};

function TrustBanner({ root, onTrust }: TrustBannerProps) {
  return (
    <div className="plume-trust-banner ink-panel" role="alert">
      <div>
        <strong>Plume hasn&apos;t seen this project before.</strong>
        <p>
          The editor is loaded read-only until you trust it. Trust is
          stored per-machine and keyed on the canonical path; renaming
          or moving the folder re-prompts.
        </p>
      </div>
      <button
        type="button"
        className="ink-button"
        onClick={() => onTrust(root)}
      >
        Trust this project
      </button>
    </div>
  );
}

function lastSegment(absolutePath: string): string {
  const trimmed = absolutePath.replace(/[/\\]+$/, '');
  const parts = trimmed.split(/[/\\]/);
  return parts[parts.length - 1] || absolutePath;
}

function formatError(err: unknown): string {
  if (isIpcError(err)) return ipcErrorMessage(err);
  if (err instanceof Error) return err.message;
  return 'Unknown error.';
}
