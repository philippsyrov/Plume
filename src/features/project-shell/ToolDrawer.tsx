import type { ProjectWorkspaceView } from './UnifiedSidebar';

type ToolDrawerProps = {
  hasProject: boolean;
  activeView: ProjectWorkspaceView;
  onChat: () => void;
  onBrowser: () => void;
  onFiles: () => void;
  onKnowledge: () => void;
  onBenchmarks: () => void;
  onOpenProject: () => void;
  onClose: () => void;
};

export function ToolDrawer({
  hasProject,
  activeView,
  onChat,
  onBrowser,
  onFiles,
  onKnowledge,
  onBenchmarks,
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
      <aside className="plume-tool-drawer" aria-label="Workspace views">
        <header className="plume-tool-drawer-header">
          <div>
            <h3>Workspace views</h3>
            <p>Choose where to work</p>
          </div>
          <button
            type="button"
            className="ink-button plume-tool-drawer-close"
            onClick={onClose}
            aria-label="Close workspace views"
          >
            Close
          </button>
        </header>
        <nav className="plume-tool-drawer-list" aria-label="Workspace view picker">
          <ToolDrawerItem
            label="Files"
            icon="files"
            meta={hasProject ? (activeView === 'files' ? 'open' : undefined) : 'open project'}
            active={activeView === 'files'}
            onClick={hasProject ? onFiles : onOpenProject}
          />
          {hasProject ? (
            <ToolDrawerItem
              label="Knowledge"
              icon="knowledge"
              meta={activeView === 'knowledge' ? 'open' : undefined}
              active={activeView === 'knowledge'}
              onClick={onKnowledge}
            />
          ) : null}
          <ToolDrawerItem
            label="Benchmarks"
            icon="benchmarks"
            meta={hasProject ? (activeView === 'benchmarks' ? 'open' : undefined) : 'open project'}
            active={activeView === 'benchmarks'}
            onClick={hasProject ? onBenchmarks : onOpenProject}
          />
          <ToolDrawerItem label="Terminal" icon="terminal" meta="soon" disabled />
          <ToolDrawerItem
            label="Browser"
            icon="browser"
            meta={activeView === 'browser' ? 'open' : undefined}
            active={activeView === 'browser'}
            onClick={onBrowser}
          />
          <ToolDrawerItem
            label={hasProject ? 'Project chat' : 'Chat'}
            icon="chat"
            meta={
              activeView === (hasProject ? 'project-chat' : 'local-chat') ? 'open' : undefined
            }
            active={activeView === (hasProject ? 'project-chat' : 'local-chat')}
            onClick={onChat}
          />
        </nav>
      </aside>
    </div>
  );
}

type ToolDrawerItemProps = {
  label: string;
  icon: 'files' | 'knowledge' | 'terminal' | 'browser' | 'chat' | 'benchmarks';
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
