// D62 unified sidebar, rewired in D63B: the Chats and Projects
// sections now render PERSISTED session summaries (D63A `sessions.*`)
// instead of the placeholder single-row local/project chats. This
// component stays presentational — list state, persistence, and the
// streaming switch guard live in `features/sessions/`.

import type { SessionScope, SessionSummary } from '../../lib/api/sessions';
import { SessionRow } from '../sessions/SessionRow';

export type ProjectWorkspaceView = 'local-chat' | 'project-chat' | 'files' | 'benchmarks';

type UnifiedSidebarProps = {
  projectName: string | null;
  trustLabel: string;
  activeView: ProjectWorkspaceView;
  settingsOpen: boolean;
  /** Non-archived local sessions, newest update first. */
  localSessions: SessionSummary[];
  /** Non-archived sessions of the open project. Ignored without a
   * project. */
  projectSessions: SessionSummary[];
  /** Persisted session currently shown in the central surface. */
  activeSessionId: string | null;
  /** Scope of the active chat surface, for row highlighting. */
  activeScope: SessionScope;
  /** True when the scope has archived sessions worth a modal entry. */
  hasArchivedLocal: boolean;
  hasArchivedProject: boolean;
  onSelectSession: (scope: SessionScope, sessionId: string) => void;
  onNewLocalChat: () => void;
  onNewProjectChat?: () => void;
  onOpenProjectChat?: () => void;
  onRenameSession: (scope: SessionScope, session: SessionSummary) => void;
  onContinueSession?: (scope: SessionScope, session: SessionSummary) => void;
  onArchiveSession: (scope: SessionScope, session: SessionSummary) => void;
  onDeleteSession: (scope: SessionScope, session: SessionSummary) => void;
  onShowArchived: (scope: SessionScope) => void;
  /** D66: open the chat-search overlay (also bound to Cmd+K). */
  onSearch: () => void;
  onSettings: () => void;
  onOpenProject: () => void;
  onCloseProject?: () => void;
};

export function UnifiedSidebar({
  projectName,
  trustLabel,
  activeView,
  settingsOpen,
  localSessions,
  projectSessions,
  activeSessionId,
  activeScope,
  hasArchivedLocal,
  hasArchivedProject,
  onSelectSession,
  onNewLocalChat,
  onNewProjectChat,
  onOpenProjectChat,
  onRenameSession,
  onContinueSession,
  onArchiveSession,
  onDeleteSession,
  onShowArchived,
  onSearch,
  onSettings,
  onOpenProject,
  onCloseProject,
}: UnifiedSidebarProps) {
  const hasProject = projectName !== null;
  const isChatView = activeView !== 'files' && activeView !== 'benchmarks';
  const rowActive = (scope: SessionScope, id: string) =>
    isChatView && activeScope === scope && activeSessionId === id;

  return (
    <aside className="plume-project-sidebar" aria-label="Project navigation">
      <div className="plume-project-sidebar-content">
        <nav className="plume-project-sidebar-nav" aria-label="Workspace">
          <SidebarButton
            label={hasProject ? 'New local chat' : 'New chat'}
            icon="chat"
            onClick={onNewLocalChat}
          />
          <SidebarButton label="Search chats" icon="search" onClick={onSearch} />
          <SidebarButton
            label="Settings"
            icon="settings"
            active={settingsOpen}
            onClick={onSettings}
          />
        </nav>
        <div className="plume-project-sidebar-section">
          <p>{hasProject ? 'Local chats' : 'Chats'}</p>
          {localSessions.length === 0 ? (
            <p className="plume-project-sidebar-empty" role="status">
              No chats yet — use {hasProject ? 'New local chat' : 'New chat'} above.
            </p>
          ) : (
            localSessions.map((session) => (
              <SessionRow
                key={session.id}
                session={session}
                active={rowActive('local', session.id)}
                onSelect={() => onSelectSession('local', session.id)}
                onRename={() => onRenameSession('local', session)}
                onContinue={() => onContinueSession?.('local', session)}
                onArchive={() => onArchiveSession('local', session)}
                onDelete={() => onDeleteSession('local', session)}
              />
            ))
          )}
          {hasArchivedLocal ? (
            <button
              type="button"
              className="plume-project-sidebar-archived"
              onClick={() => onShowArchived('local')}
            >
              Archived chats
            </button>
          ) : null}
        </div>
        <div className="plume-project-sidebar-section">
          <p>Projects</p>
          {hasProject ? (
            <>
              <SidebarActionRow
                label={projectName}
                icon="project"
                onClick={() => onOpenProjectChat?.()}
                actions={
                  onNewProjectChat
                    ? [
                        {
                          label: 'New project chat',
                          kind: 'new' as const,
                          onClick: onNewProjectChat,
                        },
                      ]
                    : []
                }
              />
              {projectSessions.length === 0 ? (
                <p className="plume-project-sidebar-empty" role="status">
                  No project chats yet — use New project chat above.
                </p>
              ) : (
                projectSessions.map((session) => (
                  <SessionRow
                    key={session.id}
                    session={session}
                    active={rowActive('project', session.id)}
                    onSelect={() => onSelectSession('project', session.id)}
                    onRename={() => onRenameSession('project', session)}
                    onContinue={() => onContinueSession?.('project', session)}
                    onArchive={() => onArchiveSession('project', session)}
                    onDelete={() => onDeleteSession('project', session)}
                  />
                ))
              )}
              {hasArchivedProject ? (
                <button
                  type="button"
                  className="plume-project-sidebar-archived"
                  onClick={() => onShowArchived('project')}
                >
                  Archived project chats
                </button>
              ) : null}
            </>
          ) : (
            <SidebarButton label="Open project" icon="project" onClick={onOpenProject} />
          )}
        </div>
      </div>
      <div className="plume-project-sidebar-footer">
        <div>
          <strong>Plume</strong>
          <span>{trustLabel}</span>
        </div>
        {hasProject && onCloseProject ? (
          <button type="button" className="ink-button" onClick={onCloseProject}>
            Close
          </button>
        ) : null}
      </div>
    </aside>
  );
}

