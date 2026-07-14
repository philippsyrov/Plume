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
import { TaskBrowserWorkspace } from './features/browser/TaskBrowserWorkspace';
import { ChatPanel } from './features/chat/ChatPanel';
import { describeAttachCandidate } from './features/chat/AttachBar';
import { ContextDropSurface } from './features/chat/ContextDropSurface';
import { contextSourceKey } from './features/chat/contextSources';
import { KnowledgePanel } from './features/knowledge/KnowledgePanel';
import { useSelectedModel } from './features/model-picker/useSelectedModel';
import { OpenForm } from './features/project-shell/OpenForm';
import { ToolDrawer } from './features/project-shell/ToolDrawer';
import { UntrustedProjectView } from './features/project-shell/UntrustedProjectView';
import {
  HelpPanel,
  NoProjectSettingsModal,
  OpenProjectModal,
  ProjectSettingsModal,
  UnifiedTopBar,
  topbarSubtitle,
  useSidebarPreference,
} from './features/project-shell/UnifiedChrome';
import {
  UnifiedSidebar,
  type ProjectWorkspaceView,
} from './features/project-shell/UnifiedSidebar';
import { lastSegment } from './features/project-shell/projectName';
import { useSessionDialogs } from './features/sessions/SessionDialogs';
import { SessionNotices } from './features/sessions/SessionNotices';
import { SessionSearchOverlay, useSearchShortcut } from './features/sessions/SessionSearch';
import { usePersistedChat } from './features/sessions/usePersistedChat';
import { useSessions } from './features/sessions/useSessions';
import type { ContextSourceRef } from './lib/api/chat';
import type { SessionIdentity } from './lib/api/sessions';

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

  // Keep the MLX supervisor window-scoped so changing views does not stop
  // live servers. Model selection stays view-scoped because each view has
  // its own user intent.
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
  // `busy` (open form). Trusted, untrusted, and no-project
  // surfaces each own one compact top strip, so the global hero
  // stays hidden there instead of repeating the Plume identity.
  const showHero = view.kind === 'idle' || view.kind === 'busy';

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
    return <UntrustedProjectView meta={meta} onTrust={onTrust} onClose={onClose} />;
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
  const [helpOpen, setHelpOpen] = useState(false);
  const [openProjectOpen, setOpenProjectOpen] = useState(false);
  const [toolDrawerOpen, setToolDrawerOpen] = useState(false);
  const [sidebarCollapsed, setSidebarCollapsed] = useSidebarPreference();
  // D63B: persisted chat sessions replace the D62 placeholder
  // title/seed state. One `useChat` instance (inside
  // `usePersistedChat`) backs both chat views, so switching sessions
  // while a stream is active is blocked — never silently detached.
  const sessions = useSessions({ projectAvailable: true });
  const persisted = usePersistedChat({ sessions, initialScope: 'project' });
  const dialogs = useSessionDialogs({
    sessions,
    persisted,
    onChatCreated: (scope) => {
      setActiveView(scope === 'local' ? 'local-chat' : 'project-chat');
      setToolDrawerOpen(false);
    },
  });
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
  const openBrowser = () => {
    void (async () => {
      const before = persisted.surfaceIdentity();
      if (before.sessionId === null) {
        const created = await persisted.startNewSession(before.scope);
        if (!created) return;
      }
      if (persisted.surfaceIdentity().sessionId === null) return;
      setActiveView('browser');
      setToolDrawerOpen(false);
    })();
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
  const useBrowserContextInChat = async (owner: SessionIdentity, source: ContextSourceRef) => {
    const before = persisted.surfaceIdentity();
    if (before.scope !== owner.scope || before.sessionId !== owner.sessionId) return 'unavailable' as const;
    const result = persisted.chat.addContextSource(source);
    const after = persisted.surfaceIdentity();
    if (after.scope !== owner.scope || after.sessionId !== owner.sessionId) return 'unavailable' as const;
    if (result === 'added' || result === 'duplicate') {
      setContextEmphasis((previous) => ({
        key: contextSourceKey(source),
        generation: (previous?.generation ?? 0) + 1,
      }));
    }
    return result;
  };
  const openSettings = () => {
    setSettingsOpen(true);
    setToolDrawerOpen(false);
  };
  const openHelp = () => {
    setHelpOpen(true);
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
        collapsed={sidebarCollapsed} onCollapsedChange={setSidebarCollapsed}
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
        onLibrary={openKnowledge} onSettings={openSettings}
        onHelp={openHelp}
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
          showTools
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
        ) : activeView === 'browser' && persisted.activeSessionId ? (
          <TaskBrowserWorkspace
            key={`browser-${persisted.activeScope}-${persisted.activeSessionId}`}
            identity={{ scope: persisted.activeScope, sessionId: persisted.activeSessionId }}
            onUseInChat={useBrowserContextInChat}
            chatProps={{
              chat: persisted.chat, selected, onClearSelection: clear,
              inspectorSelection: persisted.activeScope === 'project' ? navigatorState.selection : null,
              inspectorLineRange: persisted.activeScope === 'project' ? navigatorState.currentLineRange : null,
              projectHasInstructions: persisted.activeScope === 'project' && meta.hasAgentsMd,
              mlxServers, includeProjectContext: persisted.activeScope === 'project',
              variant: 'simple', emphasizedContextKey: contextEmphasis?.key ?? null,
            }}
          />
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
              {...(persisted.activeSessionId
                ? {
                    contextOwner: {
                      scope: 'local' as const,
                      sessionId: persisted.activeSessionId,
                    },
                  }
                : {})}
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
              {...(persisted.activeSessionId
                ? {
                    contextOwner: {
                      scope: 'project' as const,
                      sessionId: persisted.activeSessionId,
                    },
                  }
                : {})}
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
          onBrowser={openBrowser}
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
      {helpOpen ? <HelpPanel onClose={() => setHelpOpen(false)} /> : null}
      {openProjectOpen ? (
        <OpenProjectModal
          onOpen={onOpen}
          onClose={() => setOpenProjectOpen(false)}
        />
      ) : null}
    </section>
  );
}

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
  const [helpOpen, setHelpOpen] = useState(false);
  const [openProjectOpen, setOpenProjectOpen] = useState(false);
  const [activeView, setActiveView] = useState<ProjectWorkspaceView>('local-chat');
  const [toolDrawerOpen, setToolDrawerOpen] = useState(false);
  const [sidebarCollapsed, setSidebarCollapsed] = useSidebarPreference();
  // D63B: persisted local sessions. No project is open, so only the
  // local scope is available — the project database is untouchable
  // by construction here (the backend gate would reject it anyway).
  const sessions = useSessions({ projectAvailable: false });
  const persisted = usePersistedChat({ sessions, initialScope: 'local' });
  const dialogs = useSessionDialogs({
    sessions,
    persisted,
    onChatCreated: () => {
      setActiveView('local-chat');
      setToolDrawerOpen(false);
    },
  });
  // D66: chat search overlay (sidebar button or Cmd+K); local scope
  // only — no project database exists to search here.
  const [searchOpen, setSearchOpen] = useState(false);
  useSearchShortcut(() => setSearchOpen(true));
  const openSettings = () => {
    setSettingsOpen(true);
    setToolDrawerOpen(false);
  };
  const openHelp = () => {
    setHelpOpen(true);
    setToolDrawerOpen(false);
  };
  const openProjectModal = () => {
    setOpenProjectOpen(true);
    setToolDrawerOpen(false);
  };
  const openLocalChat = () => {
    setActiveView('local-chat');
    setToolDrawerOpen(false);
  };
  const openBrowser = () => {
    void (async () => {
      if (persisted.surfaceIdentity().sessionId === null) {
        const created = await persisted.startNewSession('local');
        if (!created) return;
      }
      if (persisted.surfaceIdentity().sessionId === null) return;
      setActiveView('browser');
      setToolDrawerOpen(false);
    })();
  };
  const useBrowserContextInChat = async (owner: SessionIdentity, source: ContextSourceRef) => {
    const before = persisted.surfaceIdentity();
    if (owner.scope !== 'local' || before.scope !== owner.scope || before.sessionId !== owner.sessionId) return 'unavailable' as const;
    const result = persisted.chat.addContextSource(source);
    const after = persisted.surfaceIdentity();
    if (after.scope !== owner.scope || after.sessionId !== owner.sessionId) return 'unavailable' as const;
    return result;
  };
  return (
    <section className="plume-project plume-project-codex plume-unified-shell">
      <UnifiedSidebar
        projectName={null}
        trustLabel="local chat"
        activeView={activeView}
        settingsOpen={settingsOpen}
        localSessions={sessions.visibleOf('local')}
        projectSessions={[]}
        activeSessionId={persisted.activeSessionId}
        activeScope="local"
        hasArchivedLocal={sessions.archivedOf('local').length > 0}
        hasArchivedProject={false}
        collapsed={sidebarCollapsed} onCollapsedChange={setSidebarCollapsed}
        onSelectSession={(scope, sessionId) => {
          void persisted.selectSession(scope, sessionId).then((ok) => {
            if (ok) openLocalChat();
          });
        }}
        onNewLocalChat={() => {
          void persisted.startNewSession('local').then((ok) => {
            if (ok) openLocalChat();
          });
        }}
        onRenameSession={dialogs.openRename}
        onContinueSession={(scope, session) =>
          void persisted.continueInNewChat(scope, session.id).then((ok) => {
            if (ok) openLocalChat();
          })
        }
        onRewindSession={dialogs.openRewind}
        onArchiveSession={(scope, session) =>
          void sessions.setArchived(scope, session.id, true)
        }
        onDeleteSession={dialogs.openDelete}
        onShowArchived={dialogs.openArchived}
        onSearch={() => setSearchOpen(true)}
        onLibrary={() => undefined} onSettings={openSettings}
        onHelp={openHelp}
        onOpenProject={openProjectModal}
      />
      <div className="plume-project-main">
        <UnifiedTopBar
          subtitle={topbarSubtitle(activeView, null)}
          inventory={inventory}
          servers={mlxServers}
          selected={selected}
          onSelect={select}
          toolsOpen={toolDrawerOpen}
          showTools
          showOpenProject={false}
          onToggleTools={() => setToolDrawerOpen((open) => !open)}
          onOpenProject={openProjectModal}
        />
        <SessionNotices notice={persisted.notice} saveError={persisted.saveError} />
        {activeView === 'browser' && persisted.activeSessionId ? (
          <TaskBrowserWorkspace
            key={`browser-local-${persisted.activeSessionId}`}
            identity={{ scope: 'local', sessionId: persisted.activeSessionId }}
            onUseInChat={useBrowserContextInChat}
            chatProps={{
              chat: persisted.chat, selected, onClearSelection: clear,
              inspectorSelection: null, inspectorLineRange: null,
              projectHasInstructions: false, mlxServers,
              includeProjectContext: false, variant: 'simple',
            }}
          />
        ) : (
        <section className="plume-no-project-chat" aria-label="Chat">
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
            {...(persisted.activeSessionId
              ? {
                  contextOwner: {
                    scope: 'local' as const,
                    sessionId: persisted.activeSessionId,
                  },
                }
              : {})}
          />
        </section>
        )}
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
      {toolDrawerOpen ? (
        <ToolDrawer
          hasProject={false}
          activeView={activeView}
          onChat={openLocalChat}
          onBrowser={openBrowser}
          onFiles={openProjectModal}
          onKnowledge={() => undefined}
          onBenchmarks={openProjectModal}
          onOpenProject={openProjectModal}
          onClose={() => setToolDrawerOpen(false)}
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
      {helpOpen ? <HelpPanel onClose={() => setHelpOpen(false)} /> : null}
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

function formatError(err: unknown): string {
  if (isIpcError(err)) return ipcErrorMessage(err);
  if (err instanceof Error) return err.message;
  return 'Unknown error.';
}
