export type ProjectWorkspaceView = 'local-chat' | 'project-chat' | 'files';

type UnifiedSidebarProps = {
  projectName: string | null;
  trustLabel: string;
  activeView: ProjectWorkspaceView;
  settingsOpen: boolean;
  localChatTitle: string;
  projectChatTitle: string;
  onLocalChat: () => void;
  onNewLocalChat: () => void;
  onRenameLocalChat: () => void;
  onProjectChat?: () => void;
  onNewProjectChat?: () => void;
  onRenameProjectChat?: () => void;
  onSettings: () => void;
  onOpenProject: () => void;
  onCloseProject?: () => void;
};

export function UnifiedSidebar({
  projectName,
  trustLabel,
  activeView,
  settingsOpen,
  localChatTitle,
  projectChatTitle,
  onLocalChat,
  onNewLocalChat,
  onRenameLocalChat,
  onProjectChat,
  onNewProjectChat,
  onRenameProjectChat,
  onSettings,
  onOpenProject,
  onCloseProject,
}: UnifiedSidebarProps) {
  const hasProject = projectName !== null;
  return (
    <aside className="plume-project-sidebar" aria-label="Project navigation">
      <nav className="plume-project-sidebar-nav" aria-label="Workspace">
        <SidebarButton label="New chat" icon="chat" onClick={onNewLocalChat} />
        <SidebarButton
          label="Settings"
          icon="settings"
          active={settingsOpen}
          onClick={onSettings}
        />
      </nav>
      <div className="plume-project-sidebar-section">
        <p>Chats</p>
        <SidebarActionRow
          label={localChatTitle}
          meta="now"
          active={activeView === 'local-chat'}
          onClick={onLocalChat}
          actions={[
            { label: 'Rename chat', kind: 'menu', onClick: onRenameLocalChat },
          ]}
        />
      </div>
      <div className="plume-project-sidebar-section">
        <p>Projects</p>
        {hasProject ? (
          <>
            <SidebarActionRow
              label={projectName}
              icon="project"
              onClick={onProjectChat ?? onOpenProject}
              actions={[
                ...(onNewProjectChat
                  ? [{ label: 'New project chat', kind: 'new' as const, onClick: onNewProjectChat }]
                  : []),
                ...(onRenameProjectChat
                  ? [
                      {
                        label: 'Rename project chat',
                        kind: 'menu' as const,
                        onClick: onRenameProjectChat,
                      },
                    ]
                  : []),
              ]}
            />
            <SidebarButton
              label={projectChatTitle}
              active={activeView === 'project-chat'}
              onClick={onProjectChat ?? onOpenProject}
            />
          </>
        ) : (
          <SidebarButton label="Open project" icon="project" onClick={onOpenProject} />
        )}
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

type SidebarIcon = 'chat' | 'files' | 'settings' | 'project';

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
          <span aria-hidden="true">{action.kind === 'new' ? '+' : '...'}</span>
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
