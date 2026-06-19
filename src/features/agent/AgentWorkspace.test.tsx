import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { AgentWorkspace } from './AgentWorkspace';
import type { SelectedModel } from '../model-picker/useSelectedModel';
import type { MlxServersApi } from '../providers/useMlxServers';

// Stub the chat panel — this suite is about the workspace shell (D87
// cleanup), not chat plumbing. The stub renders a recognizable textarea
// so we can assert chat still has a home in the center zone.
vi.mock('../chat/ChatPanel', () => ({
  ChatPanel: () => <textarea aria-label="Message to send" />,
}));

const selection: SelectedModel = {
  providerId: 'mlx-lm',
  providerDisplayName: 'MLX (Plume-managed)',
  modelId: 'plume-model-dir:Qwen2.5-Coder-3B-Instruct-4bit',
};

function mlxServers(): MlxServersApi {
  return {
    statuses: new Map(),
    statusOf: () => ({ kind: 'idle' }),
    handleOf: () => null,
    start: vi.fn(),
    stop: vi.fn(),
    clearError: vi.fn(),
  };
}

function renderWorkspace(selected: SelectedModel | null) {
  return render(
    <AgentWorkspace
      selected={selected}
      onClearSelection={vi.fn()}
      inspectorSelection={null}
      inspectorLineRange={null}
      projectHasInstructions={false}
      mlxServers={mlxServers()}
    />,
  );
}

describe('AgentWorkspace — D87 cleanup', () => {
  it('keeps the chat textarea visible with a model selected', () => {
    renderWorkspace(selection);
    expect(screen.getByLabelText('Message to send')).toBeVisible();
  });

  it('no longer renders the descriptive mode-card grid or footnote', () => {
    renderWorkspace(selection);
    // The old grid carried aria-label="Agent modes" and per-card status
    // badges; the footnote pointed at the docs. None should remain.
    expect(screen.queryByRole('list', { name: 'Agent modes' })).toBeNull();
    expect(screen.queryByText('not yet implemented')).toBeNull();
    expect(screen.queryByText('preview only — apply not yet')).toBeNull();
    expect(screen.queryByText(/Every future mode will still flow/)).toBeNull();
  });

  it('points the orientation line at the chat header and the Agent card', () => {
    renderWorkspace(selection);
    expect(screen.getByText(/Pick the response mode in the chat header/)).toBeInTheDocument();
  });

  it('shows the calm empty-state line before a model is picked', () => {
    renderWorkspace(null);
    expect(
      screen.getByText('Pick a model on the left to start chatting.'),
    ).toBeInTheDocument();
  });
});
