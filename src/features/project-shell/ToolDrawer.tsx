type ProjectToolView = 'local-chat' | 'project-chat' | 'files';

type ToolDrawerProps = {
  hasProject: boolean;
  activeView: ProjectToolView;
  onChat: () => void;
  onFiles: () => void;
  onOpenProject: () => void;
  onClose: () => void;
};

export function ToolDrawer({
  hasProject,
  activeView,
  onChat,
  onFiles,
  onOpenProject,
  onClose,
}: ToolDrawerProps) {
  return (
    <div
      className="plume-tool-drawer-backdrop"
      role="presentation"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <aside className="plume-tool-drawer" aria-label="Project tools">
        <header className="plume-tool-drawer-header">
          <div>
            <h3>Tools</h3>
            <p>{hasProject ? 'Project context' : 'Local chat'}</p>
          </div>
          <button
            type="button"
            className="ink-button plume-tool-drawer-close"
            onClick={onClose}
            aria-label="Close project tools"
          >
            Close
          </button>
        </header>
        <nav className="plume-tool-drawer-list" aria-label="Project tool picker">
          <ToolDrawerItem
            label="Files"
            icon="files"
            meta={hasProject ? (activeView === 'files' ? 'open' : undefined) : 'open project'}
            active={activeView === 'files'}
            onClick={hasProject ? onFiles : onOpenProject}
          />
          <ToolDrawerItem label="Terminal" icon="terminal" meta="soon" disabled />
          <ToolDrawerItem label="Browser" icon="browser" meta="soon" disabled />
          <ToolDrawerItem
            label="Project chat"
            icon="chat"
            meta={activeView === 'project-chat' ? 'open' : undefined}
            active={activeView === 'project-chat'}
            onClick={onChat}
          />
        </nav>
      </aside>
    </div>
  );
}

type ToolDrawerItemProps = {
  label: string;
  icon: 'files' | 'terminal' | 'browser' | 'chat';
  meta?: string | undefined;
  active?: boolean;
  disabled?: boolean;
  onClick?: () => void;
};

function ToolDrawerItem({
  label,
  icon,
  meta,
  active,
  disabled,
  onClick,
}: ToolDrawerItemProps) {
  return (
    <button
      type="button"
      className={`plume-tool-drawer-item${active ? ' plume-tool-drawer-item-active' : ''}`}
      onClick={onClick}
      disabled={disabled}
      aria-current={active ? 'page' : undefined}
    >
      <span
        className={`plume-tool-drawer-icon plume-tool-drawer-icon-${icon}`}
        aria-hidden="true"
      />
      <span className="plume-tool-drawer-label">{label}</span>
      {meta ? <span className="plume-tool-drawer-meta">{meta}</span> : null}
    </button>
  );
}
