import { useCallback, useEffect, useRef, useState } from 'react';

import { TaskBrowserWorkspace } from '../browser/TaskBrowserWorkspace';
import type { useAppearance } from '../appearance/useAppearance';
import { ChatPanel } from '../chat/ChatPanel';
import { HelpPanel } from '../help/HelpPanel';
import { createLibraryChatHandoff } from '../library/libraryChatHandoff';
import { LibraryWorkspace } from '../library/LibraryWorkspace';
import type { SelectedModelApi } from '../model-picker/useSelectedModel';
import type { ModelCatalogApi } from '../model-picker/useModelCatalog';
import { ModelChooserWorkspace } from '../model-picker/ModelChooser';
import type { MlxServersApi } from '../providers/useMlxServers';
import { useProviderInventory } from '../providers/useProviderInventory';
import { ArchivedSessionsSettings, useSessionDialogs } from '../sessions/SessionDialogs';
import { SessionNotices } from '../sessions/SessionNotices';
import { SessionSearchOverlay, useSearchShortcut } from '../sessions/SessionSearch';
import { usePersistedChat } from '../sessions/usePersistedChat';
import { useSessions } from '../sessions/useSessions';
import type { ContextSourceRef } from '../../lib/api/chat';
import type { SessionIdentity } from '../../lib/api/sessions';
import { ToolDrawer } from './ToolDrawer';
import {
  NoProjectSettingsModal,
  OpenProjectView,
  UnifiedTopBar,
  topbarSubtitle,
  useSidebarPreference,
} from './UnifiedChrome';
import { UnifiedSidebar, type ProjectWorkspaceView } from './UnifiedSidebar';

