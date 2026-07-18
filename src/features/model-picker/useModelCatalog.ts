import { useCallback, useEffect, useRef, useState } from 'react';

import {
  QWEN_CATALOG_ID,
  cancelCatalogDownload as cancelCatalogDownloadIpc,
  downloadCatalogModel as downloadCatalogModelIpc,
  getAppleAvailability as getAppleAvailabilityIpc,
  listCatalogModels as listCatalogModelsIpc,
  removeCatalogModel as removeCatalogModelIpc,
  subscribeCatalogDownloads as subscribeCatalogDownloadsIpc,
  type AppleAvailability,
  type CatalogDownloadEvent,
  type CatalogEntry,
  type CatalogId,
} from '../../lib/api/providers';
import type { MlxServersApi } from '../providers/useMlxServers';
import type { SelectedModelApi } from './useSelectedModel';

export type ModelCatalogState = CatalogEntry['state'] | 'downloading' | 'verifying';

export type ModelCatalogEntry = Omit<CatalogEntry, 'state'> & {
  state: ModelCatalogState;
  operationId: string | null;
  downloadedBytes: number | null;
  totalBytes: number | null;
  error: string | null;
};

export type ModelCatalogDependencies = {
  listCatalogModels: () => Promise<CatalogEntry[]>;
  getAppleAvailability: () => Promise<AppleAvailability>;
  downloadCatalogModel: (catalogId: CatalogId) => Promise<{ operationId: string }>;
  cancelCatalogDownload: (operationId: string) => Promise<{ ok: boolean }>;
  removeCatalogModel: (catalogId: CatalogId) => Promise<{ removed: boolean }>;
  subscribeCatalogDownloads: (
    onEvent: (event: CatalogDownloadEvent) => void,
  ) => Promise<() => void>;
  mlxServers: MlxServersApi;
  selectedModel: SelectedModelApi;
};

export type ModelCatalogApi = {
  entries: ModelCatalogEntry[];
  entry: (catalogId: string) => ModelCatalogEntry | null;
  loading: boolean;
  error: string | null;
  download: (catalogId: CatalogId) => Promise<void>;
  cancelDownload: (catalogId: CatalogId) => Promise<void>;
  useApple: () => Promise<void>;
  useQwen: () => Promise<void>;
  removeQwen: () => Promise<void>;
  refresh: () => Promise<void>;
};

type DownloadFence = {
  operationId: string;
  generation: number;
  lastSeq: number;
};

const MAX_PENDING_START_OPERATIONS = 2;

