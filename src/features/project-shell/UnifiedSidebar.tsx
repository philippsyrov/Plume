// D62 unified sidebar, rewired in D63B: the Chats and Projects
// sections now render PERSISTED session summaries (D63A `sessions.*`)
// instead of the placeholder single-row local/project chats. This
// component stays presentational — list state, persistence, and the
// streaming switch guard live in `features/sessions/`.

import { useState } from 'react';

import type { SessionScope, SessionSummary } from '../../lib/api/sessions';
import { SessionRow } from '../sessions/SessionRow';
import { Icon, type IconName } from './Icon';

export type ProjectWorkspaceView =
  | 'local-chat'
  | 'project-chat'
  | 'files'
  | 'benchmarks'
  | 'knowledge'
  | 'browser';

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
  collapsed: boolean;
  onCollapsedChange: (collapsed: boolean) => void;
  onSelectSession: (scope: SessionScope, sessionId: string) => void;
  onNewLocalChat: () => void;
  onNewProjectChat?: () => void;
  onOpenProjectChat?: () => void;
  onRenameSession: (scope: SessionScope, session: SessionSummary) => void;
  onContinueSession?: (scope: SessionScope, session: SessionSummary) => void;
  onRewindSession?: (scope: SessionScope, session: SessionSummary) => void;
  onArchiveSession: (scope: SessionScope, session: SessionSummary) => void;
  onDeleteSession: (scope: SessionScope, session: SessionSummary) => void;
  onShowArchived: (scope: SessionScope) => void;
  /** D66: open the chat-search overlay (also bound to Cmd+K). */
  onSearch: () => void;
  onLibrary: () => void;
  onSettings: () => void;
  onHelp: () => void;
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
  collapsed,
  onCollapsedChange,
  onSelectSession,
  onNewLocalChat,
  onNewProjectChat,
  onOpenProjectChat,
  onRenameSession,
  onContinueSession,
  onRewindSession,
  onArchiveSession,
  onDeleteSession,
  onShowArchived,
  onSearch,
  onLibrary,
  onSettings,
  onHelp,
  onOpenProject,
  onCloseProject,
}: UnifiedSidebarProps) {
  const [newChatChooserOpen, setNewChatChooserOpen] = useState(false);
  const hasProject = projectName !== null;
  const isChatView = activeView === 'local-chat' || activeView === 'project-chat';
  const rowActive = (scope: SessionScope, id: string) =>
    isChatView && activeScope === scope && activeSessionId === id;

  return (
    <aside
      className={`plume-project-sidebar${
        collapsed ? ' plume-project-sidebar-collapsed' : ''
      }`}
      aria-label="Project navigation"
    >
      <button
        type="button"
        className="plume-project-sidebar-collapse"
        onClick={() => {
          if (!collapsed) setNewChatChooserOpen(false);
          onCollapsedChange(!collapsed);
        }}
        aria-label={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
        title={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
      >
        <Icon name={collapsed ? 'sidebar-expand' : 'sidebar-collapse'} />
      </button>
      <div className="plume-project-sidebar-content">
        <nav className="plume-project-sidebar-nav" aria-label="Workspace">
          <SidebarButton
            label="New chat"
            icon="chat"
            collapsed={collapsed}
            expanded={newChatChooserOpen}
            onClick={() => {
              if (!hasProject) {
                onNewLocalChat();
                return;
              }
              setNewChatChooserOpen((open) => !open);
            }}
          />
          {hasProject && newChatChooserOpen ? (
            <div className="plume-new-chat-chooser" role="group" aria-label="New chat scope">
              <p>Start a Chat or work inside this Project.</p>
              <button
                type="button"
                aria-label="Chat"
                onClick={() => {
                  setNewChatChooserOpen(false);
                  onNewLocalChat();
                }}
              >
                <strong>Chat</strong>
                <span>Starts without project context.</span>
              </button>
              <button
                type="button"
                aria-label="Project"
                disabled={!onNewProjectChat}
                onClick={() => {
                  setNewChatChooserOpen(false);
                  onNewProjectChat?.();
                }}
              >
                <strong>Project</strong>
                <span>Uses {projectName} context and tools.</span>
              </button>
            </div>
          ) : null}
          <SidebarButton label="Search" icon="search" collapsed={collapsed} onClick={onSearch} />
          <SidebarButton
            label="Library"
            icon="library"
            collapsed={collapsed}
            active={activeView === 'knowledge'}
            disabled={!hasProject}
            onClick={onLibrary}
          />
        </nav>
        <div className="plume-project-sidebar-section">
          <p>Tasks</p>
          {localSessions.length === 0 ? (
            <p className="plume-project-sidebar-empty" role="status">
              No tasks yet — use New chat above.
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
                onRewind={() => onRewindSession?.('local', session)}
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
                meta={trustLabel}
                onClick={() => onOpenProjectChat?.()}
              />
              {projectSessions.length === 0 ? (
                <p className="plume-project-sidebar-empty" role="status">
                  No project tasks yet — use New chat above.
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
                    onRewind={() => onRewindSession?.('project', session)}
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
            <SidebarButton
              label="Open project"
              icon="project"
              collapsed={collapsed}
              onClick={onOpenProject}
            />
          )}
        </div>
      </div>
      <div className="plume-project-sidebar-footer">
        <SidebarButton
          label="Settings"
          icon="settings"
          collapsed={collapsed}
          active={settingsOpen}
          onClick={onSettings}
        />
        <SidebarButton label="Help" icon="help" collapsed={collapsed} onClick={onHelp} />
        {hasProject && onCloseProject ? (
          <SidebarButton
            label="Close project"
            icon="close"
            collapsed={collapsed}
            onClick={onCloseProject}
          />
        ) : null}
      </div>
    </aside>
  );
}

function SidebarActionRow({
  label,
  icon,
  meta,
  active,
  onClick,
}: {
  label: string;
  icon?: IconName;
  meta?: string;
  active?: boolean;
  onClick: () => void;
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
        aria-label={label}
      >
        {icon ? <Icon name={icon} className="plume-project-sidebar-icon" /> : null}
        <span className="plume-project-sidebar-label">{label}</span>
        {meta ? <span className="plume-project-sidebar-meta">{meta}</span> : null}
      </button>
    </div>
  );
}

function SidebarButton({
  label,
  icon,
  meta,
  active,
  disabled,
  collapsed,
  expanded,
  onClick,
}: {
  label: string;
  icon?: IconName;
  meta?: string;
  active?: boolean;
  disabled?: boolean;
  collapsed?: boolean;
  expanded?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      className={`plume-project-sidebar-item${active ? ' plume-project-sidebar-item-active' : ''}`}
      onClick={onClick}
      disabled={disabled}
      aria-current={active ? 'page' : undefined}
      aria-expanded={expanded}
      aria-label={label}
      title={collapsed ? label : undefined}
    >
      {icon ? <Icon name={icon} className="plume-project-sidebar-icon" /> : null}
      <span className="plume-project-sidebar-label">{label}</span>
      {meta ? <span className="plume-project-sidebar-meta">{meta}</span> : null}
    </button>
  );
}
