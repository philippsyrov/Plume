import { StrictMode } from 'react';
import { act, renderHook, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import {
  QWEN_CATALOG_ID,
  type AppleAvailability,
  type CatalogDownloadEvent,
  type CatalogEntry,
  type ServerHandle,
} from '../../lib/api/providers';
import { useModelCatalog, type ModelCatalogDependencies } from './useModelCatalog';

const APPLE_ID = 'apple-system';

const apple = (state: CatalogEntry['state'] = 'available'): CatalogEntry => ({
  id: APPLE_ID,
  displayName: 'Apple On-Device',
  subtitle: 'Built into this Mac',
  providerId: 'apple-foundation',
  modelId: 'system',
  state,
  availabilityReason: state === 'available' ? null : 'Apple model is not ready.',
  downloadBytes: null,
  license: 'Apple terms',
  sourceUrl: null,
  revision: null,
});

const qwen = (state: CatalogEntry['state'] = 'absent'): CatalogEntry => ({
  id: QWEN_CATALOG_ID,
  displayName: 'Qwen Coder 1.5B',
  subtitle: 'Recommended for coding',
  providerId: 'mlx-lm',
  modelId: 'mlx-community/Qwen2.5-Coder-1.5B-Instruct-4bit',
  state,
  availabilityReason: null,
  downloadBytes: 868_628_559,
  license: 'Apache-2.0',
  sourceUrl: 'https://huggingface.co/mlx-community/Qwen2.5-Coder-1.5B-Instruct-4bit',
  revision: 'b3252a2f97102b1fb1571fec2c9b27219a8536be',
});

function deferred<T>() {
  let resolve: (value: T) => void = () => {};
  const promise = new Promise<T>((next) => {
    resolve = next;
  });
  return { promise, resolve };
}

function setup(overrides: Partial<ModelCatalogDependencies> = {}) {
  const listeners = new Set<(event: CatalogDownloadEvent) => void>();
  const selected = vi.fn();
  const handle: ServerHandle = { id: 'srv_qwen', port: 62000, pid: 99 };
  const deps: ModelCatalogDependencies = {
    listCatalogModels: vi.fn().mockResolvedValue([apple(), qwen()]),
    getAppleAvailability: vi.fn().mockResolvedValue({
      available: true,
      reason: null,
      detail: null,
    } satisfies AppleAvailability),
    downloadCatalogModel: vi.fn().mockResolvedValue({ operationId: 'download-1' }),
    cancelCatalogDownload: vi.fn().mockResolvedValue({ ok: true }),
    removeCatalogModel: vi.fn().mockResolvedValue({ removed: true }),
    subscribeCatalogDownloads: vi.fn().mockImplementation(async (next) => {
      listeners.add(next);
      return () => {
        listeners.delete(next);
      };
    }),
    mlxServers: {
      statuses: new Map(),
      statusOf: () => ({ kind: 'idle' }),
      handleOf: () => null,
      start: vi.fn().mockResolvedValue(null),
      startCatalog: vi.fn().mockResolvedValue(handle),
      stop: vi.fn().mockResolvedValue(undefined),
      clearError: vi.fn(),
    },
    selectedModel: { selected: null, select: selected, clear: vi.fn() },
    ...overrides,
  };
  return {
    deps,
    selected,
    handle,
    activeListenerCount: () => listeners.size,
    emit: (event: CatalogDownloadEvent) => listeners.forEach((listener) => listener(event)),
  };
}

describe('useModelCatalog', () => {
  it('keeps one authoritative listing and listener during the StrictMode replay', async () => {
    const firstListing = deferred<CatalogEntry[]>();
    const secondListing = deferred<CatalogEntry[]>();
    const listCatalogModels = vi.fn()
      .mockReturnValueOnce(firstListing.promise)
      .mockReturnValueOnce(secondListing.promise);
    const { deps, activeListenerCount } = setup({ listCatalogModels });
    const { result, unmount } = renderHook(() => useModelCatalog(deps), { wrapper: StrictMode });

    await waitFor(() => expect(listCatalogModels).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(deps.subscribeCatalogDownloads).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(activeListenerCount()).toBe(1));

    await act(async () => {
      secondListing.resolve([apple(), qwen('installed')]);
      await Promise.resolve();
    });
    await waitFor(() => expect(result.current.entry(QWEN_CATALOG_ID)?.state).toBe('installed'));

    await act(async () => {
      firstListing.resolve([apple(), qwen('absent')]);
      await Promise.resolve();
    });
    expect(result.current.entry(QWEN_CATALOG_ID)?.state).toBe('installed');

    unmount();
    expect(activeListenerCount()).toBe(0);
  });

  it('ignores late events from a cancelled download generation', async () => {
    const { deps, emit } = setup();
    vi.mocked(deps.downloadCatalogModel)
      .mockResolvedValueOnce({ operationId: 'old-download' })
      .mockResolvedValueOnce({ operationId: 'new-download' });
    const { result } = renderHook(() => useModelCatalog(deps));
    await waitFor(() => expect(result.current.entry(QWEN_CATALOG_ID)).not.toBeNull());

    await act(async () => {
      await result.current.download(QWEN_CATALOG_ID);
      await result.current.cancelDownload(QWEN_CATALOG_ID);
    });
    await act(async () => {
      await result.current.download(QWEN_CATALOG_ID);
      emit({
        operationId: 'old-download',
        seq: 3,
        catalogId: QWEN_CATALOG_ID,
        phase: 'installed',
        downloadedBytes: 868_628_559,
        totalBytes: 868_628_559,
        error: null,
      });
    });

    expect(result.current.entry(QWEN_CATALOG_ID)?.state).toBe('downloading');
  });

  it('applies the matching first download event that arrives before start resolves', async () => {
    const start = deferred<{ operationId: string }>();
    const { deps, emit } = setup({ downloadCatalogModel: vi.fn().mockReturnValue(start.promise) });
    const { result } = renderHook(() => useModelCatalog(deps));
    await waitFor(() => expect(result.current.entry(QWEN_CATALOG_ID)).not.toBeNull());

    let download: Promise<void>;
    await act(async () => {
      download = result.current.download(QWEN_CATALOG_ID);
      emit({
        operationId: 'racing-download',
        seq: 1,
        catalogId: QWEN_CATALOG_ID,
        phase: 'verifying',
        downloadedBytes: 868_628_559,
        totalBytes: 868_628_559,
        error: null,
      });
      start.resolve({ operationId: 'racing-download' });
      await download;
    });

    expect(result.current.entry(QWEN_CATALOG_ID)).toMatchObject({
      state: 'verifying',
      downloadedBytes: 868_628_559,
    });
  });

  it('selects Qwen only after catalog start returns an exact handle', async () => {
    const { deps, selected, handle } = setup();
    const { result } = renderHook(() => useModelCatalog(deps));
    await waitFor(() => expect(result.current.entry(QWEN_CATALOG_ID)).not.toBeNull());

    await act(async () => {
      await result.current.useQwen();
    });

    expect(deps.mlxServers.startCatalog).toHaveBeenCalledWith(QWEN_CATALOG_ID);
    expect(deps.mlxServers.handleOf(QWEN_CATALOG_ID)).not.toEqual(handle);
    expect(selected).toHaveBeenCalledWith({
      providerId: 'mlx-lm',
      providerDisplayName: 'Qwen Coder',
      modelId: QWEN_CATALOG_ID,
    });
  });

  it('does not select Qwen when catalog start returns no exact handle', async () => {
    const { deps, selected } = setup({ listCatalogModels: vi.fn().mockResolvedValue([apple(), qwen('installed')]) });
    deps.mlxServers.startCatalog = vi.fn().mockResolvedValue(null);
    const { result } = renderHook(() => useModelCatalog(deps));
    await waitFor(() => expect(result.current.entry(QWEN_CATALOG_ID)).not.toBeNull());

    await act(async () => {
      await result.current.useQwen();
    });

    expect(selected).not.toHaveBeenCalled();
    expect(result.current.entry(QWEN_CATALOG_ID)).toMatchObject({
      state: 'installed',
      error: 'Could not start Qwen. Try again.',
    });
  });

  it('does not select Qwen when catalog start rejects', async () => {
    const { deps, selected } = setup({ listCatalogModels: vi.fn().mockResolvedValue([apple(), qwen('installed')]) });
    deps.mlxServers.startCatalog = vi.fn().mockRejectedValue(new Error('runtime unavailable'));
    const { result } = renderHook(() => useModelCatalog(deps));
    await waitFor(() => expect(result.current.entry(QWEN_CATALOG_ID)).not.toBeNull());

    await act(async () => {
      await result.current.useQwen();
    });

    expect(selected).not.toHaveBeenCalled();
    expect(result.current.entry(QWEN_CATALOG_ID)).toMatchObject({
      state: 'installed',
      error: 'runtime unavailable',
    });
  });

  it('rejects a non-monotonic download event sequence', async () => {
    const { deps, emit } = setup();
    const { result } = renderHook(() => useModelCatalog(deps));
    await waitFor(() => expect(result.current.entry(QWEN_CATALOG_ID)).not.toBeNull());

    await act(async () => {
      await result.current.download(QWEN_CATALOG_ID);
      emit({
        operationId: 'download-1',
        seq: 2,
        catalogId: QWEN_CATALOG_ID,
        phase: 'downloading',
        downloadedBytes: 200,
        totalBytes: 300,
        error: null,
      });
      emit({
        operationId: 'download-1',
        seq: 1,
        catalogId: QWEN_CATALOG_ID,
        phase: 'verifying',
        downloadedBytes: 300,
        totalBytes: 300,
        error: null,
      });
    });

    expect(result.current.entry(QWEN_CATALOG_ID)).toMatchObject({
      state: 'downloading',
      downloadedBytes: 200,
    });
  });

  it('refreshes authoritative catalog state after a terminal event', async () => {
    const listCatalogModels = vi.fn()
      .mockResolvedValueOnce([apple(), qwen('absent')])
      .mockResolvedValueOnce([apple(), qwen('installed')]);
    const { deps, emit } = setup({ listCatalogModels });
    const { result } = renderHook(() => useModelCatalog(deps));
    await waitFor(() => expect(result.current.entry(QWEN_CATALOG_ID)?.state).toBe('absent'));

    await act(async () => {
      await result.current.download(QWEN_CATALOG_ID);
      emit({
        operationId: 'download-1',
        seq: 1,
        catalogId: QWEN_CATALOG_ID,
        phase: 'installed',
        downloadedBytes: 868_628_559,
        totalBytes: 868_628_559,
        error: null,
      });
    });

    await waitFor(() => expect(listCatalogModels).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(result.current.entry(QWEN_CATALOG_ID)?.state).toBe('installed'));
  });

  it('refreshes Apple availability and does not select an unavailable model', async () => {
    const { deps, selected } = setup({
      getAppleAvailability: vi.fn().mockResolvedValue({
        available: false,
        reason: 'model-not-ready',
        detail: 'Apple model is still getting ready.',
      } satisfies AppleAvailability),
    });
    const { result } = renderHook(() => useModelCatalog(deps));
    await waitFor(() => expect(result.current.entry(APPLE_ID)).not.toBeNull());

    await act(async () => {
      await result.current.useApple();
    });

    expect(result.current.entry(APPLE_ID)).toMatchObject({
      state: 'unavailable',
      availabilityReason: 'Apple model is still getting ready.',
    });
    expect(selected).not.toHaveBeenCalled();
  });

  it('shows an honest Apple availability error and does not select after its IPC rejects', async () => {
    const { deps, selected } = setup({
      getAppleAvailability: vi.fn().mockRejectedValue(new Error('helper could not start')),
    });
    const { result } = renderHook(() => useModelCatalog(deps));
    await waitFor(() => expect(result.current.entry(APPLE_ID)).not.toBeNull());

    await act(async () => {
      await result.current.useApple();
    });

    expect(result.current.entry(APPLE_ID)).toMatchObject({
      state: 'unavailable',
      availabilityReason: 'Could not check Apple model availability.',
      error: 'helper could not start',
    });
    expect(selected).not.toHaveBeenCalled();
  });
});
