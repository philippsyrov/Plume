// Thermos I1: recovery of Plume-managed MLX servers after frontend
// handle loss. A webview reload skips the unmount stops, so the hook
// adopts still-running servers from the Rust registry on mount and
// re-keys them by the inventory `modelId` recorded at start. These
// tests pin: adoption of keyed servers, the unkeyable-row skip, the
// honest failure skip, and that an adopted handle round-trips into
// `stopServer` exactly like one the same hook instance started.

import { StrictMode } from 'react';
import { act, renderHook, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { useMlxServers } from './useMlxServers';
import {
  listServers,
  startServer,
  stopServer,
  type ListServersResponse,
} from '../../lib/api/providers';

vi.mock('../../lib/api/providers', () => ({
  listServers: vi.fn(),
  startServer: vi.fn(),
  stopServer: vi.fn(),
}));

const listServersMock = vi.mocked(listServers);
const startServerMock = vi.mocked(startServer);
const stopServerMock = vi.mocked(stopServer);

function managed(overrides: Partial<ListServersResponse['servers'][number]> = {}) {
  return {
    handleId: 'srv_0000000000000001',
    port: 4242,
    pid: 999,
    modelId: 'plume-model-dir:qwen',
    modelLabel: '/models/qwen',
    startedAtMs: 1_700_000_000_000,
    uptimeMs: 5_000,
    ...overrides,
  };
}

beforeEach(() => {
  // The hook's unmount cleanup fires a best-effort stop for every
  // adopted `running` handle; give the mock a resolved default so
  // renderHook teardown never chains `.catch` off `undefined`.
  stopServerMock.mockResolvedValue({ ok: true });
});

afterEach(() => {
  vi.clearAllMocks();
});

describe('useMlxServers — managed-server recovery', () => {
  it('adopts running servers from the registry on mount, keyed by modelId', async () => {
    listServersMock.mockResolvedValue({
      servers: [
        managed(),
        // No inventory id recorded (direct Rust caller): the panel
        // cannot key a row for it, so the hook must skip it rather
        // than invent an identity.
        managed({ handleId: 'srv_0000000000000002', port: 4243, modelId: '' }),
      ],
    });

    const { result } = renderHook(() => useMlxServers());

    await waitFor(() => {
      expect(result.current.statusOf('plume-model-dir:qwen').kind).toBe('running');
    });
    expect(result.current.handleOf('plume-model-dir:qwen')).toEqual({
      id: 'srv_0000000000000001',
      port: 4242,
      pid: 999,
    });
    // Exactly one adoption: the unkeyable row must not appear
    // anywhere in the statuses map.
    expect(result.current.statuses.size).toBe(1);
  });

  it('leaves statuses idle and logs when recovery fails', async () => {
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => {});
    listServersMock.mockRejectedValue(new Error('ipc down'));

    const { result } = renderHook(() => useMlxServers());

    await waitFor(() => {
      expect(consoleError).toHaveBeenCalledWith(
        'useMlxServers: recovering managed servers failed:',
        'ipc down',
      );
    });
    expect(result.current.statuses.size).toBe(0);
    expect(result.current.statusOf('plume-model-dir:qwen').kind).toBe('idle');
    consoleError.mockRestore();
  });

  it('makes an early Start wait for recovery and reuse the adopted handle', async () => {
    // Codex #154 P2 regression: a Start click in the window while
    // listServers() is still in flight must NOT flip the model to
    // `starting` and strand the recovered running child. `start`
    // awaits recovery, sees the adopted status, and returns its
    // handle without ever calling providers.startServer.
    let resolveListing: (value: ListServersResponse) => void = () => {};
    listServersMock.mockReturnValue(
      new Promise<ListServersResponse>((resolve) => {
        resolveListing = resolve;
      }),
    );

    const { result } = renderHook(() => useMlxServers());

    let started: Promise<unknown>;
    await act(async () => {
      // Click Start BEFORE the listing resolves...
      started = result.current.start('plume-model-dir:qwen');
      // ...then let recovery land.
      resolveListing({ servers: [managed()] });
      await expect(started).resolves.toEqual({
        id: 'srv_0000000000000001',
        port: 4242,
        pid: 999,
      });
    });

    expect(startServerMock).not.toHaveBeenCalled();
    expect(result.current.statusOf('plume-model-dir:qwen').kind).toBe('running');
  });

  it('adopts servers under StrictMode, whose replay re-runs every effect', async () => {
    // Codex #154 P3 regression: main.tsx renders under StrictMode,
    // so effects run setup → cleanup → setup in dev. Pre-fix, the
    // replayed cleanup left `unmountedRef` permanently true, which
    // silently discarded recovery (and treated every later start as
    // a post-unmount race). Adoption succeeding here proves the
    // hook re-arms on the replayed setup.
    listServersMock.mockResolvedValue({ servers: [managed()] });

    const { result } = renderHook(() => useMlxServers(), { wrapper: StrictMode });

    await waitFor(() => {
      expect(result.current.statusOf('plume-model-dir:qwen').kind).toBe('running');
    });
    expect(result.current.handleOf('plume-model-dir:qwen')).toEqual({
      id: 'srv_0000000000000001',
      port: 4242,
      pid: 999,
    });
  });

  it('makes an early Start wait for recovery under StrictMode', async () => {
    // Codex #154 P3 regression, second half: the replay leaves TWO
    // recovery promises in flight. The stale one must neither adopt
    // with a stale generation nor clear the live one's `recoveryRef`
    // gate — if it did, this early Start would stop awaiting
    // recovery and spawn a duplicate server.
    let resolveListing: (value: ListServersResponse) => void = () => {};
    listServersMock.mockReturnValue(
      new Promise<ListServersResponse>((resolve) => {
        resolveListing = resolve;
      }),
    );

    const { result } = renderHook(() => useMlxServers(), { wrapper: StrictMode });

    await act(async () => {
      const started = result.current.start('plume-model-dir:qwen');
      resolveListing({ servers: [managed()] });
      await expect(started).resolves.toEqual({
        id: 'srv_0000000000000001',
        port: 4242,
        pid: 999,
      });
    });

    expect(startServerMock).not.toHaveBeenCalled();
    expect(result.current.statusOf('plume-model-dir:qwen').kind).toBe('running');
  });

  it('stops an adopted server through the recovered handle', async () => {
    listServersMock.mockResolvedValue({ servers: [managed()] });
    stopServerMock.mockResolvedValue({ ok: true });

    const { result } = renderHook(() => useMlxServers());
    await waitFor(() => {
      expect(result.current.statusOf('plume-model-dir:qwen').kind).toBe('running');
    });

    await act(async () => {
      await result.current.stop('plume-model-dir:qwen');
    });

    expect(stopServerMock).toHaveBeenCalledWith({ handleId: 'srv_0000000000000001' });
    expect(result.current.statusOf('plume-model-dir:qwen').kind).toBe('idle');
  });
});
