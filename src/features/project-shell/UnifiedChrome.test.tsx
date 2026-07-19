// The workspace-views drawer toggle is the top-bar entry to project
// navigation (Files, Benchmarks, and Project chat). Project chat keeps
// it; the simple/local chat surface hides it — the scope boundary the
// D63 spec pins ("simple chats never expose ... the project
// drawer").

import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { act, render, renderHook, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import type { ModelCatalogApi } from '../model-picker/useModelCatalog';
import type { SelectedModelApi } from '../model-picker/useSelectedModel';
import {
  readSidebarCollapsed,
  topbarSubtitle,
  UnifiedTopBar,
  useSidebarPreference,
  writeSidebarCollapsed,
} from './UnifiedChrome';

const catalog: ModelCatalogApi = {
  entries: [],
  entry: () => null,
  loading: false,
  downloadEventsReady: true,
  error: null,
  download: vi.fn().mockResolvedValue(undefined),
  cancelDownload: vi.fn().mockResolvedValue(undefined),
  useApple: vi.fn().mockResolvedValue(undefined),
  useQwen: vi.fn().mockResolvedValue(undefined),
  removeQwen: vi.fn().mockResolvedValue(undefined),
  refresh: vi.fn().mockResolvedValue(undefined),
};

const selection: SelectedModelApi = {
  selected: null,
  select: vi.fn(),
  clear: vi.fn(),
  revision: () => 0,
};

function renderTopBar(showTools: boolean, toolsOpen = false, showOpenProject = true) {
  render(
    <UnifiedTopBar
      subtitle={showTools ? 'plume-demo' : 'Simple chat'}
      catalog={catalog}
      selection={selection}
      modelChooserOpen={false}
      onModelChooserOpenChange={vi.fn()}
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
        catalog={catalog}
        selection={selection}
        modelChooserOpen={false}
        onModelChooserOpenChange={vi.fn()}
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

  it('labels the Library surface directly', () => {
    expect(topbarSubtitle('library', 'plume-demo')).toBe('Library');
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
    expect(topbarSubtitle('library', 'plume-demo', 'Investigate checkout race')).toBe(
      'Library',
    );
  });

  it('uses the selected task title for chats without repeating scope jargon', () => {
    expect(topbarSubtitle('local-chat', null, 'Plan a weekend')).toBe('Plan a weekend');
    expect(topbarSubtitle('project-chat', 'plume-demo', 'Investigate checkout race')).toBe(
      'Investigate checkout race',
    );
    expect(topbarSubtitle('local-chat', null, null)).toBe('Chat');
    expect(topbarSubtitle('project-chat', 'plume-demo', null)).toBe('plume-demo');
    expect(topbarSubtitle('project-chat', null, null)).toBe('Project');
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

  it('keeps modal headers on a theme-aware opaque surface', () => {
    const css = readFileSync(
      join(process.cwd(), 'src/styles/layout/surfaces.css'),
      'utf8',
    );
    const header = css.match(/\.plume-project-settings-header\s*\{([^}]*)\}/s)?.[1] ?? '';

    expect(header).toMatch(/background:\s*var\(--surface-muted\)/);
    expect(header).not.toMatch(/rgba\(244,\s*242,\s*235/);
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

describe('project settings skills wiring', () => {
  it('keeps project-local skills inside Settings', () => {
    const source = readFileSync(
      join(process.cwd(), 'src/features/project-shell/UnifiedChrome.tsx'),
      'utf8',
    );

    expect(source).toContain('<SkillsPanel />');
    expect(source).toContain('<details className="plume-project-settings-advanced">');
    expect(source).toContain('<summary>Advanced project tools</summary>');
    expect(source).not.toContain('AgentDryRunPanel');
    expect(source).not.toContain('Event stream dry-run');
    expect(source).toContain('Local models, Library, and advanced project tools.');
    expect(source).toContain('<LibrarySettingsPanel projectAvailable />');
    expect(source).toContain('<LibrarySettingsPanel projectAvailable={false} />');
    expect(source).not.toContain('<MemoryPanel />');
  });
});
