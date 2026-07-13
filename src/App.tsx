import { useCallback, useEffect, useState } from 'react';

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
import type { AgentMode } from './lib/api/session';
import { useProviderInventory } from './features/providers/useProviderInventory';
import { useMlxServers, type MlxServersApi } from './features/providers/useMlxServers';
import { BenchmarksPanel } from './features/benchmarks/BenchmarksPanel';
import { ChatPanel } from './features/chat/ChatPanel';
import { describeAttachCandidate } from './features/chat/AttachBar';
import { ContextDropSurface } from './features/chat/ContextDropSurface';
import { contextSourceKey } from './features/chat/contextSources';
import { KnowledgePanel } from './features/knowledge/KnowledgePanel';
import { useSelectedModel } from './features/model-picker/useSelectedModel';
import { OpenForm } from './features/project-shell/OpenForm';
import { ToolDrawer } from './features/project-shell/ToolDrawer';
import {
  NoProjectSettingsModal,
  OpenProjectModal,
  ProjectSettingsModal,
  UnifiedTopBar,
  topbarSubtitle,
} from './features/project-shell/UnifiedChrome';
import {
  UnifiedSidebar,
  type ProjectWorkspaceView,
} from './features/project-shell/UnifiedSidebar';
import { useSessionDialogs } from './features/sessions/SessionDialogs';
import { SessionNotices } from './features/sessions/SessionNotices';
import { SessionSearchOverlay, useSearchShortcut } from './features/sessions/SessionSearch';
import { usePersistedChat } from './features/sessions/usePersistedChat';
import { useSessions } from './features/sessions/useSessions';
import type { ContextSourceRef } from './lib/api/chat';

type View =
  | { kind: 'idle'; path: string }
  | { kind: 'busy'; path: string }
  | { kind: 'open'; meta: ProjectMeta }
  // D49: no-project chat. Plume as a local chat client before
  // (or without) opening a project. File tree / inspector /
  // patch / AGENTS.md / memory / attachments stay disabled —
  // this slice is just chat against Ollama and Plume-managed
  // MLX (the latter still gated on a trusted project for
  // `providers.startServer`, so the Start button stays disabled
  // here with a hint).
  | { kind: 'chat-only' };

