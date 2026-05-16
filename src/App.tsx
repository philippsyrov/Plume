import { useCallback, useEffect, useRef, useState } from 'react';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import type { UnlistenFn } from '@tauri-apps/api/event';

import {
  openProject,
  trustProject,
  type ProjectMeta,
} from './lib/api/project';
import { ipcErrorMessage, isIpcError } from './lib/api/errors';
import {
  FileInspector,
  FileNavigator,
  useFileNavigator,
} from './features/file-tree/FileBrowser';
import { ProvidersPanel } from './features/providers/ProvidersPanel';
import { LocalModelsPanel } from './features/providers/LocalModelsPanel';
import { MemoryPanel } from './features/memory/MemoryPanel';
import { useProviderInventory } from './features/providers/useProviderInventory';
import { useMlxServers } from './features/providers/useMlxServers';
import { AgentWorkspace } from './features/agent/AgentWorkspace';
import { useSelectedModel } from './features/model-picker/useSelectedModel';
import { SystemChips } from './features/system/SystemChips';
import {
  EmptyColumn,
  InnerToggleStrip,
  PanelToggle,
  ResizeHandle,
  useInnerPanels,
  useWorkspaceLayout,
  workspaceGridTemplate,
  type InnerPanels,
  type WorkspaceLayout,
} from './features/workspace-layout';

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

  // D13: the global `Plume` hero is part of the open-project
  // affordance only. Once a project is open and trusted, the
  // compact status strip inside `TrustedView` is the top-of-
  // window identity and the hero would just steal vertical
  // real estate from the workspace. Keep the hero for `idle` /
  // `busy` (open form) and for the `unknown` trust gate (where
  // there's no other top-of-window header yet).
  const showHero = view.kind !== 'open' || view.meta.trust !== 'trusted';

  return (
    <main className={`plume-shell${showHero ? '' : ' plume-shell-compact'}`}>
      {showHero ? (
        <header className="plume-header">
          <h1>Plume</h1>
          <p>A quiet local AI coding editor — early scaffold.</p>
        </header>
      ) : null}

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

  // Drag-and-drop a folder onto the window to populate the path
  // input. Validation lives on the backend — `project.open` will
  // reject non-directory paths with a typed error, so we don't
  // pre-flight check here. See docs/AGENT_OPERABILITY.md: this is
  // the same surface a visual agent uses (drop a folder, then click
  // Open) — no automation-only IPC bypass.
  //
  // The listener is registered once and reads `busy` through a ref so
  // we don't tear down + re-register on every parent state flip. When
  // an open is in flight, drops are ignored — otherwise dropping
  // folder B while A is opening would move the view back to idle and
  // then jump back to A when its request resolves.
  const busyRef = useRef(busy);
  useEffect(() => {
    busyRef.current = busy;
  }, [busy]);
  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    let cancelled = false;
    getCurrentWebview()
      .onDragDropEvent((event) => {
        if (busyRef.current) return;
        if (event.payload.type !== 'drop') return;
        const first = event.payload.paths[0];
        if (!first) return;
        onChange(first);
      })
      .then((fn) => {
        if (cancelled) {
          fn();
        } else {
          unlisten = fn;
        }
      })
      .catch((err) => {
        console.error(
          'OpenForm: drag-drop listener registration failed:',
          err instanceof Error ? err.message : String(err),
        );
      });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [onChange]);

  return (
    <section className="plume-empty ink-panel">
      <p>
        Open a project folder to begin. Type or paste an absolute path,
        or drag a folder onto this window. The file picker dialog plugin
        lands in a later slice.
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
  if (meta.trust === 'unknown') {
    return <UntrustedView meta={meta} onTrust={onTrust} onClose={onClose} />;
  }
  return <TrustedView meta={meta} onClose={onClose} />;
}

function UntrustedView({ meta, onTrust, onClose }: ProjectViewProps) {
  return (
    <section className="plume-project">
      <TrustBanner root={meta.root} onTrust={onTrust} />
      <ProjectMetaPanel meta={meta} onClose={onClose} />
    </section>
  );
}

