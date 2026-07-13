// The workspace-views drawer toggle is the top-bar entry to project
// navigation (Files, Benchmarks, and Project chat). Project chat keeps
// it; the simple/local chat surface hides it — the scope boundary the
// D63 spec pins ("simple chats never expose ... the project
// drawer").

import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import type { ProviderInventory } from '../providers/useProviderInventory';
import type { MlxServersApi } from '../providers/useMlxServers';
import { UnifiedTopBar } from './UnifiedChrome';

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