function asModelCatalogEntry(entry: CatalogEntry): ModelCatalogEntry {
  return {
    ...entry,
    operationId: null,
    downloadedBytes: null,
    totalBytes: entry.downloadBytes,
    error: null,
  };
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * Owns the app-level catalog projection for one window. Selection and managed
 * MLX handles remain separate: the selection records only a catalog id while
 * `useMlxServers` remains the sole handle owner for chat dispatch.
 */
export function useModelCatalog(deps: ModelCatalogDependencies): ModelCatalogApi {
  const [entries, setEntries] = useState<ModelCatalogEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const listingGenerationRef = useRef(0);
  const listenerGenerationRef = useRef(0);
  const downloadGenerationRef = useRef(0);
  const activeDownloadRef = useRef<DownloadFence | null>(null);
  const awaitingDownloadStartRef = useRef(false);
  const pendingStartEventsRef = useRef(new Map<string, CatalogDownloadEvent>());

  const updateEntry = useCallback(
    (catalogId: string, update: (entry: ModelCatalogEntry) => ModelCatalogEntry) => {
      setEntries((current) => current.map((entry) => (entry.id === catalogId ? update(entry) : entry)));
    },
    [],
  );

  const refresh = useCallback(async () => {
    const generation = ++listingGenerationRef.current;
    setLoading(true);
    try {
      const listed = await deps.listCatalogModels();
      if (generation !== listingGenerationRef.current) return;
      setEntries(listed.map(asModelCatalogEntry));
      setError(null);
    } catch (nextError) {
      if (generation !== listingGenerationRef.current) return;
      setError(errorMessage(nextError));
    } finally {
      if (generation === listingGenerationRef.current) setLoading(false);
    }
  }, [deps.listCatalogModels]);

  const acceptDownloadEvent = useCallback(
    (event: CatalogDownloadEvent) => {
      const active = activeDownloadRef.current;
      // The event must belong to the currently elected operation and advance
      // its sequence. Old cancellation/retry events never repaint this window.
      if (
        active === null ||
        event.catalogId !== QWEN_CATALOG_ID ||
        event.operationId !== active.operationId ||
        active.generation !== downloadGenerationRef.current ||
        event.seq <= active.lastSeq
      ) {
        return;
      }
      active.lastSeq = event.seq;
      const state: ModelCatalogState =
        event.phase === 'downloading' || event.phase === 'started'
          ? 'downloading'
          : event.phase === 'verifying'
            ? 'verifying'
            : event.phase === 'installed'
              ? 'installed'
              : event.phase === 'failed'
                ? 'failed'
                : 'absent';
      updateEntry(event.catalogId, (entry) => ({
        ...entry,
        state,
        operationId: event.phase === 'installed' || event.phase === 'failed' || event.phase === 'cancelled'
          ? null
          : active.operationId,
        downloadedBytes: event.downloadedBytes,
        totalBytes: event.totalBytes,
        error: event.error,
      }));
      if (event.phase === 'installed' || event.phase === 'failed' || event.phase === 'cancelled') {
        activeDownloadRef.current = null;
        void refresh();
      }
    },
    [refresh, updateEntry],
  );

  const bufferPendingStartEvent = useCallback((event: CatalogDownloadEvent) => {
    if (!awaitingDownloadStartRef.current || event.catalogId !== QWEN_CATALOG_ID) return;
    const previous = pendingStartEventsRef.current.get(event.operationId);
    if (previous !== undefined && previous.seq >= event.seq) return;
    if (
      previous === undefined &&
      pendingStartEventsRef.current.size >= MAX_PENDING_START_OPERATIONS
    ) {
      const oldestOperationId = pendingStartEventsRef.current.keys().next().value;
      if (oldestOperationId !== undefined) pendingStartEventsRef.current.delete(oldestOperationId);
    }
    // Keep only the newest event per operation. The returned operation id
    // elects one entry after IPC resolves, so older progress is unnecessary.
    pendingStartEventsRef.current.set(event.operationId, event);
  }, []);

  useEffect(() => {
    const generation = ++listenerGenerationRef.current;
    let disposed = false;
    let unlisten: (() => void) | null = null;
    void refresh();
    void deps.subscribeCatalogDownloads((event) => {
      if (disposed || generation !== listenerGenerationRef.current) return;
      if (activeDownloadRef.current === null && awaitingDownloadStartRef.current) {
        bufferPendingStartEvent(event);
        return;
      }
      acceptDownloadEvent(event);
    }).then((nextUnlisten) => {
      if (disposed || generation !== listenerGenerationRef.current) {
        nextUnlisten();
        return;
      }
      unlisten = nextUnlisten;
    }).catch((subscribeError: unknown) => {
      if (disposed || generation !== listenerGenerationRef.current) return;
      setError(errorMessage(subscribeError));
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [acceptDownloadEvent, bufferPendingStartEvent, deps.subscribeCatalogDownloads, refresh]);

  const download = useCallback(
    async (catalogId: CatalogId) => {
      if (catalogId !== QWEN_CATALOG_ID) return;
      awaitingDownloadStartRef.current = true;
      pendingStartEventsRef.current.clear();
      try {
        const started = await deps.downloadCatalogModel(catalogId);
        awaitingDownloadStartRef.current = false;
        // Invalidate any older list response before presenting the new local
        // intent. A retry cannot be overwritten by a pre-retry terminal list.
        ++listingGenerationRef.current;
        activeDownloadRef.current = {
          operationId: started.operationId,
          generation: ++downloadGenerationRef.current,
          lastSeq: -1,
        };
        updateEntry(catalogId, (entry) => ({
          ...entry,
          state: 'downloading',
          operationId: started.operationId,
          downloadedBytes: 0,
          totalBytes: entry.downloadBytes,
          error: null,
        }));
        setError(null);
        const pending = pendingStartEventsRef.current.get(started.operationId);
        pendingStartEventsRef.current.clear();
        if (pending !== undefined) acceptDownloadEvent(pending);
      } catch (downloadError) {
        awaitingDownloadStartRef.current = false;
        pendingStartEventsRef.current.clear();
        updateEntry(catalogId, (entry) => ({
          ...entry,
          state: 'failed',
          operationId: null,
          error: errorMessage(downloadError),
        }));
      }
    },
    [acceptDownloadEvent, deps.downloadCatalogModel, updateEntry],
  );

  const cancelDownload = useCallback(
    async (catalogId: CatalogId) => {
      const active = activeDownloadRef.current;
      if (catalogId !== QWEN_CATALOG_ID || active === null) return;
      await deps.cancelCatalogDownload(active.operationId);
      // Drop the fence before refresh so a terminal event from the cancelled
      // operation cannot repaint a retry that starts while the list is in flight.
      activeDownloadRef.current = null;
      ++downloadGenerationRef.current;
      await refresh();
    },
    [deps.cancelCatalogDownload, refresh],
  );

  const useApple = useCallback(async () => {
    try {
      const availability = await deps.getAppleAvailability();
      updateEntry('apple-system', (entry) => ({
        ...entry,
        state: availability.available ? 'available' : 'unavailable',
        availabilityReason: availability.available ? null : availability.detail ?? entry.availabilityReason,
        error: null,
      }));
      if (!availability.available) return;
      deps.selectedModel.select({
        providerId: 'apple-foundation',
        providerDisplayName: 'Apple On-Device',
        modelId: 'system',
      });
    } catch (availabilityError) {
      updateEntry('apple-system', (entry) => ({
        ...entry,
        state: 'unavailable',
        availabilityReason: 'Could not check Apple model availability.',
        error: errorMessage(availabilityError),
      }));
    }
  }, [deps.getAppleAvailability, deps.selectedModel, updateEntry]);

  const useQwen = useCallback(async () => {
    try {
      const handle = await deps.mlxServers.startCatalog(QWEN_CATALOG_ID);
      if (handle === null) {
        updateEntry(QWEN_CATALOG_ID, (entry) => ({
          ...entry,
          state: entry.state === 'running' ? 'installed' : entry.state,
          error: 'Could not start Qwen. Try again.',
        }));
        return;
      }
      updateEntry(QWEN_CATALOG_ID, (entry) => ({ ...entry, state: 'running', error: null }));
      deps.selectedModel.select({
        providerId: 'mlx-lm',
        providerDisplayName: 'Qwen Coder',
        modelId: QWEN_CATALOG_ID,
      });
    } catch (startError) {
      updateEntry(QWEN_CATALOG_ID, (entry) => ({
        ...entry,
        state: entry.state === 'running' ? 'installed' : entry.state,
        error: errorMessage(startError),
      }));
    }
  }, [deps.mlxServers, deps.selectedModel, updateEntry]);

  const removeQwen = useCallback(async () => {
    if (deps.mlxServers.statusOf(QWEN_CATALOG_ID).kind === 'running') return;
    await deps.removeCatalogModel(QWEN_CATALOG_ID);
    await refresh();
  }, [deps.mlxServers, deps.removeCatalogModel, refresh]);

  const entry = useCallback(
    (catalogId: string) => entries.find((candidate) => candidate.id === catalogId) ?? null,
    [entries],
  );

  return {
    entries,
    entry,
    loading,
    error,
    download,
    cancelDownload,
    useApple,
    useQwen,
    removeQwen,
    refresh,
  };
}

export const defaultModelCatalogDependencies = {
  listCatalogModels: listCatalogModelsIpc,
  getAppleAvailability: getAppleAvailabilityIpc,
  downloadCatalogModel: downloadCatalogModelIpc,
  cancelCatalogDownload: cancelCatalogDownloadIpc,
  removeCatalogModel: removeCatalogModelIpc,
  subscribeCatalogDownloads: subscribeCatalogDownloadsIpc,
};
