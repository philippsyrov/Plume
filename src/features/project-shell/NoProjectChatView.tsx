import { useCallback, useEffect, useState } from 'react';

import { TaskBrowserWorkspace } from '../browser/TaskBrowserWorkspace';
import type { useAppearance } from '../appearance/useAppearance';
import { ChatPanel } from '../chat/ChatPanel';
import { HelpPanel } from '../help/HelpPanel';
import { createLibraryChatHandoff } from '../library/libraryChatHandoff';
import { LibraryWorkspace } from '../library/LibraryWorkspace';
import { useSelectedModel } from '../model-picker/useSelectedModel';
import type { MlxServersApi } from '../providers/useMlxServers';
import { useProviderInventory } from '../providers/useProviderInventory';
import { useSessionDialogs } from '../sessions/SessionDialogs';
import { SessionNotices } from '../sessions/SessionNotices';
import { SessionSearchOverlay, useSearchShortcut } from '../sessions/SessionSearch';
import { usePersistedChat } from '../sessions/usePersistedChat';
import { useSessions } from '../sessions/useSessions';
import type { ContextSourceRef } from '../../lib/api/chat';
import type { SessionIdentity } from '../../lib/api/sessions';
import { ToolDrawer } from './ToolDrawer';
import {
  NoProjectSettingsModal,
  OpenProjectModal,
  UnifiedTopBar,
  topbarSubtitle,
  useSidebarPreference,
} from './UnifiedChrome';
import { UnifiedSidebar, type ProjectWorkspaceView } from './UnifiedSidebar';

export function NoProjectChatView({
  onOpen,
  openingPath,
  mlxServers,
  appearance,
}: {
  onOpen: (path: string) => void;
  openingPath: string | null;
  mlxServers: MlxServersApi;
  appearance: ReturnType<typeof useAppearance>;
}) {
  const { selected, select, clear } = useSelectedModel();
  const inventory = useProviderInventory();
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [helpOpen, setHelpOpen] = useState(false);
  const [openProjectOpen, setOpenProjectOpen] = useState(false);
  const [activeView, setActiveView] = useState<ProjectWorkspaceView>('local-chat');
  const [toolDrawerOpen, setToolDrawerOpen] = useState(false);
  const [browserOverlaySafety, setBrowserOverlaySafety] = useState<{
    browserKey: string;
    safe: boolean;
  } | null>(null);
  const [acknowledgedOverlayBrowserKey, setAcknowledgedOverlayBrowserKey] = useState<string | null>(null);
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
    setToolDrawerOpen(false);
  };
  const openLocalChat = () => {
    setActiveView('local-chat');
    setToolDrawerOpen(false);
  };
  const openLibrary = () => {
    setActiveView('library');
    setToolDrawerOpen(false);
  };
  const openBrowser = () => {
    void (async () => {
      if (persisted.surfaceIdentity().sessionId === null) {
        const created = await persisted.startNewSession('local');
        if (!created) return;
      }
      if (persisted.surfaceIdentity().sessionId === null) return;
      setBrowserOverlaySafety(null);
      setAcknowledgedOverlayBrowserKey(null);
      setActiveView('browser');
      setToolDrawerOpen(false);
    })();
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
    toolDrawerOpen || settingsOpen || helpOpen || openProjectOpen || searchOpen || dialogs.node !== null;
  const browserSessionId = activeView === 'browser' ? persisted.activeSessionId : null;
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
        hasArchivedLocal={sessions.archivedOf('local').length > 0}
        hasArchivedProject={false}
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
        onShowArchived={dialogs.openArchived}
        onSearch={() => setSearchOpen(true)}
        onLibrary={openLibrary}
        onSettings={openSettings}
        onHelp={openHelp}
        onOpenProject={openProjectModal}
      />
      <div className="plume-project-main">
        <UnifiedTopBar
          subtitle={topbarSubtitle(activeView, null, activeSessionTitle)}
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
        {activeView === 'library' ? (
          <LibraryWorkspace
            projectIdentity={null}
            disabled={persisted.chat.status === 'streaming'}
            onUseInChat={libraryHandoff.useItemInChat}
            onDropSource={libraryHandoff.useSourceInChat}
          />
        ) : browserActive ? (
          <TaskBrowserWorkspace
            key={`browser-local-${browserSessionId}`}
            identity={{ scope: 'local', sessionId: browserSessionId }}
            onUseInChat={useBrowserContextInChat}
            suspended={htmlOverlayOpen}
            onOverlaySafeChange={onBrowserOverlaySafeChange}
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
          onChat={openLocalChat}
          onBrowser={openBrowser}
          onFiles={openProjectModal}
          onLibrary={openLibrary}
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
          onClose={() => setSettingsOpen(false)}
        />
      ) : null}
      {helpOpen && htmlOverlayReady ? <HelpPanel onClose={() => setHelpOpen(false)} /> : null}
      {openProjectOpen && htmlOverlayReady ? (
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
