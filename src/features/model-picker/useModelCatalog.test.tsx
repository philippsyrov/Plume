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
import { useModelCatalog } from './useModelCatalog';
import { APPLE_ID, apple, deferred, qwen, setup } from './useModelCatalog.test-support';

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
      emit({
        operationId: 'old-download',
        seq: 2,
        catalogId: QWEN_CATALOG_ID,
        phase: 'cancelled',
        downloadedBytes: 100,
        totalBytes: 300,
        error: null,
      });
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
      state: 'start-failed',
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
      state: 'start-failed',
      error: 'runtime unavailable',
    });
  });

  it('projects an installed Qwen as starting while its managed startup is in flight', async () => {
    const start = deferred<ServerHandle | null>();
    const { deps } = setup({ listCatalogModels: vi.fn().mockResolvedValue([apple(), qwen('installed')]) });
    deps.mlxServers.startCatalog = vi.fn().mockReturnValue(start.promise);
    const { result } = renderHook(() => useModelCatalog(deps));
    await waitFor(() => expect(result.current.entry(QWEN_CATALOG_ID)?.state).toBe('installed'));

    void result.current.useQwen();

    await waitFor(() => expect(result.current.entry(QWEN_CATALOG_ID)?.state).toBe('starting'));
    expect(deps.mlxServers.startCatalog).toHaveBeenCalledTimes(1);
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

  it('keeps active verifying progress and cancellation ownership across a manual receipt refresh', async () => {
    const listCatalogModels = vi.fn()
      .mockResolvedValueOnce([apple(), qwen('absent')])
      .mockResolvedValueOnce([apple(), qwen('absent')]);
    const { deps, emit } = setup({ listCatalogModels });
    const { result } = renderHook(() => useModelCatalog(deps));
    await waitFor(() => expect(result.current.entry(QWEN_CATALOG_ID)?.state).toBe('absent'));

    await act(async () => {
      await result.current.download(QWEN_CATALOG_ID);
      emit({
        operationId: 'download-1',
        seq: 1,
        catalogId: QWEN_CATALOG_ID,
        phase: 'verifying',
        downloadedBytes: 868_628_559,
        totalBytes: 868_628_559,
        error: null,
      });
      await result.current.refresh();
    });

    expect(result.current.entry(QWEN_CATALOG_ID)).toMatchObject({
      state: 'verifying',
      operationId: 'download-1',
      downloadedBytes: 868_628_559,
    });
    await act(async () => {
      await result.current.cancelDownload(QWEN_CATALOG_ID);
    });
    expect(deps.cancelCatalogDownload).toHaveBeenCalledWith('download-1');
  });

  it('keeps a failed download retryable after receipt-only refresh reports absent', async () => {
    const refresh = deferred<CatalogEntry[]>();
    const listCatalogModels = vi.fn()
      .mockResolvedValueOnce([apple(), qwen('absent')])
      .mockReturnValueOnce(refresh.promise);
    const { deps, emit } = setup({ listCatalogModels });
    const { result } = renderHook(() => useModelCatalog(deps));
    await waitFor(() => expect(result.current.entry(QWEN_CATALOG_ID)?.state).toBe('absent'));

    await act(async () => {
      await result.current.download(QWEN_CATALOG_ID);
      emit({
        operationId: 'download-1',
        seq: 1,
        catalogId: QWEN_CATALOG_ID,
        phase: 'failed',
        downloadedBytes: 200,
        totalBytes: 300,
        error: 'Network lost.',
      });
      refresh.resolve([apple(), qwen('absent')]);
      await Promise.resolve();
    });

    await waitFor(() => expect(result.current.entry(QWEN_CATALOG_ID)).toMatchObject({
      state: 'failed',
      error: 'Network lost.',
    }));
  });

  it('does not let a failed operation refresh overwrite a newer retry intent', async () => {
    const staleRefresh = deferred<CatalogEntry[]>();
    const retryStart = deferred<{ operationId: string }>();
    const listCatalogModels = vi.fn()
      .mockResolvedValueOnce([apple(), qwen('absent')])
      .mockReturnValueOnce(staleRefresh.promise);
    const { deps, emit } = setup({
      listCatalogModels,
      downloadCatalogModel: vi.fn()
        .mockResolvedValueOnce({ operationId: 'failed-download' })
        .mockReturnValueOnce(retryStart.promise),
    });
    const { result } = renderHook(() => useModelCatalog(deps));
    await waitFor(() => expect(result.current.entry(QWEN_CATALOG_ID)?.state).toBe('absent'));

    await act(async () => {
      await result.current.download(QWEN_CATALOG_ID);
      emit({
        operationId: 'failed-download',
        seq: 1,
        catalogId: QWEN_CATALOG_ID,
        phase: 'failed',
        downloadedBytes: 200,
        totalBytes: 300,
        error: 'Network lost.',
      });
      void result.current.download(QWEN_CATALOG_ID);
      staleRefresh.resolve([apple(), qwen('absent')]);
      await Promise.resolve();
    });

    expect(result.current.entry(QWEN_CATALOG_ID)?.state).toBe('downloading');
    await act(async () => {
      retryStart.resolve({ operationId: 'retry-download' });
      await Promise.resolve();
    });
  });

  it('keeps cancellation active until its terminal event releases retry', async () => {
    const { deps, emit } = setup({
      downloadCatalogModel: vi.fn()
        .mockResolvedValueOnce({ operationId: 'old-download' })
        .mockResolvedValueOnce({ operationId: 'retry-download' }),
    });
    const { result } = renderHook(() => useModelCatalog(deps));
    await waitFor(() => expect(result.current.entry(QWEN_CATALOG_ID)).not.toBeNull());

    await act(async () => {
      await result.current.download(QWEN_CATALOG_ID);
      await result.current.cancelDownload(QWEN_CATALOG_ID);
      await result.current.download(QWEN_CATALOG_ID);
    });
    expect(deps.downloadCatalogModel).toHaveBeenCalledTimes(1);

    await act(async () => {
      emit({
        operationId: 'old-download',
        seq: 1,
        catalogId: QWEN_CATALOG_ID,
        phase: 'cancelled',
        downloadedBytes: 100,
        totalBytes: 300,
        error: null,
      });
      await result.current.download(QWEN_CATALOG_ID);
      emit({
        operationId: 'old-download',
        seq: 2,
        catalogId: QWEN_CATALOG_ID,
        phase: 'installed',
        downloadedBytes: 300,
        totalBytes: 300,
        error: null,
      });
    });

    expect(deps.downloadCatalogModel).toHaveBeenCalledTimes(2);
    expect(result.current.entry(QWEN_CATALOG_ID)?.state).toBe('downloading');
  });

  it('restores the same live operation after cancellation acknowledgement rejects', async () => {
    const { deps } = setup({ cancelCatalogDownload: vi.fn().mockRejectedValueOnce(new Error('IPC offline')).mockResolvedValue({ ok: true }) });
    const { result } = renderHook(() => useModelCatalog(deps));
    await waitFor(() => expect(result.current.entry(QWEN_CATALOG_ID)).not.toBeNull());

    await act(async () => {
      await result.current.download(QWEN_CATALOG_ID);
      await result.current.cancelDownload(QWEN_CATALOG_ID);
    });
    expect(result.current.entry(QWEN_CATALOG_ID)).toMatchObject({
      state: 'downloading',
      error: 'Could not cancel download. Try again.',
    });

    await act(async () => {
      await result.current.cancelDownload(QWEN_CATALOG_ID);
    });
    expect(deps.cancelCatalogDownload).toHaveBeenCalledTimes(2);
  });

  it('keeps a later Qwen selection when a slower Apple availability check resolves', async () => {
    const availability = deferred<AppleAvailability>();
    const { deps, selected } = setup({ getAppleAvailability: vi.fn().mockReturnValue(availability.promise) });
    const { result } = renderHook(() => useModelCatalog(deps));
    await waitFor(() => expect(result.current.entry(APPLE_ID)).not.toBeNull());

    void result.current.useApple();
    await act(async () => {
      await result.current.useQwen();
      availability.resolve({ available: true, reason: null, detail: null });
      await Promise.resolve();
    });

    expect(selected).toHaveBeenCalledTimes(1);
    expect(selected).toHaveBeenCalledWith({
      providerId: 'mlx-lm',
      providerDisplayName: 'Qwen Coder',
      modelId: QWEN_CATALOG_ID,
    });
  });

  it('keeps a later Apple selection when a slower Qwen start resolves', async () => {
    const start = deferred<ServerHandle | null>();
    const { deps, selected } = setup();
    deps.mlxServers.startCatalog = vi.fn().mockReturnValue(start.promise);
    const { result } = renderHook(() => useModelCatalog(deps));
    await waitFor(() => expect(result.current.entry(APPLE_ID)).not.toBeNull());

    void result.current.useQwen();
    await act(async () => {
      await result.current.useApple();
      start.resolve({ id: 'late-qwen', port: 62001, pid: 100 });
      await Promise.resolve();
    });

    expect(selected).toHaveBeenCalledTimes(1);
    expect(selected).toHaveBeenCalledWith({
      providerId: 'apple-foundation',
      providerDisplayName: 'Apple On-Device',
      modelId: 'system',
    });
  });

  it('does not let a slow Apple action overwrite a later direct selection', async () => {
    const availability = deferred<AppleAvailability>();
    const { deps, selected } = setup({ getAppleAvailability: vi.fn().mockReturnValue(availability.promise) });
    const { result } = renderHook(() => useModelCatalog(deps));
    await waitFor(() => expect(result.current.entry(APPLE_ID)).not.toBeNull());

    void result.current.useApple();
    deps.selectedModel.select({
      providerId: 'ollama',
      providerDisplayName: 'Ollama',
      modelId: 'qwen2.5-coder',
    });
    await act(async () => {
      availability.resolve({ available: true, reason: null, detail: null });
      await Promise.resolve();
    });

    expect(selected).toHaveBeenCalledTimes(1);
    expect(selected).toHaveBeenCalledWith({
      providerId: 'ollama',
      providerDisplayName: 'Ollama',
      modelId: 'qwen2.5-coder',
    });
  });

  it('does not let a slow Qwen action overwrite a later direct selection', async () => {
    const start = deferred<ServerHandle | null>();
    const { deps, selected } = setup();
    deps.mlxServers.startCatalog = vi.fn().mockReturnValue(start.promise);
    const { result } = renderHook(() => useModelCatalog(deps));
    await waitFor(() => expect(result.current.entry(QWEN_CATALOG_ID)).not.toBeNull());

    void result.current.useQwen();
    deps.selectedModel.select({
      providerId: 'ollama',
      providerDisplayName: 'Ollama',
      modelId: 'qwen2.5-coder',
    });
    await act(async () => {
      start.resolve({ id: 'late-qwen', port: 62001, pid: 100 });
      await Promise.resolve();
    });

    expect(selected).toHaveBeenCalledTimes(1);
    expect(selected).toHaveBeenCalledWith({
      providerId: 'ollama',
      providerDisplayName: 'Ollama',
      modelId: 'qwen2.5-coder',
    });
  });

  it('coalesces repeated Qwen selection while its managed start is in flight', async () => {
    const start = deferred<ServerHandle | null>();
    const { deps, selected } = setup();
    deps.mlxServers.startCatalog = vi.fn().mockReturnValue(start.promise);
    const { result } = renderHook(() => useModelCatalog(deps));
    await waitFor(() => expect(result.current.entry(QWEN_CATALOG_ID)).not.toBeNull());

    const first = result.current.useQwen();
    const second = result.current.useQwen();
    expect(deps.mlxServers.startCatalog).toHaveBeenCalledTimes(1);
    await act(async () => {
      start.resolve({ id: 'shared-qwen', port: 62001, pid: 100 });
      await Promise.all([first, second]);
    });

    expect(selected).toHaveBeenCalledTimes(1);
    expect(selected).toHaveBeenCalledWith({
      providerId: 'mlx-lm',
      providerDisplayName: 'Qwen Coder',
      modelId: QWEN_CATALOG_ID,
    });
  });

  it('makes a repeated Qwen click newer than an intervening direct selection without a second start', async () => {
    const start = deferred<ServerHandle | null>();
    const { deps, selected } = setup();
    deps.mlxServers.startCatalog = vi.fn().mockReturnValue(start.promise);
    const { result } = renderHook(() => useModelCatalog(deps));
    await waitFor(() => expect(result.current.entry(QWEN_CATALOG_ID)).not.toBeNull());

    const first = result.current.useQwen();
    deps.selectedModel.select({
      providerId: 'ollama',
      providerDisplayName: 'Ollama',
      modelId: 'qwen2.5-coder',
    });
    const second = result.current.useQwen();
    expect(deps.mlxServers.startCatalog).toHaveBeenCalledTimes(1);
    await act(async () => {
      start.resolve({ id: 'shared-qwen', port: 62001, pid: 100 });
      await Promise.all([first, second]);
    });

    expect(selected).toHaveBeenCalledTimes(2);
    expect(selected).toHaveBeenLastCalledWith({
      providerId: 'mlx-lm',
      providerDisplayName: 'Qwen Coder',
      modelId: QWEN_CATALOG_ID,
    });
  });

  it('makes a repeated Qwen click newer than an intervening Apple intent without a second start', async () => {
    const start = deferred<ServerHandle | null>();
    const availability = deferred<AppleAvailability>();
    const { deps, selected } = setup({ getAppleAvailability: vi.fn().mockReturnValue(availability.promise) });
    deps.mlxServers.startCatalog = vi.fn().mockReturnValue(start.promise);
    const { result } = renderHook(() => useModelCatalog(deps));
    await waitFor(() => expect(result.current.entry(QWEN_CATALOG_ID)).not.toBeNull());

    const first = result.current.useQwen();
    void result.current.useApple();
    const second = result.current.useQwen();
    await act(async () => {
      start.resolve({ id: 'shared-qwen', port: 62001, pid: 100 });
      availability.resolve({ available: true, reason: null, detail: null });
      await Promise.all([first, second]);
    });

    expect(deps.mlxServers.startCatalog).toHaveBeenCalledTimes(1);
    expect(selected).toHaveBeenCalledTimes(1);
    expect(selected).toHaveBeenCalledWith({
      providerId: 'mlx-lm',
      providerDisplayName: 'Qwen Coder',
      modelId: QWEN_CATALOG_ID,
    });
  });

  it('does not let an older Apple availability response repaint a newer unavailable result', async () => {
    const first = deferred<AppleAvailability>();
    const { deps, selected } = setup({
      getAppleAvailability: vi.fn()
        .mockReturnValueOnce(first.promise)
        .mockResolvedValueOnce({
          available: false,
          reason: 'model-not-ready',
          detail: 'Still preparing.',
        } satisfies AppleAvailability),
    });
    const { result } = renderHook(() => useModelCatalog(deps));
    await waitFor(() => expect(result.current.entry(APPLE_ID)).not.toBeNull());

    void result.current.useApple();
    await act(async () => {
      await result.current.useApple();
      first.resolve({ available: true, reason: null, detail: null });
      await Promise.resolve();
    });

    expect(result.current.entry(APPLE_ID)).toMatchObject({
      state: 'unavailable',
      availabilityReason: 'Still preparing.',
      error: null,
    });
    expect(selected).not.toHaveBeenCalled();
  });

  it('does not let an older Apple error repaint a newer available result', async () => {
    const first = deferred<AppleAvailability>();
    const { deps, selected } = setup({
      getAppleAvailability: vi.fn()
        .mockReturnValueOnce(first.promise)
        .mockResolvedValueOnce({ available: true, reason: null, detail: null } satisfies AppleAvailability),
    });
    const { result } = renderHook(() => useModelCatalog(deps));
    await waitFor(() => expect(result.current.entry(APPLE_ID)).not.toBeNull());

    void result.current.useApple();
    await act(async () => {
      await result.current.useApple();
      first.reject(new Error('old helper failure'));
      await Promise.resolve();
    });

    expect(result.current.entry(APPLE_ID)).toMatchObject({
      state: 'available',
      error: null,
    });
    expect(selected).toHaveBeenCalledTimes(1);
  });

  it('waits for the catalog listener before starting a download', async () => {
    const listener = deferred<() => void>();
    const { deps } = setup({ subscribeCatalogDownloads: vi.fn().mockReturnValue(listener.promise) });
    const { result } = renderHook(() => useModelCatalog(deps));
    await waitFor(() => expect(result.current.entry(QWEN_CATALOG_ID)).not.toBeNull());

    const download = result.current.download(QWEN_CATALOG_ID);
    await Promise.resolve();
    expect(deps.downloadCatalogModel).not.toHaveBeenCalled();

    await act(async () => {
      listener.resolve(() => {});
      await download;
    });
    expect(deps.downloadCatalogModel).toHaveBeenCalledTimes(1);
  });

  it('keeps a listener failure visible after catalog listing succeeds', async () => {
    const listing = deferred<CatalogEntry[]>();
    const { deps } = setup({
      listCatalogModels: vi.fn().mockReturnValue(listing.promise),
      subscribeCatalogDownloads: vi.fn().mockRejectedValue(new Error('listener unavailable')),
    });
    const { result } = renderHook(() => useModelCatalog(deps));

    await waitFor(() => expect(result.current.error).toBe('listener unavailable'));
    await act(async () => {
      listing.resolve([apple(), qwen()]);
      await Promise.resolve();
    });

    expect(result.current.error).toBe('listener unavailable');
  });

  it('re-subscribes after a transient listener failure before starting a fast download', async () => {
    const listeners = new Set<(event: CatalogDownloadEvent) => void>();
    const subscribeCatalogDownloads = vi.fn()
      .mockRejectedValueOnce(new Error('listener unavailable'))
      .mockImplementation(async (listener: (event: CatalogDownloadEvent) => void) => {
        listeners.add(listener);
        return () => listeners.delete(listener);
      });
    const listCatalogModels = vi.fn()
      .mockResolvedValueOnce([apple(), qwen()])
      .mockResolvedValue([apple(), qwen('installed')]);
    const downloadCatalogModel = vi.fn(async () => {
      listeners.forEach((listener) => listener({
        operationId: 'fast-download',
        seq: 1,
        catalogId: QWEN_CATALOG_ID,
        phase: 'installed',
        downloadedBytes: 868_628_559,
        totalBytes: 868_628_559,
        error: null,
      }));
      return { operationId: 'fast-download' };
    });
    const { deps } = setup({ subscribeCatalogDownloads, listCatalogModels, downloadCatalogModel });
    const { result } = renderHook(() => useModelCatalog(deps));

    await waitFor(() => expect(result.current.error).toBe('listener unavailable'));
    await act(async () => {
      await result.current.refresh();
    });
    await waitFor(() => expect(subscribeCatalogDownloads).toHaveBeenCalledTimes(2));
    expect(listeners.size).toBe(1);

    await act(async () => {
      await result.current.download(QWEN_CATALOG_ID);
    });
    expect(downloadCatalogModel).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(result.current.entry(QWEN_CATALOG_ID)?.state).toBe('installed'));
  });

  it('coalesces concurrent listener recovery retries before starting one download', async () => {
    const recovery = deferred<() => void>();
    const subscribeCatalogDownloads = vi.fn()
      .mockRejectedValueOnce(new Error('listener unavailable'))
      .mockReturnValueOnce(recovery.promise);
    const { deps } = setup({ subscribeCatalogDownloads });
    const { result } = renderHook(() => useModelCatalog(deps));
    await waitFor(() => expect(result.current.error).toBe('listener unavailable'));

    const first = result.current.download(QWEN_CATALOG_ID);
    const second = result.current.download(QWEN_CATALOG_ID);
    await waitFor(() => expect(subscribeCatalogDownloads).toHaveBeenCalledTimes(2));
    await act(async () => {
      recovery.resolve(() => {});
      await Promise.all([first, second]);
    });

    expect(deps.downloadCatalogModel).toHaveBeenCalledTimes(1);
  });

  it('cleans a subscription that resolves after unmount', async () => {
    const listener = deferred<() => void>();
    const unlisten = vi.fn();
    const { deps } = setup({ subscribeCatalogDownloads: vi.fn().mockReturnValue(listener.promise) });
    const { unmount } = renderHook(() => useModelCatalog(deps));

    await waitFor(() => expect(deps.subscribeCatalogDownloads).toHaveBeenCalledTimes(1));
    unmount();
    await act(async () => {
      listener.resolve(unlisten);
      await Promise.resolve();
    });

    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it('projects receipt-listed Qwen as running from the MLX handle owner', async () => {
    const { deps, handle } = setup({ listCatalogModels: vi.fn().mockResolvedValue([apple(), qwen('installed')]) });
    deps.mlxServers.statusOf = vi.fn().mockReturnValue({ kind: 'running', handle });
    deps.mlxServers.handleOf = vi.fn().mockReturnValue(handle);
    const { result } = renderHook(() => useModelCatalog(deps));

    await waitFor(() => expect(result.current.entry(QWEN_CATALOG_ID)).toMatchObject({
      state: 'running',
    }));
    expect(result.current.entry(QWEN_CATALOG_ID)).not.toHaveProperty('handle');
    expect(deps.mlxServers.statusOf).toHaveBeenCalledWith(QWEN_CATALOG_ID);
  });

  it('projects managed startup and error states only for receipt-installed Qwen', async () => {
    const starting = setup({ listCatalogModels: vi.fn().mockResolvedValue([apple(), qwen('installed')]) });
    starting.deps.mlxServers.statusOf = vi.fn().mockReturnValue({ kind: 'starting' });
    const startingHook = renderHook(() => useModelCatalog(starting.deps));

    await waitFor(() => expect(startingHook.result.current.entry(QWEN_CATALOG_ID)?.state).toBe('starting'));

    const failed = setup({ listCatalogModels: vi.fn().mockResolvedValue([apple(), qwen('installed')]) });
    failed.deps.mlxServers.statusOf = vi.fn().mockReturnValue({ kind: 'error', message: 'runtime unavailable' });
    const failedHook = renderHook(() => useModelCatalog(failed.deps));
    await waitFor(() => expect(failedHook.result.current.entry(QWEN_CATALOG_ID)).toMatchObject({
      state: 'start-failed',
      error: 'runtime unavailable',
    }));

    const absent = setup({ listCatalogModels: vi.fn().mockResolvedValue([apple(), qwen('absent')]) });
    absent.deps.mlxServers.statusOf = vi.fn().mockReturnValue({ kind: 'error', message: 'stale error' });
    const absentHook = renderHook(() => useModelCatalog(absent.deps));
    await waitFor(() => expect(absentHook.result.current.entry(QWEN_CATALOG_ID)?.state).toBe('absent'));
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