function TrustedView({ meta, onClose }: { meta: ProjectMeta; onClose: () => void }) {
  // The hook owns directory + selection state. Splitting it here means
  // the navigator (left zone) and the inspector (right zone) read the
  // same state without prop drilling through the workspace shell.
  const navigatorState = useFileNavigator(meta.root);
  // D6: window-local selected-model state. Lives at this level so the
  // provider panel (left zone) drives it and the agent workspace
  // (center zone) reads it. Closing the project unmounts TrustedView
  // and drops the selection — that's the intended scope today.
  const { selected, select, clear } = useSelectedModel();
  // D30: workspace shell layout — column widths + show/hide state,
  // persisted to localStorage. The hook also registers Cmd+Shift+[
  // and Cmd+Shift+] for toggling the side panels.
  const layout = useWorkspaceLayout();
  // D32: per-column inner-panel visibility. Independent persistence
  // (`plume:inner-panels-v1`) from the outer layout so changing one
  // doesn't churn the other's storage key.
  const innerPanels = useInnerPanels();
  // D32: provider inventory hook is called ONCE here, even though
  // two panels (Providers, Local models) read from it. That keeps
  // the IPC load constant regardless of which combination of panels
  // is currently visible — and avoids re-fetching when a user hides
  // and then re-shows one of them.
  const inventory = useProviderInventory();
  // D46: per-model MLX server lifecycle. Hoisted here so both the
  // Local models panel (Start/Stop UI) and the agent workspace
  // (chat dispatch — looks up the handle for the selected model)
  // read the same source of truth.
  const mlxServers = useMlxServers();
  return (
    <section className="plume-project">
      <ProjectStatusStrip meta={meta} onClose={onClose} layout={layout} />
      <div
        className="plume-workspace"
        aria-label="Project workspace"
        style={{ gridTemplateColumns: workspaceGridTemplate(layout) }}
      >
        {layout.leftVisible ? (
          <>
            <div className="plume-workspace-left">
              <InnerToggleStrip
                side="left"
                items={leftToggleItems(innerPanels)}
              />
              {innerPanels.leftAnyVisible ? (
                <>
                  {innerPanels.files ? <FileNavigator state={navigatorState} /> : null}
                  {innerPanels.providers ? (
                    <ProvidersPanel
                      inventory={inventory}
                      selected={selected}
                      onSelect={select}
                    />
                  ) : null}
                  {innerPanels.localModels ? (
                    <LocalModelsPanel
                      inventory={inventory}
                      servers={mlxServers}
                      selected={selected}
                      onSelect={select}
                    />
                  ) : null}
                  {innerPanels.memory ? <MemoryPanel /> : null}
                </>
              ) : (
                <EmptyColumn side="left" />
              )}
            </div>
            <ResizeHandle
              edge="left"
              current={layout.leftWidth}
              min={layout.LEFT_MIN}
              max={layout.leftMax}
              onChange={layout.setLeftWidth}
              ariaLabel="Resize left panel"
            />
          </>
        ) : null}
        <div className="plume-workspace-center">
          <AgentWorkspace
            selected={selected}
            onClearSelection={clear}
            inspectorSelection={navigatorState.selection}
            inspectorLineRange={navigatorState.currentLineRange}
            projectHasInstructions={meta.hasAgentsMd}
            mlxServers={mlxServers}
          />
        </div>
        {layout.rightVisible ? (
          <>
            <ResizeHandle
              edge="right"
              current={layout.rightWidth}
              min={layout.RIGHT_MIN}
              max={layout.rightMax}
              onChange={layout.setRightWidth}
              ariaLabel="Resize right panel"
            />
            <div className="plume-workspace-right">
              <InnerToggleStrip
                side="right"
                items={rightToggleItems(innerPanels)}
              />
              {innerPanels.rightAnyVisible ? (
                <>
                  {innerPanels.inspector ? <FileInspector state={navigatorState} /> : null}
                </>
              ) : (
                <EmptyColumn side="right" />
              )}
            </div>
          </>
        ) : null}
      </div>
    </section>
  );
}

/// D32: builders for the chip-strip items inside each column. Kept
/// as small helpers next to the shell so the order, labels, and
/// shape live in one place — adding a future panel (Diff / Preview)
/// is a one-line insertion here plus a field on `useInnerPanels`.
function leftToggleItems(p: InnerPanels) {
  return [
    { id: 'files', label: 'Files', visible: p.files, onToggle: p.toggleFiles },
    {
      id: 'providers',
      label: 'Providers',
      visible: p.providers,
      onToggle: p.toggleProviders,
    },
    {
      id: 'local-models',
      label: 'Local models',
      visible: p.localModels,
      onToggle: p.toggleLocalModels,
    },
    {
      id: 'memory',
      label: 'Memory',
      visible: p.memory,
      onToggle: p.toggleMemory,
    },
  ];
}

function rightToggleItems(p: InnerPanels) {
  return [
    {
      id: 'inspector',
      label: 'Inspector',
      visible: p.inspector,
      onToggle: p.toggleInspector,
    },
  ];
}

type ProjectMetaPanelProps = {
  meta: ProjectMeta;
  onClose: () => void;
};

function ProjectMetaPanel({ meta, onClose }: ProjectMetaPanelProps) {
  return (
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
    </div>
  );
}

type ProjectStatusStripProps = {
  meta: ProjectMeta;
  onClose: () => void;
  layout: WorkspaceLayout;
};

function ProjectStatusStrip({ meta, onClose, layout }: ProjectStatusStripProps) {
  const gitText =
    meta.git === null
      ? 'no git'
      : `${meta.git.branch ?? '(detached)'}${
          meta.git.dirtyCount > 0 ? ` · ${meta.git.dirtyCount}Δ` : ''
        }`;
  return (
    <header className="plume-status-strip ink-panel">
      <div className="plume-status-info">
        <strong>{lastSegment(meta.root)}</strong>
        <span className="plume-status-detail" title={meta.root}>
          {meta.root}
        </span>
      </div>
      <div className="plume-status-meta">
        <span className="ink-badge plume-trust-trusted">trusted</span>
        <span className="ink-badge">{gitText}</span>
        {meta.packageManagers.map((pm) => (
          <span key={pm} className="ink-badge plume-pm-badge">
            {pm}
          </span>
        ))}
        <SystemChips />
        {/* D30: side-panel toggles. The buttons live next to the
            Close action so the panel-control affordances stay in
            one cluster at the top of the window. */}
        <PanelToggle side="left" visible={layout.leftVisible} onToggle={layout.toggleLeft} />
        <PanelToggle
          side="right"
          visible={layout.rightVisible}
          onToggle={layout.toggleRight}
        />
        <button type="button" className="ink-button" onClick={onClose}>
          Close
        </button>
      </div>
    </header>
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
