// D62 unified sidebar, rewired in D63B: the Chats and Projects
// sections now render PERSISTED session summaries (D63A `sessions.*`)
// instead of the placeholder single-row local/project chats. This
// component stays presentational — list state, persistence, and the
// streaming switch guard live in `features/sessions/`.

import { useEffect, useRef, useState, type Ref } from 'react';

import type { SessionScope, SessionSummary } from '../../lib/api/sessions';
import { SessionRow } from '../sessions/SessionRow';
import { Icon, type IconName } from './Icon';

export type ProjectWorkspaceView =
  | 'local-chat'
  | 'project-chat'
  | 'files'
  | 'benchmarks'
  | 'library'
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
  onExportSession: (scope: SessionScope, session: SessionSummary) => void;
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
  onExportSession,
  onSearch,
  onLibrary,
  onSettings,
  onHelp,
  onOpenProject,
  onCloseProject,
}: UnifiedSidebarProps) {
  const [newChatChooserOpen, setNewChatChooserOpen] = useState(false);
  const newChatButtonRef = useRef<HTMLButtonElement | null>(null);
  const chatChoiceRef = useRef<HTMLButtonElement | null>(null);
  const hasProject = projectName !== null;
  const isChatView = activeView === 'local-chat' || activeView === 'project-chat';
  const rowActive = (scope: SessionScope, id: string) =>
    isChatView && activeScope === scope && activeSessionId === id;
  const navigate = (action: () => void) => {
    setNewChatChooserOpen(false);
    action();
  };

  useEffect(() => {
    if (!newChatChooserOpen) return;
    chatChoiceRef.current?.focus();
    const onKeyDown = (event: globalThis.KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      event.preventDefault();
      setNewChatChooserOpen(false);
      newChatButtonRef.current?.focus();
    };
    document.addEventListener('keydown', onKeyDown);
    return () => document.removeEventListener('keydown', onKeyDown);
  }, [newChatChooserOpen]);

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
            buttonRef={newChatButtonRef}
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
                ref={chatChoiceRef}
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
          <SidebarButton
            label="Search"
            icon="search"
            collapsed={collapsed}
            onClick={() => navigate(onSearch)}
          />
          <SidebarButton
            label="Library"
            icon="library"
            collapsed={collapsed}
            active={activeView === 'library'}
            onClick={() => navigate(onLibrary)}
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
                onSelect={() => navigate(() => onSelectSession('local', session.id))}
                onRename={() => navigate(() => onRenameSession('local', session))}
                onContinue={() => navigate(() => onContinueSession?.('local', session))}
                onRewind={() => navigate(() => onRewindSession?.('local', session))}
                onArchive={() => navigate(() => onArchiveSession('local', session))}
                onDelete={() => navigate(() => onDeleteSession('local', session))}
                onExport={() => onExportSession('local', session)}
              />
            ))
          )}
        </div>
        <div className="plume-project-sidebar-section">
          <p>Projects</p>
          {hasProject ? (
            <>
              <ProjectActionRow
                label={projectName}
                meta={trustLabel}
                onClick={() => navigate(() => onOpenProjectChat?.())}
                {...(onCloseProject === undefined ? {} : { onCloseProject })}
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
                    onSelect={() => navigate(() => onSelectSession('project', session.id))}
                    onRename={() => navigate(() => onRenameSession('project', session))}
                    onContinue={() => navigate(() => onContinueSession?.('project', session))}
                    onRewind={() => navigate(() => onRewindSession?.('project', session))}
                    onArchive={() => navigate(() => onArchiveSession('project', session))}
                    onDelete={() => navigate(() => onDeleteSession('project', session))}
                    onExport={() => onExportSession('project', session)}
                  />
                ))
              )}
            </>
          ) : (
            <SidebarButton
              label="Open project"
              icon="project"
              collapsed={collapsed}
              onClick={() => navigate(onOpenProject)}
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
          onClick={() => navigate(onSettings)}
        />
        <button
          type="button"
          className="plume-project-sidebar-help"
          aria-label="Help"
          title="Help"
          onClick={() => navigate(onHelp)}
        >
          <Icon name="help" />
        </button>
      </div>
    </aside>
  );
}

function ProjectActionRow({
  label,
  meta,
  onClick,
  onCloseProject,
}: {
  label: string;
  meta: string;
  onClick: () => void;
  onCloseProject?: () => void;
}) {
  const [menuOpen, setMenuOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const menuItemRef = useRef<HTMLButtonElement | null>(null);

  useEffect(() => {
    if (!menuOpen) return;
    menuItemRef.current?.focus();
    const close = (event: globalThis.MouseEvent | globalThis.KeyboardEvent) => {
      if (event instanceof KeyboardEvent && event.key !== 'Escape') return;
      if (event instanceof MouseEvent && event.target instanceof Node && rootRef.current?.contains(event.target)) return;
      setMenuOpen(false);
      if (event instanceof KeyboardEvent) triggerRef.current?.focus();
    };
    document.addEventListener('keydown', close);
    document.addEventListener('mousedown', close);
    return () => {
      document.removeEventListener('keydown', close);
      document.removeEventListener('mousedown', close);
    };
  }, [menuOpen]);

  return (
    <div ref={rootRef} className="plume-project-sidebar-action-row plume-project-row">
      <button type="button" className="plume-project-sidebar-action-main" onClick={onClick}>
        <Icon name="project" className="plume-project-sidebar-icon" />
        <span className="plume-project-sidebar-label">{label}</span>
        <span className="plume-project-sidebar-meta">{meta}</span>
      </button>
      {onCloseProject ? (
        <>
          <button
            ref={triggerRef}
            type="button"
            className="plume-project-sidebar-mini plume-project-sidebar-mini-menu"
            aria-label={`Project actions for ${label}`}
            aria-haspopup="menu"
            aria-expanded={menuOpen}
            onClick={() => setMenuOpen((open) => !open)}
          >
            <Icon name="more" />
          </button>
          {menuOpen ? (
            <div className="plume-project-row-menu" role="menu" aria-label={`Actions for ${label}`}>
              <button
                ref={menuItemRef}
                type="button"
                role="menuitem"
                onClick={() => {
                  setMenuOpen(false);
                  onCloseProject();
                }}
              >
                Close project
              </button>
            </div>
          ) : null}
        </>
      ) : null}
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
  buttonRef,
  onClick,
}: {
  label: string;
  icon?: IconName;
  meta?: string;
  active?: boolean;
  disabled?: boolean;
  collapsed?: boolean;
  expanded?: boolean;
  buttonRef?: Ref<HTMLButtonElement>;
  onClick: () => void;
}) {
  return (
    <button
      ref={buttonRef}
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
