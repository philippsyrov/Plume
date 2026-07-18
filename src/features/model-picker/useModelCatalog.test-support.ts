import { vi } from 'vitest';

import {
  QWEN_CATALOG_ID,
  type AppleAvailability,
  type CatalogDownloadEvent,
  type CatalogEntry,
  type ServerHandle,
} from '../../lib/api/providers';
import type { ModelCatalogDependencies } from './useModelCatalog';

export const APPLE_ID = 'apple-system';

export const apple = (state: CatalogEntry['state'] = 'available'): CatalogEntry => ({
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

export const qwen = (state: CatalogEntry['state'] = 'absent'): CatalogEntry => ({
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

export function deferred<T>() {
  let resolve: (value: T) => void = () => {};
  let reject: (reason?: unknown) => void = () => {};
  const promise = new Promise<T>((next, fail) => {
    resolve = next;
    reject = fail;
  });
  return { promise, resolve, reject };
}

export function setup(overrides: Partial<ModelCatalogDependencies> = {}) {
  const listeners = new Set<(event: CatalogDownloadEvent) => void>();
  let selectionRevision = 0;
  const selected = vi.fn(() => {
    selectionRevision += 1;
  });
  const clearSelection = vi.fn(() => {
    selectionRevision += 1;
  });
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
    selectedModel: {
      selected: null,
      select: selected,
      clear: clearSelection,
      revision: () => selectionRevision,
    },
    ...overrides,
  };
  return {
    deps,
    selected,
    clearSelection,
    handle,
    activeListenerCount: () => listeners.size,
    emit: (event: CatalogDownloadEvent) => listeners.forEach((listener) => listener(event)),
  };
}
