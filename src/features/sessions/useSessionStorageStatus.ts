import { useCallback, useState } from 'react';

import { ipcErrorMessage, isIpcError } from '../../lib/api/errors';
import { sessionStorageUsage, type SessionScope } from '../../lib/api/sessions';

export function useSessionStorageStatus() {
  // Usage and refused writes are independent: projected writes can be refused
  // while the current database remains below the cap.
  const [status, setStatus] = useState({
    atCap: false,
    writesRefused: false,
    warning: null as string | null,
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
        atCap,
        warning: nearing
          ? `This chat store is nearly full (${Math.round(usage.usedBytes / (1024 * 1024))} MB of ${Math.round(usage.capBytes / (1024 * 1024))} MB). Export and delete conversations you no longer need before new messages stop saving.`
          : null,
      }));
    } catch (err) {
      const message = isIpcError(err) ? ipcErrorMessage(err) : String(err);
      console.error('sessions.storage failed:', message);
    }
  }, []);

  const recordSaveSuccess = useCallback(() => {
    setStatus((current) =>
      current.writesRefused ? { ...current, writesRefused: false } : current,
    );
  }, []);

  const recordSaveFailure = useCallback((err: unknown) => {
    if (isIpcError(err) && err.kind === 'StorageFull') {
      setStatus((current) => ({ ...current, writesRefused: true }));
    }
  }, []);

  return {
    full: status.atCap || status.writesRefused,
    warning: status.warning,
    refresh,
    recordSaveSuccess,
    recordSaveFailure,
  };
}