export function NoProjectChatView({
  onOpen,
  openingPath,
  mlxServers,
  selectedModel,
  modelCatalog,
  appearance,
}: {
  onOpen: (path: string) => Promise<boolean>;
  openingPath: string | null;
  mlxServers: MlxServersApi;
  selectedModel: SelectedModelApi;
  modelCatalog: ModelCatalogApi;
  appearance: ReturnType<typeof useAppearance>;
}) {
  const { selected, select, clear } = selectedModel;
  const inventory = useProviderInventory();
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [helpOpen, setHelpOpen] = useState(false);
  const [openProjectOpen, setOpenProjectOpen] = useState(false);
  const [activeView, setActiveView] = useState<ProjectWorkspaceView>('local-chat');
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
    setModelChooserOpen(false);
    setToolDrawerOpen(false);
  };
  const setModelWorkspaceOpen = (open: boolean) => {
    setModelChooserOpen(open);
    if (open) setOpenProjectOpen(false);
  };
  const openLocalChat = () => {
    setActiveView('local-chat');
    setToolDrawerOpen(false);
  };
  const openLibrary = () => {
    setActiveView('library');
    setToolDrawerOpen(false);
  };
  const openBrowser = async (url?: string): Promise<void> => {
      if (url === undefined) setBrowserNavigationRequest(null);
      // Home owns the Browser here. Creating a chat instead — which is what
      // happened whenever the Browser asked before the Home lookup returned —
      // handed the workspace to a conversation the user never opened.
      const navigationIdentity = await persisted.ensureOwnedSession('local');
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
  const useBrowserContextInChat = async (
    owner: SessionIdentity,
    source: ContextSourceRef,
  ) => {
    const before = persisted.surfaceIdentity();
    if (
      owner.scope !== 'local' ||
      before.scope !== owner.scope ||
      before.sessionId !== owner.sessionId
    ) {
      return 'unavailable' as const;
    }
    const result = persisted.chat.addContextSource(source);
    const after = persisted.surfaceIdentity();
    if (after.scope !== owner.scope || after.sessionId !== owner.sessionId) {
      return 'unavailable' as const;
    }
    return result;
  };
  const libraryHandoff = createLibraryChatHandoff({
    persisted,
    projectAvailable: false,
    onAccepted: openLocalChat,
  });
  const activeSessionTitle =
    sessions.visibleOf('local').find(({ id }) => id === persisted.activeSessionId)?.title ??
    null;
  const htmlOverlayOpen =
    toolDrawerOpen || settingsOpen || helpOpen || searchOpen || dialogs.node !== null;
  const browserSessionId = activeView === 'browser' && !openProjectOpen && !modelChooserOpen
    ? persisted.activeSessionId
    : null;
  const browserActive = browserSessionId !== null;
  const browserSessionKey = browserActive ? `local:${browserSessionId}` : null;
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
        projectName={null}
        trustLabel="local chat"
        activeView={activeView}
        settingsOpen={settingsOpen}
        localSessions={sessions.visibleOf('local')}
        projectSessions={[]}
        activeSessionId={persisted.activeSessionId}
        activeScope="local"
        collapsed={sidebarCollapsed}
        onCollapsedChange={setSidebarCollapsed}
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
        onExportSession={dialogs.exportSession}
        onSearch={() => setSearchOpen(true)}
        onLibrary={openLibrary}
        onSettings={openSettings}
        onHelp={openHelp}
        onOpenProject={openProjectModal}
      />
      <div className="plume-project-main">
        <UnifiedTopBar
          subtitle={openProjectOpen
            ? 'Open project'
            : modelChooserOpen
              ? 'Models'
              : topbarSubtitle(activeView, null, activeSessionTitle)}
          catalog={modelCatalog}
          selection={selectedModel}
          modelChooserOpen={modelChooserOpen && htmlOverlayReady}
          onModelChooserOpenChange={setModelWorkspaceOpen}
          toolsOpen={toolDrawerOpen}
          showTools
          showOpenProject={false}
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
            busy={openingPath !== null}
            onClose={() => setOpenProjectOpen(false)}
          />
        ) : activeView === 'library' ? (
          <LibraryWorkspace
            projectIdentity={null}
            disabled={persisted.chat.status === 'streaming'}
            onUseInChat={libraryHandoff.useItemInChat}
            onDropSource={libraryHandoff.useSourceInChat}
            onOpenProject={openProjectModal}
          />
        ) : browserActive ? (
          <TaskBrowserWorkspace
            key={`browser-local-${browserSessionId}`}
            identity={{ scope: 'local', sessionId: browserSessionId }}
            onUseInChat={useBrowserContextInChat}
            suspended={htmlOverlayOpen}
            onOverlaySafeChange={onBrowserOverlaySafeChange}
            {...(browserNavigationRequest ? { navigationRequest: browserNavigationRequest } : {})}
            onOpenResearchSource={openBrowser}
            chatProps={{
              chat: persisted.chat,
              selected,
              onClearSelection: clear,
              inspectorSelection: null,
              inspectorLineRange: null,
              projectHasInstructions: false,
              mlxServers,
              includeProjectContext: false,
              variant: 'simple',
              onChooseModel: () => setModelChooserOpen(true),
              prepareSend: persisted.prepareSend,
            }}
          />
        ) : (
          <section className="plume-no-project-chat" aria-label="Chat">
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
              prepareSend={persisted.prepareSend}
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
      {htmlOverlayReady ? dialogs.node : null}
      {searchOpen && htmlOverlayReady ? (
        <SessionSearchOverlay
          projectAvailable={false}
          notice={persisted.notice}
          onSelect={(scope, sessionId) => persisted.selectSession(scope, sessionId)}
          onClose={() => setSearchOpen(false)}
        />
      ) : null}
      {toolDrawerOpen && htmlOverlayReady ? (
        <ToolDrawer
          hasProject={false}
          activeView={activeView}
          onBrowser={() => void openBrowser()}
          onFiles={openProjectModal}
          onBenchmarks={openProjectModal}
          onOpenProject={openProjectModal}
          onClose={() => setToolDrawerOpen(false)}
        />
      ) : null}
      {settingsOpen && htmlOverlayReady ? (
        <NoProjectSettingsModal
          inventory={inventory}
          servers={mlxServers}
          selected={selected}
          onSelect={select}
          appearance={appearance}
          archivedContent={(
            <ArchivedSessionsSettings
              sessions={sessions}
              persisted={persisted}
              projectAvailable={false}
            />
          )}
          onClose={() => setSettingsOpen(false)}
        />
      ) : null}
      {helpOpen && htmlOverlayReady ? <HelpPanel onClose={() => setHelpOpen(false)} /> : null}
      {openingPath ? (
        <div className="plume-unified-opening" role="status">
          Opening {openingPath}
        </div>
      ) : null}
    </section>
  );
}
