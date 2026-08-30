import { useCallback, useState } from 'react';

import { ipcErrorMessage, isIpcError } from '../../lib/api/errors';
import { sessionStorageUsage, type SessionScope } from '../../lib/api/sessions';

type StorageState = {
  atCap: boolean;
  writesRefused: boolean;
  error: string | null;
  warning: string | null;
};

const EMPTY_STORAGE: StorageState = {
  atCap: false,
  writesRefused: false,
  error: null,
  warning: null,
};

export function useSessionStorageStatus(activeScope: SessionScope) {
  // Usage and refused writes are independent: projected writes can be refused
  // while the current database remains below the cap.
  const [status, setStatus] = useState<Record<SessionScope, StorageState>>({
    local: EMPTY_STORAGE,
    project: EMPTY_STORAGE,
  });

  const refresh = useCallback(async (scope: SessionScope) => {
    try {
      const usage = await sessionStorageUsage({ scope });
      const atCap = usage.usedBytes >= usage.capBytes;
      const nearing = !atCap && usage.usedBytes >= usage.warnBytes;
      // A usage response cannot clear a projected-write refusal; only a
      // successful save proves that writes work again.
      setStatus((current) => ({
        ...current,
        [scope]: {
          ...current[scope],
          atCap,
          warning: nearing
            ? `This chat store is nearly full (${Math.round(usage.usedBytes / (1024 * 1024))} MB of ${Math.round(usage.capBytes / (1024 * 1024))} MB). Export and delete conversations you no longer need before new messages stop saving.`
            : null,
        },
      }));
    } catch (err) {
      const message = isIpcError(err) ? ipcErrorMessage(err) : String(err);
      console.error('sessions.storage failed:', message);
    }
  }, []);

  const setRefused = useCallback((scope: SessionScope, refused: boolean) => {
    setStatus((current) =>
      current[scope].writesRefused === refused
        ? current
        : { ...current, [scope]: { ...current[scope], writesRefused: refused } },
    );
  }, []);

  const setError = useCallback((scope: SessionScope, error: string | null) => {
    setStatus((current) =>
      current[scope].error === error
        ? current
        : { ...current, [scope]: { ...current[scope], error } },
    );
  }, []);

  return {
    full: status[activeScope].atCap || status[activeScope].writesRefused,
    error: status[activeScope].error,
    warning: status[activeScope].warning,
    refresh,
    setRefused,
    setError,
  };
}