export function App() {
  const [view, setView] = useState<View>({ kind: 'chat-only' });
  const [error, setError] = useState<string | null>(null);
  const [openingPath, setOpeningPath] = useState<string | null>(null);

  // D49 Codex MEDIUM fix: hoist the MLX-server lifecycle bus to
  // App scope so it survives `View` transitions. Pre-fix the hook
  // lived inside both `TrustedView` and `NoProjectChatView`, and
  // D46's `useMlxServers` unmount cleanup fires
  // `providers.stopServer` for every running handle when its host
  // tears down. With two separate hooks, jumping from a trusted
  // project (where the user just started an MLX server) to
  // no-project chat unmounted the first hook, stopped every live
  // handle, then mounted the second hook with an empty registry
  // snapshot — the claim that "already-running servers stay
  // reachable" was false. Hoisting the hook here means cleanup
  // only runs when the App itself unmounts (window close /
  // quit), which matches the supervisor's process-wide registry
  // and the user's "I started a server, don't kill it just
  // because I switched views" mental model.
  //
  // Selection state (`useSelectedModel`) stays view-scoped on
  // purpose: leaving a trusted project session shouldn't carry
  // the previously selected model into no-project chat (the
  // user's intent is different on each side). The MLX bus is
  // window-scoped because the underlying registry is, too.
  const mlxServers = useMlxServers();

  const onOpen = useCallback(async (path: string) => {
    setError(null);
    setOpeningPath(path);
    try {
      const meta = await openProject(path);
      setView({ kind: 'open', meta });
    } catch (err) {
      setError(formatError(err));
      setView((current) =>
        current.kind === 'idle' || current.kind === 'busy'
          ? { kind: 'idle', path }
          : current,
      );
    } finally {
      setOpeningPath(null);
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
    setView({ kind: 'chat-only' });
    setError(null);
  }, []);

  // D49: jump straight to no-project chat from the open form.
  // Closing the no-project view returns to the open form so the
  // user can pick a project after deciding to commit.
  const onChatOnly = useCallback(() => {
    setError(null);
    setView({ kind: 'chat-only' });
  }, []);

  // D13: the global `Plume` hero is part of the open-project
  // affordance only. Once a project is open and trusted, the
  // compact status strip inside `TrustedView` is the top-of-
  // window identity and the hero would just steal vertical
  // real estate from the workspace. Keep the hero for `idle` /
  // `busy` (open form) and for the `unknown` trust gate (where
  // there's no other top-of-window header yet).
  // D49: the no-project chat surface owns its own top strip,
  // so hide the global hero there too.
  const showHero =
    view.kind === 'chat-only'
      ? false
      : view.kind !== 'open' || view.meta.trust !== 'trusted';

  return (
    <main className={`plume-shell${showHero ? '' : ' plume-shell-compact'}`}>
      {showHero ? (
        <header className="plume-header">
          <h1>Plume</h1>
          <p>A quiet local AI coding editor — early scaffold.</p>
        </header>
      ) : null}

      {view.kind === 'open' ? (
        <ProjectView
          // D63B (Codex P1): key by root so opening a DIFFERENT project
          // remounts the whole project shell. Session lists, the loaded
          // transcript, dialogs, and drafts all reset — project A's
          // chats can never stay visible while the backend scope
          // already points at project B. Matches the backend, where
          // every `project.open` is a fresh session; the app-scoped
          // MLX bus deliberately survives (it lives above this key).
          key={view.meta.root}
          meta={view.meta}
          onTrust={onTrust}
          onClose={onClose}
          onOpen={onOpen}
          mlxServers={mlxServers}
        />
      ) : view.kind === 'chat-only' ? (
        <NoProjectChatView
          onOpen={onOpen}
          openingPath={openingPath}
          mlxServers={mlxServers}
        />
      ) : (
        <OpenForm
          path={view.path}
          busy={openingPath !== null}
          onOpen={onOpen}
          onChange={(path) => setView({ kind: 'idle', path })}
          onChatOnly={onChatOnly}
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

type ProjectViewProps = {
  meta: ProjectMeta;
  onTrust: (root: string) => void;
  onClose: () => void;
  onOpen: (path: string) => void;
  /** D49 Codex MEDIUM fix: the MLX-server bus is App-scoped now
   *  so it survives transitions to / from no-project chat. */
  mlxServers: MlxServersApi;
};

function ProjectView({ meta, onTrust, onClose, onOpen, mlxServers }: ProjectViewProps) {
  if (meta.trust === 'unknown') {
    // UntrustedView doesn't surface the MLX panel — the bus is
    // still alive at the App level, just not visible here.
    return <UntrustedView meta={meta} onTrust={onTrust} onClose={onClose} />;
  }
  return (
    <TrustedView
      meta={meta}
      onClose={onClose}
      onOpen={onOpen}
      mlxServers={mlxServers}
    />
  );
}

function UntrustedView({
  meta,
  onTrust,
  onClose,
}: Omit<ProjectViewProps, 'mlxServers' | 'onOpen'>) {
  return (
    <section className="plume-project">
      <TrustBanner root={meta.root} onTrust={onTrust} />
      <ProjectMetaPanel meta={meta} onClose={onClose} />
    </section>
  );
}

function TrustedView({
  meta,
  onClose,
  onOpen,
  mlxServers,
}: {
  meta: ProjectMeta;
  onClose: () => void;
  onOpen: (path: string) => void;
  mlxServers: MlxServersApi;
}) {
  // The hook owns directory + selection state. Splitting it here means
  // the navigator (left zone) and the inspector (right zone) read the
  // same state without prop drilling through the workspace shell.
  const navigatorState = useFileNavigator(meta.root);
  // D6: window-local selected-model state. Lives at this level so the
  // provider panel (left zone) drives it and the agent workspace
  // (center zone) reads it. Closing the project unmounts TrustedView
  // and drops the selection — that's the intended scope today.
  const { selected, select, clear } = useSelectedModel();
  const [agentMode, setAgentMode] = useState<AgentMode | null>(null);
  // D32: provider inventory hook is called ONCE here, even though
  // two panels (Providers, Local models) read from it. That keeps
  // the IPC load constant regardless of which combination of panels
  // is currently visible — and avoids re-fetching when a user hides
  // and then re-shows one of them.
  const inventory = useProviderInventory();
  // D46: per-model MLX server lifecycle. Passed in from `App`
  // (D49 Codex MEDIUM fix) so the bus survives view transitions —
  // a server the user starts inside TrustedView stays reachable
  // when they jump to no-project chat, and vice versa.
  const [activeView, setActiveView] = useState<ProjectWorkspaceView>('project-chat');
  const [contextEmphasis, setContextEmphasis] = useState<{
    key: string;
    generation: number;
  } | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [openProjectOpen, setOpenProjectOpen] = useState(false);
  const [toolDrawerOpen, setToolDrawerOpen] = useState(false);
  // D63B: persisted chat sessions replace the D62 placeholder
  // title/seed state. One `useChat` instance (inside
  // `usePersistedChat`) backs both chat views, so switching sessions
  // while a stream is active is blocked — never silently detached.
  const sessions = useSessions({ projectAvailable: true });
  const persisted = usePersistedChat({ sessions, initialScope: 'project' });
  const dialogs = useSessionDialogs({ sessions, persisted });
  // D66: chat search overlay (sidebar button or Cmd+K).
  const [searchOpen, setSearchOpen] = useState(false);
  useSearchShortcut(() => setSearchOpen(true));
  useEffect(() => {
    if (contextEmphasis === null) return;
    const timeout = window.setTimeout(() => setContextEmphasis(null), 900);
    return () => window.clearTimeout(timeout);
  }, [contextEmphasis]);
  const chatViewOf = (scope: 'local' | 'project'): ProjectWorkspaceView =>
    scope === 'local' ? 'local-chat' : 'project-chat';
  const selectSession = (scope: 'local' | 'project', sessionId: string) => {
    void persisted.selectSession(scope, sessionId).then((ok) => {
      if (!ok) return;
      setActiveView(chatViewOf(scope));
      setToolDrawerOpen(false);
    });
  };
  const searchSelect = (scope: 'local' | 'project', sessionId: string) =>
    persisted.selectSession(scope, sessionId).then((ok) => {
      if (ok) {
        setActiveView(chatViewOf(scope));
        setToolDrawerOpen(false);
      }
      return ok;
    });
  const newChat = (scope: 'local' | 'project') => {
    void persisted.startNewSession(scope).then((ok) => {
      if (!ok) return;
      setActiveView(chatViewOf(scope));
      setToolDrawerOpen(false);
    });
  };
  const continueChat = (scope: 'local' | 'project', sessionId: string) => {
    void persisted.continueInNewChat(scope, sessionId).then((ok) => {
      if (!ok) return;
      setActiveView(chatViewOf(scope));
      setToolDrawerOpen(false);
    });
  };
  const openProjectChat = () => {
    void persisted.openScope('project').then((ok) => {
      if (!ok) return;
      setActiveView('project-chat');
      setToolDrawerOpen(false);
    });
  };
  const openFiles = () => {
    setActiveView('files');
    setToolDrawerOpen(false);
  };
  const openBenchmarks = () => {
    setActiveView('benchmarks');
    setToolDrawerOpen(false);
  };
  const openKnowledge = () => {
    setActiveView('knowledge');
    setToolDrawerOpen(false);
  };
  const useContextInChat = async (source: ContextSourceRef) => {
    const opened = await persisted.openScope('project');
    if (!opened) return 'unavailable' as const;
    if (persisted.surfaceIdentity().scope !== 'project') return 'unavailable' as const;
    const result = persisted.chat.addContextSource(source);
    if (result === 'added' || result === 'duplicate') {
      setContextEmphasis((previous) => ({
        key: contextSourceKey(source),
        generation: (previous?.generation ?? 0) + 1,
      }));
      setActiveView('project-chat');
      setToolDrawerOpen(false);
    }
    return result;
  };
  const openSettings = () => {
    setSettingsOpen(true);
    setToolDrawerOpen(false);
  };
  const openProjectModal = () => {
    setOpenProjectOpen(true);
    setToolDrawerOpen(false);
  };
  const isLocalChatSurface = persisted.activeScope === 'local';
  const inspectorCandidate = describeAttachCandidate(
    navigatorState.selection,
    navigatorState.currentLineRange,
    null,
  );
  const inspectorContextSource: ContextSourceRef | null =
    inspectorCandidate.kind === 'eligible'
      ? inspectorCandidate.lineRange === null
        ? { kind: 'projectFile', relPath: inspectorCandidate.relPath }
        : {
            kind: 'projectFile',
            relPath: inspectorCandidate.relPath,
            startLine: inspectorCandidate.lineRange.startLine,
            endLine: inspectorCandidate.lineRange.endLine,
          }
      : null;
  return (
    <section className="plume-project plume-project-codex plume-unified-shell">
      <UnifiedSidebar
        projectName={lastSegment(meta.root)}
        trustLabel={meta.trust}
        activeView={activeView}
        settingsOpen={settingsOpen}
        localSessions={sessions.visibleOf('local')}
        projectSessions={sessions.visibleOf('project')}
        activeSessionId={persisted.activeSessionId}
        activeScope={persisted.activeScope}
        hasArchivedLocal={sessions.archivedOf('local').length > 0}
        hasArchivedProject={sessions.archivedOf('project').length > 0}
        onSelectSession={selectSession}
        onNewLocalChat={() => newChat('local')}
        onNewProjectChat={() => newChat('project')}
        onOpenProjectChat={openProjectChat}
        onRenameSession={dialogs.openRename}
        onContinueSession={(scope, session) => continueChat(scope, session.id)}
        onRewindSession={dialogs.openRewind}
        onArchiveSession={(scope, session) =>
          void sessions.setArchived(scope, session.id, true)
        }
        onDeleteSession={dialogs.openDelete}
        onShowArchived={dialogs.openArchived}
        onSearch={() => setSearchOpen(true)}
        onSettings={openSettings}
        onOpenProject={openProjectModal}
        onCloseProject={onClose}
      />
      <div className="plume-project-main">
        <UnifiedTopBar
          subtitle={topbarSubtitle(activeView, lastSegment(meta.root))}
          inventory={inventory}
          servers={mlxServers}
          selected={selected}
          onSelect={select}
          toolsOpen={toolDrawerOpen}
          showTools={activeView !== 'local-chat'}
          showOpenProject
          onToggleTools={() => setToolDrawerOpen((open) => !open)}
          onOpenProject={openProjectModal}
        />
        <SessionNotices notice={persisted.notice} saveError={persisted.saveError} />
        {activeView === 'files' ? (
          <ContextDropSurface
            onDropSource={useContextInChat}
            disabled={persisted.chat.status === 'streaming'}
          >
            {({ onDragActiveChange }) => (
              <div className="plume-project-files-view">
                <FileNavigator state={navigatorState} />
                <FileInspector
                  state={navigatorState}
                  contextSource={inspectorContextSource}
                  onUseInChat={useContextInChat}
                  onContextDragActiveChange={onDragActiveChange}
                />
              </div>
            )}
          </ContextDropSurface>
        ) : activeView === 'benchmarks' ? (
          <div className="plume-project-benchmarks-view">
            {/* D132: read-only viewer over benchmark-artifacts/ and
                benchmarks/catalog/ of THIS project. Trusted-only by
                construction — the fs verbs it reads through refuse
                without a trusted open project. */}
            <BenchmarksPanel />
          </div>
        ) : activeView === 'knowledge' ? (
          <ContextDropSurface
            onDropSource={useContextInChat}
            disabled={persisted.chat.status === 'streaming'}
          >
            {({ onDragActiveChange }) => (
              <div className="plume-project-knowledge-view">
                <KnowledgePanel
                  onUseInChat={useContextInChat}
                  onContextDragActiveChange={onDragActiveChange}
                />
              </div>
            )}
          </ContextDropSurface>
        ) : isLocalChatSurface ? (
          <section className="plume-project-chat-view" aria-label="Local chat">
            {/* Local chat inside a project window stays a SIMPLE chat:
                no inspector attachment, no AGENTS.md instructions, no
                project context folded into sends — same boundary as
                the no-project surface (D63 spec: simple chats never
                expose project capabilities). */}
            <ChatPanel
              key={`local-${persisted.activeSessionId ?? 'empty'}`}
              chat={persisted.chat}
              selected={selected}
              onClearSelection={clear}
              inspectorSelection={null}
              inspectorLineRange={null}
              projectHasInstructions={false}
              mlxServers={mlxServers}
              includeProjectContext={false}
              variant="simple"
            />
          </section>
        ) : (
          <section className="plume-project-chat-view" aria-label="Project chat">
            <ChatPanel
              key={`project-${persisted.activeSessionId ?? 'empty'}`}
              chat={persisted.chat}
              selected={selected}
              onClearSelection={clear}
              inspectorSelection={navigatorState.selection}
              inspectorLineRange={navigatorState.currentLineRange}
              projectHasInstructions={meta.hasAgentsMd}
              mlxServers={mlxServers}
              variant="simple"
              emphasizedContextKey={contextEmphasis?.key ?? null}
            />
          </section>
        )}
      </div>
      {dialogs.node}
      {searchOpen ? (
        <SessionSearchOverlay
          projectAvailable
          notice={persisted.notice}
          onSelect={searchSelect}
          onClose={() => setSearchOpen(false)}
        />
      ) : null}
      {toolDrawerOpen ? (
        <ToolDrawer
          hasProject
          activeView={activeView}
          onChat={openProjectChat}
          onFiles={openFiles}
          onKnowledge={openKnowledge}
          onBenchmarks={openBenchmarks}
          onOpenProject={openProjectModal}
          onClose={() => setToolDrawerOpen(false)}
        />
      ) : null}
      {settingsOpen ? (
        <ProjectSettingsModal
          inventory={inventory}
          servers={mlxServers}
          selected={selected}
          onSelect={select}
          agentMode={agentMode}
          onAgentModeChange={setAgentMode}
          inspectorSelection={navigatorState.selection}
          inspectorLineRange={navigatorState.currentLineRange}
          onClose={() => setSettingsOpen(false)}
        />
      ) : null}
      {openProjectOpen ? (
        <OpenProjectModal
          onOpen={onOpen}
          onClose={() => setOpenProjectOpen(false)}
        />
      ) : null}
    </section>
  );
}

/// D49: no-project chat shell.
///
/// Reuses the provider / local-models / chat panels but skips
/// everything project-shaped: no file navigator, no inspector,
/// no Memory panel, no AGENTS.md badge, no attachment UI in
/// chat. Chat against Ollama works exactly like the project
/// flow today — the backend's `chat.send` already tolerates
/// `optional_trusted_open` returning `None` (no AGENTS.md, no
/// memory folded in, attachment field must be omitted).
///
/// Plume-managed MLX servers stay gated: `providers.startServer`
/// requires a trusted open project on the backend side (the
/// safety contract for spawning a Python subprocess). The
/// Local-models panel here passes `noProject` so the Start
/// button renders disabled with a "open a project to start"
/// hint instead of letting the user click into a `NeedsApproval`.
/// MLX servers the user already started in a trusted session
/// keep running and surface as `port N · Stop` rows here — the
/// `mlxServers` bus is App-scoped (D49 Codex MEDIUM fix), so
/// transitioning from TrustedView to NoProjectChatView no longer
/// tears down live handles. The bus only cleans up on App
/// unmount (window close / quit).
function NoProjectChatView({
  onOpen,
  openingPath,
  mlxServers,
}: {
  onOpen: (path: string) => void;
  openingPath: string | null;
  mlxServers: MlxServersApi;
}) {
  const { selected, select, clear } = useSelectedModel();
  const inventory = useProviderInventory();
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [openProjectOpen, setOpenProjectOpen] = useState(false);
  // D63B: persisted local sessions. No project is open, so only the
  // local scope is available — the project database is untouchable
  // by construction here (the backend gate would reject it anyway).
  const sessions = useSessions({ projectAvailable: false });
  const persisted = usePersistedChat({ sessions, initialScope: 'local' });
  const dialogs = useSessionDialogs({ sessions, persisted });
  // D66: chat search overlay (sidebar button or Cmd+K); local scope
  // only — no project database exists to search here.
  const [searchOpen, setSearchOpen] = useState(false);
  useSearchShortcut(() => setSearchOpen(true));
  const openSettings = () => {
    setSettingsOpen(true);
  };
  const openProjectModal = () => {
    setOpenProjectOpen(true);
  };
  return (
    <section className="plume-project plume-project-codex plume-unified-shell">
      <UnifiedSidebar
        projectName={null}
        trustLabel="local chat"
        activeView="local-chat"
        settingsOpen={settingsOpen}
        localSessions={sessions.visibleOf('local')}
        projectSessions={[]}
        activeSessionId={persisted.activeSessionId}
        activeScope="local"
        hasArchivedLocal={sessions.archivedOf('local').length > 0}
        hasArchivedProject={false}
        onSelectSession={(scope, sessionId) =>
          void persisted.selectSession(scope, sessionId)
        }
        onNewLocalChat={() => void persisted.startNewSession('local')}
        onRenameSession={dialogs.openRename}
        onContinueSession={(scope, session) =>
          void persisted.continueInNewChat(scope, session.id)
        }
        onRewindSession={dialogs.openRewind}
        onArchiveSession={(scope, session) =>
          void sessions.setArchived(scope, session.id, true)
        }
        onDeleteSession={dialogs.openDelete}
        onShowArchived={dialogs.openArchived}
        onSearch={() => setSearchOpen(true)}
        onSettings={openSettings}
        onOpenProject={openProjectModal}
      />
      <div className="plume-project-main">
        <UnifiedTopBar
          subtitle="Simple chat"
          inventory={inventory}
          servers={mlxServers}
          selected={selected}
          onSelect={select}
          toolsOpen={false}
          showTools={false}
          showOpenProject={false}
          onToggleTools={() => undefined}
          onOpenProject={openProjectModal}
        />
        <SessionNotices notice={persisted.notice} saveError={persisted.saveError} />
        <section className="plume-no-project-chat" aria-label="Chat">
          {/*
            ChatPanel already accepts `null` for inspector inputs and
            `false` for projectHasInstructions, so the same component
            renders the no-project shape verbatim — no attachment
            chip eligibility, no AGENTS.md badge, no Memory badge.
            The chat-send IPC tolerates the no-project case (the
            backend's `optional_trusted_open` returns None and the
            assembler skips the project-shaped sections).
          */}
          <ChatPanel
            key={`local-${persisted.activeSessionId ?? 'empty'}`}
            chat={persisted.chat}
            selected={selected}
            onClearSelection={clear}
            inspectorSelection={null}
            inspectorLineRange={null}
            projectHasInstructions={false}
            mlxServers={mlxServers}
            includeProjectContext={false}
            variant="simple"
          />
        </section>
      </div>
      {dialogs.node}
      {searchOpen ? (
        <SessionSearchOverlay
          projectAvailable={false}
          notice={persisted.notice}
          onSelect={(scope, sessionId) => persisted.selectSession(scope, sessionId)}
          onClose={() => setSearchOpen(false)}
        />
      ) : null}
      {settingsOpen ? (
        <NoProjectSettingsModal
          inventory={inventory}
          servers={mlxServers}
          selected={selected}
          onSelect={select}
          onClose={() => setSettingsOpen(false)}
        />
      ) : null}
      {openProjectOpen ? (
        <OpenProjectModal
          onOpen={onOpen}
          onClose={() => setOpenProjectOpen(false)}
        />
      ) : null}
      {openingPath ? (
        <div className="plume-unified-opening" role="status">
          Opening {openingPath}
        </div>
      ) : null}
    </section>
  );
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
