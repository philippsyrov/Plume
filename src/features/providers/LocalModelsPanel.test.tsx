import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { render, screen, within } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { LocalModelsPanel } from './LocalModelsPanel';
import type { ProviderInventory } from './useProviderInventory';
import type { MlxServersApi, MlxServerStatus } from './useMlxServers';
import type { LocalModel } from '../../lib/api/providers';
import type { SelectedModel } from '../model-picker/useSelectedModel';

const providersCss = readFileSync(
  join(process.cwd(), 'src/styles/layout/providers.css'),
  'utf8',
);

const MODEL: LocalModel = {
  id: 'plume-model-dir:Qwen2.5-Coder-3B-Instruct-4bit',
  name: 'Qwen2.5-Coder-3B-Instruct-4bit',
  path: '/home/user/plume-models/Qwen2.5-Coder-3B-Instruct-4bit',
  kind: 'mlx-folder',
  sizeBytes: 2_000_000_000,
  source: 'plume-model-dir',
};

function inventory(models: LocalModel[]): ProviderInventory {
  return {
    state: {
      kind: 'ready',
      providers: [],
      healthById: new Map(),
      localModels: models,
      localModelError: null,
    },
    refreshing: false,
    revision: 0,
    refresh: vi.fn(),
  } as unknown as ProviderInventory;
}

function servers(status: MlxServerStatus): MlxServersApi {
  return {
    statuses: new Map(),
    statusOf: () => status,
    handleOf: () => null,
    start: vi.fn(),
    stop: vi.fn(),
    clearError: vi.fn(),
  };
}

const selectedAs = (m: LocalModel): SelectedModel => ({
  providerId: 'mlx-lm',
  providerDisplayName: 'MLX (Plume-managed)',
  modelId: m.id,
});

describe('LocalModelsPanel — D87 compact row', () => {
  it('keeps the actionable header separate from the descriptive meta line', () => {
    render(
      <LocalModelsPanel
        inventory={inventory([MODEL])}
        servers={servers({ kind: 'idle' })}
        selected={null}
        onSelect={vi.fn()}
      />,
    );

    // The toggle button holds only the name (no badges crammed inside it).
    const toggle = screen.getByRole('button', { name: /Expand details for/ });
    expect(toggle).toHaveTextContent(MODEL.name);
    expect(within(toggle).queryByText('MLX folder')).toBeNull();

    // The kind / source / size badges live on the meta line instead.
    expect(screen.getByText('MLX folder')).toBeInTheDocument();
    expect(screen.getByText('Plume')).toBeInTheDocument();
  });

  it('renders selected + port + Stop as one controls cluster when running and selected', () => {
    const running: MlxServerStatus = {
      kind: 'running',
      handle: { id: 'h1', port: 64606, pid: 4242 },
    };
    const { container } = render(
      <LocalModelsPanel
        inventory={inventory([MODEL])}
        servers={servers(running)}
        selected={selectedAs(MODEL)}
        onSelect={vi.fn()}
      />,
    );

    const controls = container.querySelector('.plume-local-models-controls');
    expect(controls).not.toBeNull();
    // All three live in the single controls cluster — not scattered or
    // wrapped over the name.
    expect(within(controls as HTMLElement).getByText('selected')).toBeInTheDocument();
    expect(within(controls as HTMLElement).getByText('port 64606')).toBeInTheDocument();
    expect(
      within(controls as HTMLElement).getByRole('button', { name: 'Stop' }),
    ).toBeInTheDocument();
  });

  it('lays out the header as a single non-wrapping row in CSS', () => {
    // The whole point of D87: the header must not wrap, so a selected +
    // running model's controls never spill under the name.
    expect(providersCss).toMatch(
      /\.plume-local-models-row-header\s*\{[^}]*flex-wrap:\s*nowrap[^}]*\}/s,
    );
    expect(providersCss).toMatch(
      /\.plume-local-models-controls\s*\{[^}]*flex-wrap:\s*nowrap[^}]*\}/s,
    );
  });
});
