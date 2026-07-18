import { act, renderHook, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { QWEN_CATALOG_ID } from '../../lib/api/providers';
import { useModelCatalog } from './useModelCatalog';
import { apple, qwen, setup } from './useModelCatalog.test-support';

describe('useModelCatalog lifecycle recovery', () => {
  it('adopts a remounted Qwen download from its first nonterminal event and refreshes on terminal', async () => {
    const listCatalogModels = vi.fn()
      .mockResolvedValueOnce([apple(), qwen('absent')])
      .mockResolvedValueOnce([apple(), qwen('absent')])
      .mockResolvedValueOnce([apple(), qwen('installed')]);
    const { deps, emit } = setup({ listCatalogModels });
    const first = renderHook(() => useModelCatalog(deps));
    await waitFor(() => expect(first.result.current.entry(QWEN_CATALOG_ID)).not.toBeNull());
    first.unmount();

    const { result } = renderHook(() => useModelCatalog(deps));
    await waitFor(() => expect(result.current.entry(QWEN_CATALOG_ID)?.state).toBe('absent'));
    await act(async () => {
      emit({
        operationId: 'rust-survived-remount',
        seq: 4,
        catalogId: QWEN_CATALOG_ID,
        phase: 'downloading',
        downloadedBytes: 200,
        totalBytes: 300,
        error: null,
      });
    });
    expect(result.current.entry(QWEN_CATALOG_ID)).toMatchObject({
      state: 'downloading',
      operationId: 'rust-survived-remount',
      downloadedBytes: 200,
    });
    await act(async () => {
      await result.current.cancelDownload(QWEN_CATALOG_ID);
      emit({
        operationId: 'rust-survived-remount',
        seq: 5,
        catalogId: QWEN_CATALOG_ID,
        phase: 'installed',
        downloadedBytes: 300,
        totalBytes: 300,
        error: null,
      });
    });
    expect(deps.cancelCatalogDownload).toHaveBeenCalledWith('rust-survived-remount');
    await waitFor(() => expect(result.current.entry(QWEN_CATALOG_ID)?.state).toBe('installed'));
  });

  it('refreshes receipt state when a remounted hook first observes a terminal event', async () => {
    const listCatalogModels = vi.fn()
      .mockResolvedValueOnce([apple(), qwen('absent')])
      .mockResolvedValueOnce([apple(), qwen('installed')]);
    const { deps, emit } = setup({ listCatalogModels });
    const { result } = renderHook(() => useModelCatalog(deps));
    await waitFor(() => expect(result.current.entry(QWEN_CATALOG_ID)?.state).toBe('absent'));

    await act(async () => {
      emit({
        operationId: 'terminal-before-adoption',
        seq: 8,
        catalogId: QWEN_CATALOG_ID,
        phase: 'installed',
        downloadedBytes: 300,
        totalBytes: 300,
        error: null,
      });
    });

    await waitFor(() => expect(listCatalogModels).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(result.current.entry(QWEN_CATALOG_ID)?.state).toBe('installed'));
  });

  it('keeps an orphan failed terminal retryable when its receipt refresh is absent', async () => {
    const listCatalogModels = vi.fn().mockResolvedValue([apple(), qwen('absent')]);
    const { deps, emit } = setup({ listCatalogModels });
    const { result } = renderHook(() => useModelCatalog(deps));
    await waitFor(() => expect(result.current.entry(QWEN_CATALOG_ID)?.state).toBe('absent'));

    await act(async () => {
      emit({
        operationId: 'failed-before-adoption',
        seq: 9,
        catalogId: QWEN_CATALOG_ID,
        phase: 'failed',
        downloadedBytes: 120,
        totalBytes: 300,
        error: 'Network lost.',
      });
    });

    await waitFor(() => expect(listCatalogModels).toHaveBeenCalledTimes(2));
    expect(result.current.entry(QWEN_CATALOG_ID)).toMatchObject({
      state: 'failed',
      downloadedBytes: 120,
      totalBytes: 300,
      error: 'Network lost.',
    });
  });
});
