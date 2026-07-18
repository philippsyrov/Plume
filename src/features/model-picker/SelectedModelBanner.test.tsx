import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';

import { SelectedModelBanner } from './SelectedModelBanner';
import type { SelectedModel } from './useSelectedModel';
import type { MlxServersApi, MlxServerStatus } from '../providers/useMlxServers';
import type { ServerHandle } from '../../lib/api/providers';

const mlxSelection: SelectedModel = {
  providerId: 'mlx-lm',
  providerDisplayName: 'MLX (Plume-managed)',
  modelId: 'plume-model-dir:Qwen2.5-Coder-3B-Instruct-4bit',
};

const ollamaSelection: SelectedModel = {
  providerId: 'ollama',
  providerDisplayName: 'Ollama',
  modelId: 'qwen2.5-coder:3b',
};

function servers(status: MlxServerStatus): MlxServersApi {
  return {
    statuses: new Map(),
    statusOf: () => status,
    handleOf: () => null,
    start: vi.fn().mockResolvedValue(null),
    startCatalog: vi.fn().mockResolvedValue(null),
    stop: vi.fn().mockResolvedValue(undefined),
    clearError: vi.fn(),
  };
}

describe('SelectedModelBanner — D89 rescue', () => {
  it('drops the stale "no chat / no loading happens yet" hedging', () => {
    render(<SelectedModelBanner selected={null} onClear={vi.fn()} />);
    expect(screen.queryByText(/no chat/i)).toBeNull();
    expect(screen.queryByText(/happens yet/i)).toBeNull();
    expect(
      screen.getByText(/Pick one from the Providers or Local models panel/),
    ).toBeInTheDocument();
  });

  it('shows provider · model for a selection', () => {
    render(<SelectedModelBanner selected={ollamaSelection} onClear={vi.fn()} />);
    expect(screen.getByText('Ollama')).toBeInTheDocument();
    expect(screen.getByText('qwen2.5-coder:3b')).toBeInTheDocument();
    // A non-MLX selection has no Start/Stop affordance.
    expect(screen.queryByRole('button', { name: 'Start' })).toBeNull();
    expect(screen.queryByRole('button', { name: 'Stop' })).toBeNull();
  });

  it('offers Start for an idle MLX model and calls the bus', async () => {
    const bus = servers({ kind: 'idle' });
    render(
      <SelectedModelBanner selected={mlxSelection} onClear={vi.fn()} mlxServers={bus} />,
    );
    const start = screen.getByRole('button', { name: 'Start' });
    await userEvent.click(start);
    expect(bus.start).toHaveBeenCalledWith(mlxSelection.modelId);
  });

  it('shows running port + Stop for a running MLX model and calls the bus', async () => {
    const handle: ServerHandle = { id: 'h1', port: 64606, pid: 4242 };
    const bus = servers({ kind: 'running', handle });
    render(
      <SelectedModelBanner selected={mlxSelection} onClear={vi.fn()} mlxServers={bus} />,
    );
    expect(screen.getByText('running · port 64606')).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Stop' }));
    expect(bus.stop).toHaveBeenCalledWith(mlxSelection.modelId);
  });

  it('disables Start in no-project chat mode but keeps the model visible', () => {
    const bus = servers({ kind: 'idle' });
    render(
      <SelectedModelBanner
        selected={mlxSelection}
        onClear={vi.fn()}
        mlxServers={bus}
        noProject
      />,
    );
    const start = screen.getByRole('button', { name: 'Start' });
    expect(start).toBeDisabled();
    expect(start).toHaveAttribute(
      'title',
      'Open and trust a project to start Plume-managed runtimes.',
    );
    // The model id is still shown — chat-only mode just can't start it.
    expect(screen.getByText(mlxSelection.modelId)).toBeInTheDocument();
  });

  it('still offers Stop for a running model in no-project mode', async () => {
    // Stop is an ungated cleanup verb — a server started in a trusted
    // session stays stoppable after switching to chat-only.
    const handle: ServerHandle = { id: 'h1', port: 5000, pid: 9 };
    const bus = servers({ kind: 'running', handle });
    render(
      <SelectedModelBanner
        selected={mlxSelection}
        onClear={vi.fn()}
        mlxServers={bus}
        noProject
      />,
    );
    const stop = screen.getByRole('button', { name: 'Stop' });
    expect(stop).not.toBeDisabled();
    await userEvent.click(stop);
    expect(bus.stop).toHaveBeenCalledWith(mlxSelection.modelId);
  });

  it('surfaces a start error and re-offers Start', () => {
    const bus = servers({ kind: 'error', message: 'mlx-lm exited (1)' });
    render(
      <SelectedModelBanner selected={mlxSelection} onClear={vi.fn()} mlxServers={bus} />,
    );
    expect(screen.getByText('mlx-lm exited (1)')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Start' })).toBeInTheDocument();
  });
});
