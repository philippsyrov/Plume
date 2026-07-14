// The workspace-views drawer toggle is the top-bar entry to project
// navigation (Files, Benchmarks, and Project chat). Project chat keeps
// it; the simple/local chat surface hides it — the scope boundary the
// D63 spec pins ("simple chats never expose ... the project
// drawer").

import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { act, render, renderHook, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';

import type { ProviderInventory } from '../providers/useProviderInventory';
import type { MlxServersApi } from '../providers/useMlxServers';
import {
  HelpPanel,
  readSidebarCollapsed,
  topbarSubtitle,
  UnifiedTopBar,
  useSidebarPreference,
  writeSidebarCollapsed,
} from './UnifiedChrome';

const inventory: ProviderInventory = {
  state: { kind: 'loading' },
  refreshing: false,
  revision: 0,
  load: vi.fn().mockResolvedValue(undefined),
};

const servers: MlxServersApi = {
  statuses: new Map(),
  statusOf: () => ({ kind: 'idle' }),
  handleOf: () => null,
  start: vi.fn().mockResolvedValue(null),
  stop: vi.fn().mockResolvedValue(undefined),
  clearError: vi.fn(),
};

function renderTopBar(showTools: boolean, toolsOpen = false, showOpenProject = true) {
  render(
    <UnifiedTopBar
      subtitle={showTools ? 'plume-demo' : 'Simple chat'}
      inventory={inventory}
      servers={servers}
      selected={null}
      onSelect={vi.fn()}
      toolsOpen={toolsOpen}
      showTools={showTools}
      showOpenProject={showOpenProject}
      onToggleTools={vi.fn()}
      onOpenProject={vi.fn()}
    />,
  );
}

describe('UnifiedTopBar workspace views access', () => {
  it('owns one visible task title without repeating the Plume identity', () => {
    renderTopBar(true);
    expect(screen.getAllByRole('heading', { name: 'plume-demo' })).toHaveLength(1);
    expect(screen.queryByRole('heading', { name: 'Plume' })).not.toBeInTheDocument();
  });

  it('keeps window dragging on an empty region and opts controls out', () => {
    const { container } = render(
      <UnifiedTopBar
        subtitle="Browser"
        inventory={inventory}
        servers={servers}
        selected={null}
        onSelect={vi.fn()}
        toolsOpen={false}
        showTools
        showOpenProject
        onToggleTools={vi.fn()}
        onOpenProject={vi.fn()}
      />,
    );

    const dragRegion = container.querySelector('[data-tauri-drag-region="true"]');
    expect(dragRegion).toBeInTheDocument();
    expect(dragRegion).toBeEmptyDOMElement();
    for (const control of screen.getAllByRole('button')) {
      expect(control).toHaveAttribute('data-tauri-drag-region', 'false');
    }
  });

  it('labels the existing knowledge surface as Library for users', () => {
    expect(topbarSubtitle('knowledge', 'plume-demo')).toBe('Library');
  });

  it('keeps the exact selected task title while Browser is open', () => {
    expect(topbarSubtitle('browser', 'plume-demo', 'Investigate checkout race')).toBe(
      'Investigate checkout race',
    );
  });

  it('falls back to Browser when no selected task summary matches', () => {
    expect(topbarSubtitle('browser', 'plume-demo', null)).toBe('Browser');
  });

  it('does not let a task title replace another destination label', () => {
    expect(topbarSubtitle('files', 'plume-demo', 'Investigate checkout race')).toBe('Files');
    expect(topbarSubtitle('knowledge', 'plume-demo', 'Investigate checkout race')).toBe(
      'Library',
    );
    expect(topbarSubtitle('project-chat', 'plume-demo', 'Investigate checkout race')).toBe(
      'plume-demo',
    );
  });

  it('project surfaces keep the workspace-views toggle', () => {
    renderTopBar(true);
    expect(screen.getByRole('button', { name: 'Open workspace views' })).toHaveAttribute(
      'title',
      'Workspace views',
    );
  });

  it('names the toggle as a close action while the drawer is open', () => {
    renderTopBar(true, true);
    expect(screen.getByRole('button', { name: 'Close workspace views' })).toBeInTheDocument();
  });

  it('the simple chat surface hides the workspace-views toggle', () => {
    renderTopBar(false, false, false);
    expect(
      screen.queryByRole('button', { name: /workspace views/i }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole('button', { name: 'Open a project' }),
    ).not.toBeInTheDocument();
  });

  it('project mode keeps Open a project as the switch-project action', () => {
    renderTopBar(true);
    expect(screen.getByRole('button', { name: 'Open a project' })).toBeInTheDocument();
  });
});

describe('macOS titlebar configuration', () => {
  it('uses Tauri 2.11.1 overlay chrome without a duplicate native title', () => {
    const config = JSON.parse(
      readFileSync(join(process.cwd(), 'src-tauri/tauri.conf.json'), 'utf8'),
    ) as {
      app: {
        windows: Array<{
          label: string;
          titleBarStyle?: string;
          hiddenTitle?: boolean;
          decorations?: boolean;
          transparent?: boolean;
        }>;
      };
    };
    const cargoLock = readFileSync(join(process.cwd(), 'src-tauri/Cargo.lock'), 'utf8');
    const mainWindow = config.app.windows.find(({ label }) => label === 'main');

    expect(cargoLock).toMatch(/name = "tauri"\nversion = "2\.11\.1"/);
    expect(mainWindow).toMatchObject({
      titleBarStyle: 'Overlay',
      hiddenTitle: true,
      decorations: true,
      transparent: false,
    });
  });
});

describe('sidebar preference', () => {
  it('persists only the collapsed boolean', () => {
    writeSidebarCollapsed(true);
    expect(localStorage.getItem('plume:sidebar-v1')).toBe('{"collapsed":true}');
  });

  it('restores a valid preference and falls back expanded for invalid JSON', () => {
    localStorage.setItem('plume:sidebar-v1', '{"collapsed":true}');
    expect(readSidebarCollapsed()).toBe(true);
    localStorage.setItem('plume:sidebar-v1', '{not json');
    expect(readSidebarCollapsed()).toBe(false);
    localStorage.setItem('plume:sidebar-v1', '{"collapsed":"yes"}');
    expect(readSidebarCollapsed()).toBe(false);
  });

  it('keeps the in-memory collapse state usable when storage throws', () => {
    localStorage.removeItem('plume:sidebar-v1');
    const setItem = vi.spyOn(window.localStorage, 'setItem').mockImplementation(() => {
      throw new Error('storage unavailable');
    });
    const { result } = renderHook(() => useSidebarPreference());

    expect(() => act(() => result.current[1](true))).not.toThrow();
    expect(result.current[0]).toBe(true);
    setItem.mockRestore();
  });
});

describe('HelpPanel', () => {
  it('briefly explains Chat, Project, Library, and Browser without claiming a Handbook', async () => {
    const onClose = vi.fn();
    render(<HelpPanel onClose={onClose} />);

    expect(screen.getByRole('dialog', { name: 'Help' })).toBeInTheDocument();
    expect(screen.getByText(/Chat works without project context/)).toBeInTheDocument();
    expect(screen.getByText(/Project uses the trusted folder/)).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Library' })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Browser' })).toBeInTheDocument();
    expect(screen.queryByText(/Handbook/i)).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Close help' }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('moves focus inside, traps Tab, and closes on Escape', async () => {
    const onClose = vi.fn();
    render(
      <>
        <button type="button">Background action</button>
        <HelpPanel onClose={onClose} />
      </>,
    );
    const close = screen.getByRole('button', { name: 'Close help' });

    await waitFor(() => expect(close).toHaveFocus());
    await userEvent.tab();
    expect(close).toHaveFocus();
    expect(screen.getByRole('button', { name: 'Background action' })).not.toHaveFocus();

    await userEvent.keyboard('{Escape}');
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});

describe('project settings skills wiring', () => {
  it('keeps project-local skills inside Settings', () => {
    const source = readFileSync(
      join(process.cwd(), 'src/features/project-shell/UnifiedChrome.tsx'),
      'utf8',
    );

    expect(source).toContain('<SkillsPanel />');
    expect(source).toContain('project memory, and skills.');
  });
});
