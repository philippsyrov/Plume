// D63B: the Tools drawer toggle is the top-bar entry to project
// capabilities (Files view, memory, agent panels). Project chat keeps
// it; the simple/local chat surface hides it — the scope boundary the
// D63 spec pins ("simple chats never expose ... the project tool
// drawer").

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

function renderTopBar(showTools: boolean) {
  render(
    <UnifiedTopBar
      subtitle={showTools ? 'plume-demo' : 'Simple chat'}
      inventory={inventory}
      servers={servers}
      selected={null}
      onSelect={vi.fn()}
      toolsOpen={false}
      showTools={showTools}
      onToggleTools={vi.fn()}
      onOpenProject={vi.fn()}
    />,
  );
}

describe('UnifiedTopBar tools access', () => {
  it('project surfaces keep the project-tools toggle', () => {
    renderTopBar(true);
    expect(screen.getByRole('button', { name: 'Open project tools' })).toBeInTheDocument();
  });

  it('the simple chat surface hides the project-tools toggle', () => {
    renderTopBar(false);
    expect(
      screen.queryByRole('button', { name: /project tools/i }),
    ).not.toBeInTheDocument();
  });
});
