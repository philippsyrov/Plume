import { useCallback, useEffect, useRef, useState } from 'react';

import {
  closeProject,
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
import { useAppearance } from './features/appearance/useAppearance';
import { TaskBrowserWorkspace } from './features/browser/TaskBrowserWorkspace';
import { ChatPanel } from './features/chat/ChatPanel';
import { describeAttachCandidate } from './features/chat/AttachBar';
import { ContextDropSurface } from './features/chat/ContextDropSurface';
import { contextSourceKey } from './features/chat/contextSources';
import { HelpPanel } from './features/help/HelpPanel';
import { createLibraryChatHandoff } from './features/library/libraryChatHandoff';
import { LibraryWorkspace } from './features/library/LibraryWorkspace';
import {
  defaultModelCatalogDependencies,
  useModelCatalog,
} from './features/model-picker/useModelCatalog';
import { useSelectedModel } from './features/model-picker/useSelectedModel';
import { ModelChooserWorkspace } from './features/model-picker/ModelChooser';
import { OpenForm } from './features/project-shell/OpenForm';
import { NoProjectChatView } from './features/project-shell/NoProjectChatView';
import { ToolDrawer } from './features/project-shell/ToolDrawer';
import { UntrustedProjectView } from './features/project-shell/UntrustedProjectView';
import {
  OpenProjectView,
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
import {
  ArchivedSessionsSettings,
  useSessionDialogs,
} from './features/sessions/SessionDialogs';
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
  // Keep backend project mutations and their successful React commits in the
  // same order. The generation only suppresses errors/loading from stale intent.
  const projectTransitionGenerationRef = useRef(0);
  const projectTransitionTailRef = useRef<Promise<void>>(Promise.resolve());

  const enqueueProjectTransition = useCallback(<T,>(operation: () => Promise<T>) => {
    const generation = projectTransitionGenerationRef.current + 1;
    projectTransitionGenerationRef.current = generation;
    const result = projectTransitionTailRef.current.then(operation);
    projectTransitionTailRef.current = result.then(() => undefined, () => undefined);
    return { generation, result };
  }, []);

  const windowModels = useWindowModelState();
  const { mlxServers, selectedModel, modelCatalog } = windowModels;
  const appearance = useAppearance();

  const onOpen = useCallback(async (path: string): Promise<boolean> => {
    setError(null);
    setOpeningPath(path);
    const { generation, result } = enqueueProjectTransition(() => openProject(path));
    try {
      const meta = await result;
      setView({ kind: 'open', meta });
      return true;
    } catch (err) {
      if (generation !== projectTransitionGenerationRef.current) return false;
      setError(formatError(err));
      setView((current) =>
        current.kind === 'idle' || current.kind === 'busy'
          ? { kind: 'idle', path }
          : current,
      );
      return false;
    } finally {
      if (generation === projectTransitionGenerationRef.current) setOpeningPath(null);
    }
  }, [enqueueProjectTransition]);

  const onTrust = useCallback(async (root: string) => {
    setError(null);
    const { generation, result } = enqueueProjectTransition(() => trustProject(root));
    try {
      const meta = await result;
      setView({ kind: 'open', meta });
    } catch (err) {
      if (generation !== projectTransitionGenerationRef.current) return;
      setError(formatError(err));
    }
  }, [enqueueProjectTransition]);

  const onClose = useCallback(async () => {
    setError(null);
    setOpeningPath(null);
    const { generation, result } = enqueueProjectTransition(closeProject);
    try {
      await result;
      setView({ kind: 'chat-only' });
    } catch (err) {
      if (generation !== projectTransitionGenerationRef.current) return;
      setError(formatError(err));
    }
  }, [enqueueProjectTransition]);

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
          // Key by the backend's fresh project-session identity, not its path.
          // Reopening the same root is still a new generation, so session
          // lists, transcripts, dialogs, drafts, and workspace state must all
          // reset. The app-scoped MLX bus deliberately survives above this key.
          key={view.meta.id}
          meta={view.meta}
          onTrust={onTrust}
          onClose={onClose}
          onOpen={onOpen}
          mlxServers={mlxServers}
          selectedModel={selectedModel}
          modelCatalog={modelCatalog}
          appearance={appearance}
        />
      ) : view.kind === 'chat-only' ? (
        <NoProjectChatView
          onOpen={onOpen}
          openingPath={openingPath}
          mlxServers={mlxServers}
          selectedModel={selectedModel}
          modelCatalog={modelCatalog}
          appearance={appearance}
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

/**
 * App-level ownership seam for the next selector surface. The returned
 * catalog API stays alive while local/project shells mount and unmount, just
 * like the managed-handle and selected-model APIs it coordinates.
 */
export function useWindowModelState() {
  const mlxServers = useMlxServers();
  const selectedModel = useSelectedModel();
  const modelCatalog = useModelCatalog({
    ...defaultModelCatalogDependencies,
    mlxServers,
    selectedModel,
  });
  return { mlxServers, selectedModel, modelCatalog };
}

type ProjectViewProps = {
  meta: ProjectMeta;
  onTrust: (root: string) => void;
  onClose: () => void;
  onOpen: (path: string) => Promise<boolean>;
  /** D49 Codex MEDIUM fix: the MLX-server bus is App-scoped now
   *  so it survives transitions to / from no-project chat. */
  mlxServers: MlxServersApi;
  /** Selection is window-scoped with the catalog and MLX handle map. */
  selectedModel: ReturnType<typeof useSelectedModel>;
  modelCatalog: ReturnType<typeof useModelCatalog>;
  appearance: ReturnType<typeof useAppearance>;
};

function ProjectView({ meta, onTrust, onClose, onOpen, mlxServers, selectedModel, modelCatalog, appearance }: ProjectViewProps) {
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
      selectedModel={selectedModel}
      modelCatalog={modelCatalog}
      appearance={appearance}
    />
  );
}

function TrustedView({
  meta,
  onClose,
  onOpen,
  mlxServers,
  selectedModel,
  modelCatalog,
  appearance,
}: {
  meta: ProjectMeta;
  onClose: () => void;
  onOpen: (path: string) => Promise<boolean>;
  mlxServers: MlxServersApi;
  selectedModel: ReturnType<typeof useSelectedModel>;
  modelCatalog: ReturnType<typeof useModelCatalog>;
  appearance: ReturnType<typeof useAppearance>;
}) {
  // The hook owns directory + selection state. Splitting it here means
  // the navigator (left zone) and the inspector (right zone) read the
  // same state without prop drilling through the workspace shell.
  const navigatorState = useFileNavigator(meta.root);
  const { selected, select, clear } = selectedModel;
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
  const [modelChooserOpen, setModelChooserOpen] = useState(false);
  const [browserOverlaySafety, setBrowserOverlaySafety] = useState<{
    browserKey: string;
    safe: boolean;
  } | null>(null);
  const [acknowledgedOverlayBrowserKey, setAcknowledgedOverlayBrowserKey] = useState<string | null>(null);
  const [browserNavigationRequest, setBrowserNavigationRequest] = useState<{
    id: number;
    identity: SessionIdentity;
    url: string;
    onResult: (outcome: 'opened' | 'needsApproval' | 'failed') => void;
  } | null>(null);
  const browserNavigationRequestIdRef = useRef(0);
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
  const openLibrary = () => {
    setActiveView('library');
    setToolDrawerOpen(false);
  };
  const openBrowser = async (url?: string): Promise<void> => {
      if (url === undefined) setBrowserNavigationRequest(null);
      // The Browser is owned by a chat, so it needs one before it can open.
      // Resolving that through the shared path matters most at launch: on
      // local scope the owner is Home, and creating a chat here instead would
      // give the Browser workspace to a conversation the user never opened.
      const navigationIdentity = await persisted.ensureOwnedSession(
        persisted.surfaceIdentity().scope,
      );
      if (navigationIdentity === null) throw new Error('Could not open this source.');
      let navigation: Promise<void> | null = null;
      if (url !== undefined) {
        browserNavigationRequestIdRef.current += 1;
        navigation = new Promise<void>((resolve, reject) => {
          setBrowserNavigationRequest({
            id: browserNavigationRequestIdRef.current,
            identity: navigationIdentity,
            url,
            onResult: (outcome) => {
              if (outcome === 'failed') reject(new Error('Could not open this source.'));
              else resolve();
            },
          });
        });
      }
      setBrowserOverlaySafety(null);
      setAcknowledgedOverlayBrowserKey(null);
      setActiveView('browser');
      setToolDrawerOpen(false);
      if (navigation !== null) await navigation;
  };
  const useContextInChat = async (source: ContextSourceRef) => {
    const owner = await ensureContextOwner('project');
    if (owner === null) return 'unavailable' as const;
    const result = persisted.chat.addContextSource(source);
    const after = persisted.surfaceIdentity();
    if (after.scope !== owner.scope || after.sessionId !== owner.sessionId) {
      return 'unavailable' as const;
    }
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
  const ensureContextOwner = (scope: 'local' | 'project'): Promise<SessionIdentity | null> =>
    persisted.ensureOwnedSession(scope);
  const libraryHandoff = createLibraryChatHandoff({
    persisted,
    projectAvailable: true,
    onAccepted: (owner, source) => {
      setContextEmphasis((previous) => ({
        key: contextSourceKey(source),
        generation: (previous?.generation ?? 0) + 1,
      }));
      setActiveView(chatViewOf(owner.scope));
      setToolDrawerOpen(false);
    },
  });
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
    setModelChooserOpen(false);
    setToolDrawerOpen(false);
  };
  const setModelWorkspaceOpen = (open: boolean) => {
    setModelChooserOpen(open);
    if (open) setOpenProjectOpen(false);
  };
  const isLocalChatSurface = persisted.activeScope === 'local';
  const activeSessionTitle =
    sessions.visibleOf(persisted.activeScope).find(({ id }) => id === persisted.activeSessionId)
      ?.title ?? null;
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
  const htmlOverlayOpen =
    toolDrawerOpen || settingsOpen || helpOpen || searchOpen || dialogs.node !== null;
  const browserSessionId = activeView === 'browser' && !openProjectOpen && !modelChooserOpen
    ? persisted.activeSessionId
    : null;
  const browserActive = browserSessionId !== null;
  const browserSessionKey = browserActive
    ? `${persisted.activeScope}:${browserSessionId}`
    : null;
  const browserOverlaySafe = browserSessionKey !== null
    && browserOverlaySafety?.browserKey === browserSessionKey
    && browserOverlaySafety.safe;
  const onBrowserOverlaySafeChange = useCallback((safe: boolean) => {
    if (browserSessionKey === null) return;
    setBrowserOverlaySafety((current) =>
      current?.browserKey === browserSessionKey && current.safe === safe
        ? current
        : { browserKey: browserSessionKey, safe },
    );
  }, [browserSessionKey]);
  useEffect(() => {
    if (!htmlOverlayOpen) {
      setAcknowledgedOverlayBrowserKey(null);
      return;
    }
    if (browserOverlaySafe) setAcknowledgedOverlayBrowserKey(browserSessionKey);
  }, [browserOverlaySafe, browserSessionKey, htmlOverlayOpen]);
  const htmlOverlayReady = !browserActive
    || browserOverlaySafe
    || acknowledgedOverlayBrowserKey === browserSessionKey;
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
        onExportSession={dialogs.exportSession}
        onSearch={() => setSearchOpen(true)}
        onLibrary={openLibrary} onSettings={openSettings}
        onHelp={openHelp}
        onOpenProject={openProjectModal}
        onCloseProject={onClose}
      />
      <div className="plume-project-main">
        <UnifiedTopBar
          subtitle={openProjectOpen
            ? 'Open project'
            : modelChooserOpen
              ? 'Models'
              : topbarSubtitle(activeView, lastSegment(meta.root), activeSessionTitle)}
          catalog={modelCatalog}
          selection={selectedModel}
          modelChooserOpen={modelChooserOpen && htmlOverlayReady}
          onModelChooserOpenChange={setModelWorkspaceOpen}
          toolsOpen={toolDrawerOpen}
          showTools
          showOpenProject
          onToggleTools={() => setToolDrawerOpen((open) => !open)}
          onOpenProject={openProjectModal}
        />
        <SessionNotices
          notice={persisted.notice}
          saveError={persisted.saveError}
          storageFull={persisted.storageFull}
          storageWarning={persisted.storageWarning}
        />
        {modelChooserOpen ? (
          <ModelChooserWorkspace
            catalog={modelCatalog}
            selection={selectedModel}
            onClose={() => setModelChooserOpen(false)}
          />
        ) : openProjectOpen ? (
          <OpenProjectView
            onOpen={onOpen}
            onClose={() => setOpenProjectOpen(false)}
          />
        ) : activeView === 'files' ? (
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
        ) : activeView === 'library' ? (
          <LibraryWorkspace
            projectIdentity={meta.root}
            disabled={persisted.chat.status === 'streaming'}
            onUseInChat={libraryHandoff.useItemInChat}
            onDropSource={libraryHandoff.useSourceInChat}
            onOpenProject={openProjectModal}
          />
        ) : browserActive ? (
          <TaskBrowserWorkspace
            key={`browser-${persisted.activeScope}-${browserSessionId}`}
            identity={{ scope: persisted.activeScope, sessionId: browserSessionId }}
            onUseInChat={useBrowserContextInChat}
            suspended={htmlOverlayOpen}
            onOverlaySafeChange={onBrowserOverlaySafeChange}
            {...(browserNavigationRequest ? { navigationRequest: browserNavigationRequest } : {})}
            onOpenResearchSource={openBrowser}
            chatProps={{
              chat: persisted.chat, selected, onClearSelection: clear,
              inspectorSelection: persisted.activeScope === 'project' ? navigatorState.selection : null,
              inspectorLineRange: persisted.activeScope === 'project' ? navigatorState.currentLineRange : null,
              projectHasInstructions: persisted.activeScope === 'project' && meta.hasAgentsMd,
              mlxServers, includeProjectContext: persisted.activeScope === 'project',
              onChooseModel: () => setModelChooserOpen(true),
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
              onChooseModel={() => setModelChooserOpen(true)}
              inspectorSelection={null}
              inspectorLineRange={null}
              projectHasInstructions={false}
              mlxServers={mlxServers}
              includeProjectContext={false}
              variant="simple"
              onOpenResearchSource={openBrowser}
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
              onChooseModel={() => setModelChooserOpen(true)}
              inspectorSelection={navigatorState.selection}
              inspectorLineRange={navigatorState.currentLineRange}
              projectHasInstructions={meta.hasAgentsMd}
              mlxServers={mlxServers}
              variant="simple"
              onOpenResearchSource={openBrowser}
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
      {htmlOverlayReady ? dialogs.node : null}
      {searchOpen && htmlOverlayReady ? (
        <SessionSearchOverlay
          projectAvailable
          notice={persisted.notice}
          onSelect={searchSelect}
          onClose={() => setSearchOpen(false)}
        />
      ) : null}
      {toolDrawerOpen && htmlOverlayReady ? (
        <ToolDrawer
          hasProject
          activeView={activeView}
          onBrowser={() => void openBrowser()}
          onFiles={openFiles}
          onBenchmarks={openBenchmarks}
          onOpenProject={openProjectModal}
          onClose={() => setToolDrawerOpen(false)}
        />
      ) : null}
      {settingsOpen && htmlOverlayReady ? (
        <ProjectSettingsModal
          inventory={inventory}
          servers={mlxServers}
          selected={selected}
          onSelect={select}
          agentMode={agentMode}
          onAgentModeChange={setAgentMode}
          inspectorSelection={navigatorState.selection}
          inspectorLineRange={navigatorState.currentLineRange}
          appearance={appearance}
          archivedContent={(
            <ArchivedSessionsSettings
              sessions={sessions}
              persisted={persisted}
              projectAvailable
            />
          )}
          onClose={() => setSettingsOpen(false)}
        />
      ) : null}
      {helpOpen && htmlOverlayReady ? <HelpPanel onClose={() => setHelpOpen(false)} /> : null}
    </section>
  );
}

function formatError(err: unknown): string {
  if (isIpcError(err)) return ipcErrorMessage(err);
  if (err instanceof Error) return err.message;
  return 'Unknown error.';
}
