import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

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
  downloadEventsReady: boolean;
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
  cancelling: boolean;
};

type DownloadFailure = {
  generation: number;
  error: string;
  downloadedBytes: number;
  totalBytes: number;
};

type QwenSelectionAttempt = {
  intent: number;
  selectionRevision: number;
  selected: boolean;
  start: ReturnType<MlxServersApi['startCatalog']>;
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

function projectServerState(entry: ModelCatalogEntry, mlxServers: MlxServersApi): ModelCatalogEntry {
  if (entry.id !== QWEN_CATALOG_ID) return entry;
  const status = mlxServers.statusOf(QWEN_CATALOG_ID);
  if (status.kind === 'running') return { ...entry, state: 'running', error: null };
  // Catalog listing is receipt-backed and therefore reports an installed Qwen
  // after its managed server stops. Do not leave an old local `running` paint.
  return entry.state === 'running' ? { ...entry, state: 'installed' } : entry;
}

/**
 * Owns the app-level catalog projection for one window. Selection and managed
 * MLX handles remain separate: the selection records only a catalog id while
 * `useMlxServers` remains the sole handle owner for chat dispatch.
 */
export function useModelCatalog(deps: ModelCatalogDependencies): ModelCatalogApi {
  const [entries, setEntries] = useState<ModelCatalogEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [listingError, setListingError] = useState<string | null>(null);
  const [subscriptionError, setSubscriptionError] = useState<string | null>(null);
  const [downloadEventsReady, setDownloadEventsReady] = useState(false);
  const listingGenerationRef = useRef(0);
  const listenerGenerationRef = useRef(0);
  const downloadGenerationRef = useRef(0);
  const activeDownloadRef = useRef<DownloadFence | null>(null);
  const terminalFailureRef = useRef<DownloadFailure | null>(null);
  const awaitingDownloadStartRef = useRef(false);
  const pendingStartEventsRef = useRef(new Map<string, CatalogDownloadEvent>());
  const downloadEventsReadyRef = useRef(false);
  const listenerMountedRef = useRef(false);
  const listenerUnlistenRef = useRef<(() => void) | null>(null);
  const listenerAttemptRef = useRef<Promise<boolean> | null>(null);
  const selectionIntentRef = useRef(0);
  const qwenSelectionAttemptRef = useRef<QwenSelectionAttempt | null>(null);

  const updateEntry = useCallback(
    (catalogId: string, update: (entry: ModelCatalogEntry) => ModelCatalogEntry) => {
      setEntries((current) => current.map((entry) => (entry.id === catalogId ? update(entry) : entry)));
    },
    [],
  );

  const refreshListing = useCallback(async () => {
    const generation = ++listingGenerationRef.current;
    setLoading(true);
    try {
      const listed = await deps.listCatalogModels();
      if (generation !== listingGenerationRef.current) return;
      const failure = terminalFailureRef.current;
      setEntries(listed.map((entry) => {
        const projected = asModelCatalogEntry(entry);
        if (
          entry.id !== QWEN_CATALOG_ID ||
          failure === null ||
          failure.generation !== downloadGenerationRef.current
        ) {
          return projected;
        }
        // The catalog receipt intentionally reports failed/cancelled downloads
        // as absent. Preserve the most recent terminal failure as an honest
        // retryable frontend state until a newer Qwen action supersedes it.
        return {
          ...projected,
          state: 'failed',
          downloadedBytes: failure.downloadedBytes,
          totalBytes: failure.totalBytes,
          error: failure.error,
        };
      }));
      setListingError(null);
    } catch (nextError) {
      if (generation !== listingGenerationRef.current) return;
      setListingError(errorMessage(nextError));
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
        if (event.phase === 'failed') {
          terminalFailureRef.current = {
            generation: active.generation,
            error: event.error ?? 'Download failed. Try again.',
            downloadedBytes: event.downloadedBytes,
            totalBytes: event.totalBytes,
          };
        } else {
          terminalFailureRef.current = null;
        }
        activeDownloadRef.current = null;
        void refreshListing();
      }
    },
    [refreshListing, updateEntry],
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

  const ensureDownloadEvents = useCallback((): Promise<boolean> => {
    if (downloadEventsReadyRef.current) return Promise.resolve(true);
    if (listenerAttemptRef.current !== null) return listenerAttemptRef.current;
    if (!listenerMountedRef.current) return Promise.resolve(false);

    const generation = ++listenerGenerationRef.current;
    downloadEventsReadyRef.current = false;
    setDownloadEventsReady(false);
    setSubscriptionError(null);
    let attempt: Promise<boolean>;
    attempt = deps.subscribeCatalogDownloads((event) => {
      if (!listenerMountedRef.current || generation !== listenerGenerationRef.current) return;
      if (activeDownloadRef.current === null && awaitingDownloadStartRef.current) {
        bufferPendingStartEvent(event);
        return;
      }
      acceptDownloadEvent(event);
    }).then((nextUnlisten) => {
      if (!listenerMountedRef.current || generation !== listenerGenerationRef.current) {
        nextUnlisten();
        return false;
      }
      listenerUnlistenRef.current = nextUnlisten;
      downloadEventsReadyRef.current = true;
      setDownloadEventsReady(true);
      return true;
    }).catch((subscribeError: unknown) => {
      if (listenerMountedRef.current && generation === listenerGenerationRef.current) {
        downloadEventsReadyRef.current = false;
        setDownloadEventsReady(false);
        setSubscriptionError(errorMessage(subscribeError));
      }
      return false;
    }).finally(() => {
      if (listenerAttemptRef.current === attempt) listenerAttemptRef.current = null;
    });
    listenerAttemptRef.current = attempt;
    return attempt;
  }, [acceptDownloadEvent, bufferPendingStartEvent, deps.subscribeCatalogDownloads]);

  useEffect(() => {
    listenerMountedRef.current = true;
    void refreshListing();
    void ensureDownloadEvents();
    return () => {
      listenerMountedRef.current = false;
      ++listenerGenerationRef.current;
      downloadEventsReadyRef.current = false;
      listenerAttemptRef.current = null;
      listenerUnlistenRef.current?.();
      listenerUnlistenRef.current = null;
    };
  }, [ensureDownloadEvents, refreshListing]);

  const refresh = useCallback(async () => {
    await Promise.all([refreshListing(), ensureDownloadEvents()]);
  }, [ensureDownloadEvents, refreshListing]);

  const download = useCallback(
    async (catalogId: CatalogId) => {
      if (catalogId !== QWEN_CATALOG_ID) return;
      if (!downloadEventsReadyRef.current) {
        const ready = await ensureDownloadEvents();
        if (ready) {
          // A concurrent retry may have won while this caller was restoring
          // the subscription. Re-check its operation fence before starting.
          if (activeDownloadRef.current !== null || awaitingDownloadStartRef.current) return;
        } else {
          updateEntry(catalogId, (entry) => ({
            ...entry,
            state: 'failed',
            operationId: null,
            error: 'Model download updates are not ready. Try again.',
          }));
          return;
        }
      }
      if (activeDownloadRef.current !== null || awaitingDownloadStartRef.current) {
        return;
      }
      if (!downloadEventsReadyRef.current) {
        updateEntry(catalogId, (entry) => ({
          ...entry,
          state: 'failed',
          operationId: null,
          error: 'Model download updates are not ready. Try again.',
        }));
        return;
      }
      const generation = ++downloadGenerationRef.current;
      terminalFailureRef.current = null;
      // A new explicit retry is newer than every outstanding receipt listing.
      // Invalidate those reads before the download IPC can await its response.
      ++listingGenerationRef.current;
      awaitingDownloadStartRef.current = true;
      pendingStartEventsRef.current.clear();
      updateEntry(catalogId, (entry) => ({
        ...entry,
        state: 'downloading',
        operationId: null,
        downloadedBytes: 0,
        totalBytes: entry.downloadBytes,
        error: null,
      }));
      try {
        const started = await deps.downloadCatalogModel(catalogId);
        awaitingDownloadStartRef.current = false;
        activeDownloadRef.current = {
          operationId: started.operationId,
          generation,
          lastSeq: -1,
          cancelling: false,
        };
        updateEntry(catalogId, (entry) => ({
          ...entry,
          state: 'downloading',
          operationId: started.operationId,
          downloadedBytes: 0,
          totalBytes: entry.downloadBytes,
          error: null,
        }));
        setListingError(null);
        const pending = pendingStartEventsRef.current.get(started.operationId);
        pendingStartEventsRef.current.clear();
        if (pending !== undefined) acceptDownloadEvent(pending);
      } catch (downloadError) {
        awaitingDownloadStartRef.current = false;
        pendingStartEventsRef.current.clear();
        terminalFailureRef.current = {
          generation,
          error: errorMessage(downloadError),
          downloadedBytes: 0,
          totalBytes: 0,
        };
        updateEntry(catalogId, (entry) => ({
          ...entry,
          state: 'failed',
          operationId: null,
          error: errorMessage(downloadError),
        }));
      }
    },
    [acceptDownloadEvent, deps.downloadCatalogModel, ensureDownloadEvents, updateEntry],
  );

  const cancelDownload = useCallback(
    async (catalogId: CatalogId) => {
      const active = activeDownloadRef.current;
      if (catalogId !== QWEN_CATALOG_ID || active === null || active.cancelling) return;
      active.cancelling = true;
      try {
        await deps.cancelCatalogDownload(active.operationId);
      } catch {
        if (activeDownloadRef.current !== active) return;
        active.cancelling = false;
        updateEntry(catalogId, (entry) => ({
          ...entry,
          error: 'Could not cancel download. Try again.',
        }));
        return;
      }
      // Cancellation ACK means only that Rust observed the request. The worker
      // still owns its registry slot until it emits the matching terminal
      // Cancelled event, so keep this fence to block an unsafe immediate retry.
    },
    [deps.cancelCatalogDownload, updateEntry],
  );

  const useApple = useCallback(async () => {
    const intent = ++selectionIntentRef.current;
    const selectionRevision = deps.selectedModel.revision();
    try {
      const availability = await deps.getAppleAvailability();
      updateEntry('apple-system', (entry) => ({
        ...entry,
        state: availability.available ? 'available' : 'unavailable',
        availabilityReason: availability.available ? null : availability.detail ?? entry.availabilityReason,
        error: null,
      }));
      if (
        !availability.available ||
        intent !== selectionIntentRef.current ||
        selectionRevision !== deps.selectedModel.revision()
      ) return;
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
    let attempt = qwenSelectionAttemptRef.current;
    if (attempt === null) {
      attempt = {
        intent: ++selectionIntentRef.current,
        selectionRevision: deps.selectedModel.revision(),
        selected: false,
        start: deps.mlxServers.startCatalog(QWEN_CATALOG_ID),
      };
      qwenSelectionAttemptRef.current = attempt;
    }
    try {
      const handle = await attempt.start;
      if (handle === null) {
        updateEntry(QWEN_CATALOG_ID, (entry) => ({
          ...entry,
          state: entry.state === 'running' ? 'installed' : entry.state,
          error: 'Could not start Qwen. Try again.',
        }));
        return;
      }
      updateEntry(QWEN_CATALOG_ID, (entry) => ({ ...entry, state: 'running', error: null }));
      if (
        attempt.selected ||
        attempt.intent !== selectionIntentRef.current ||
        attempt.selectionRevision !== deps.selectedModel.revision()
      ) return;
      attempt.selected = true;
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
    } finally {
      if (qwenSelectionAttemptRef.current === attempt) qwenSelectionAttemptRef.current = null;
    }
  }, [deps.mlxServers, deps.selectedModel, updateEntry]);

  const removeQwen = useCallback(async () => {
    if (deps.mlxServers.statusOf(QWEN_CATALOG_ID).kind === 'running') return;
    await deps.removeCatalogModel(QWEN_CATALOG_ID);
    await refresh();
  }, [deps.mlxServers, deps.removeCatalogModel, refresh]);

  const projectedEntries = useMemo(
    () => entries.map((entry) => projectServerState(entry, deps.mlxServers)),
    [deps.mlxServers, entries],
  );
  const entry = useCallback(
    (catalogId: string) => projectedEntries.find((candidate) => candidate.id === catalogId) ?? null,
    [projectedEntries],
  );

  return {
    entries: projectedEntries,
    entry,
    loading,
    downloadEventsReady,
    error: subscriptionError ?? listingError,
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
