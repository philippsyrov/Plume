import { useEffect, useRef, type KeyboardEvent as ReactKeyboardEvent } from 'react';

import { Icon, type IconName } from './Icon';
import type { ProjectWorkspaceView } from './UnifiedSidebar';

type ToolDrawerProps = {
  hasProject: boolean;
  activeView: ProjectWorkspaceView;
  onChat: () => void;
  onBrowser: () => void;
  onFiles: () => void;
  onLibrary: () => void;
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
  onLibrary,
  onBenchmarks,
  onOpenProject,
  onClose,
}: ToolDrawerProps) {
  const previousFocusRef = useRef(
    document.activeElement instanceof HTMLElement ? document.activeElement : null,
  );
  const drawerRef = useRef<HTMLElement | null>(null);
  const closeRef = useRef<HTMLButtonElement | null>(null);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;

  useEffect(() => {
    closeRef.current?.focus();
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      event.preventDefault();
      onCloseRef.current();
    };
    document.addEventListener('keydown', closeOnEscape);
    return () => {
      document.removeEventListener('keydown', closeOnEscape);
      previousFocusRef.current?.focus();
    };
  }, []);

  const containTab = (event: ReactKeyboardEvent<HTMLElement>) => {
    if (event.key !== 'Tab') return;
    const controls = drawerRef.current === null ? [] : focusableControls(drawerRef.current);
    const first = controls[0];
    const last = controls.at(-1);
    if (first === undefined || last === undefined) return;
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  };

  return (
    <div
      className="plume-tool-drawer-backdrop"
      role="presentation"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <aside
        ref={drawerRef}
        className="plume-tool-drawer"
        aria-label="Workspace views"
        onKeyDown={containTab}
      >
        <header className="plume-tool-drawer-header">
          <h3>Workspace views</h3>
          <button
            ref={closeRef}
            type="button"
            className="ink-button plume-tool-drawer-close"
            onClick={onClose}
            aria-label="Close workspace views"
          >
            <Icon name="close" />
          </button>
        </header>
        <nav className="plume-tool-drawer-list" aria-label="Workspace view picker">
          <ToolDrawerItem
            label="Files"
            icon="files"
            meta={hasProject ? undefined : 'Open project'}
            active={activeView === 'files'}
            onClick={hasProject ? onFiles : onOpenProject}
          />
          <ToolDrawerItem
            label="Library"
            icon="library"
            active={activeView === 'library'}
            onClick={onLibrary}
          />
          <ToolDrawerItem
            label="Benchmarks"
            icon="benchmarks"
            meta={hasProject ? undefined : 'Open project'}
            active={activeView === 'benchmarks'}
            onClick={hasProject ? onBenchmarks : onOpenProject}
          />
          <ToolDrawerItem
            label="Browser"
            icon="browser"
            active={activeView === 'browser'}
            onClick={onBrowser}
          />
          <ToolDrawerItem
            label={hasProject ? 'Project chat' : 'Chat'}
            icon="chat"
            active={activeView === (hasProject ? 'project-chat' : 'local-chat')}
            onClick={onChat}
          />
        </nav>
      </aside>
    </div>
  );
}

function focusableControls(container: HTMLElement): HTMLElement[] {
  return Array.from(
    container.querySelectorAll<HTMLElement>(
      'button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), a[href], [tabindex]:not([tabindex="-1"])',
    ),
  );
}

type ToolDrawerItemProps = {
  label: string;
  icon: Extract<IconName, 'files' | 'library' | 'browser' | 'chat' | 'benchmarks'>;
  meta?: string | undefined;
  active?: boolean;
  onClick?: () => void;
};

function ToolDrawerItem({
  label,
  icon,
  meta,
  active,
  onClick,
}: ToolDrawerItemProps) {
  return (
    <button
      type="button"
      className={`plume-tool-drawer-item${active ? ' plume-tool-drawer-item-active' : ''}`}
      onClick={onClick}
      aria-current={active ? 'page' : undefined}
    >
      <Icon className="plume-tool-drawer-icon" name={icon} size={20} />
      <span className="plume-tool-drawer-label">{label}</span>
      {meta ? <span className="plume-tool-drawer-meta">{meta}</span> : null}
    </button>
  );
}