type SidebarAction = {
  label: string;
  kind: 'new' | 'menu';
  onClick: () => void;
};

type SidebarIcon = 'chat' | 'files' | 'settings' | 'project' | 'search';

function SidebarActionRow({
  label,
  icon,
  meta,
  active,
  onClick,
  actions,
}: {
  label: string;
  icon?: SidebarIcon;
  meta?: string;
  active?: boolean;
  onClick: () => void;
  actions: SidebarAction[];
}) {
  return (
    <div
      className={`plume-project-sidebar-action-row${
        active ? ' plume-project-sidebar-action-row-active' : ''
      }`}
    >
      <button
        type="button"
        className="plume-project-sidebar-action-main"
        onClick={onClick}
        aria-current={active ? 'page' : undefined}
      >
        {icon ? (
          <span
            className={`plume-project-sidebar-icon plume-project-sidebar-icon-${icon}`}
            aria-hidden="true"
          />
        ) : null}
        <span className="plume-project-sidebar-label">{label}</span>
        {meta ? <span className="plume-project-sidebar-meta">{meta}</span> : null}
      </button>
      {actions.map((action) => (
        <button
          key={action.label}
          type="button"
          className={`plume-project-sidebar-mini plume-project-sidebar-mini-${action.kind}`}
          onClick={action.onClick}
          aria-label={action.label}
          title={action.label}
        >
          <span>{action.kind === 'new' ? action.label : '...'}</span>
        </button>
      ))}
    </div>
  );
}

function SidebarButton({
  label,
  icon,
  meta,
  active,
  disabled,
  onClick,
}: {
  label: string;
  icon?: SidebarIcon;
  meta?: string;
  active?: boolean;
  disabled?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      className={`plume-project-sidebar-item${active ? ' plume-project-sidebar-item-active' : ''}`}
      onClick={onClick}
      disabled={disabled}
      aria-current={active ? 'page' : undefined}
    >
      {icon ? (
        <span
          className={`plume-project-sidebar-icon plume-project-sidebar-icon-${icon}`}
          aria-hidden="true"
        />
      ) : null}
      <span className="plume-project-sidebar-label">{label}</span>
      {meta ? <span className="plume-project-sidebar-meta">{meta}</span> : null}
    </button>
  );
}
