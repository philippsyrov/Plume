// D46: per-model MLX server lifecycle state.
//
// Owns the bookkeeping for Plume-managed MLX-LM servers the user
// starts and stops from the Local models panel. The hook does NOT
// drive the chat dispatch — it only tracks which model has a live
// `ServerHandle` and lets callers look it up by `modelId`. Chat
// routing in `useChat.send` reads the matching handle's `id`
// (passed in via `SendOptions.handleId`) and threads it through to
// `chat.send`, which the D45 backend dispatches onto the MLX SSE
// adapter.
//
// Lifecycle states per model id:
//
//   * `idle`     — no server running; "Start" button is enabled.
//   * `starting` — `providers.startServer` is in flight; both
//                  buttons are disabled, the row shows a status hint.
//   * `running`  — handle is live; "Stop" button is enabled and the
//                  row surfaces the bound port for diagnostics.
//   * `stopping` — `providers.stopServer` is in flight; same hint
//                  as `starting`.
//   * `error`    — the last start/stop attempt failed; the row shows
//                  the error inline and offers Start again. The
//                  next successful start clears the error.
//
// State is per-`modelId` because the supervisor allows concurrent
// servers — a user could in principle start two different MLX folders
// at once. The hook doesn't enforce a "one server at a time" rule;
// that's a UX choice on top.
//
// IPC contract (D40):
//   * `providers.startServer({ providerId: 'mlx-lm', modelId })` →
//     `ServerHandle { id, port, pid }` on success.
//   * `providers.stopServer({ handleId })` → `{ ok: true }`. Stopping
//     an unknown handle rejects with `NotFound` — we treat that as
//     a graceful no-op so the UI doesn't get stuck in `stopping`.
//
// Trust gate (D40): `providers.startServer` requires a trusted open
// project (spawning a Python subprocess sits behind the same gate
// as `memory.remember` / `patch.apply`). A `NeedsApproval` rejection
// surfaces here as `error` state with a "trust the project to start
// the server" hint; clearing the gate and clicking Start again
// works.

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import { isIpcError, ipcErrorMessage } from '../../lib/api/errors';
import {
  listServers,
  startServer,
  stopServer,
  type ServerHandle,
} from '../../lib/api/providers';

/** Provider id passed to `providers.startServer` for MLX servers. */
export const MLX_LM_PROVIDER_ID = 'mlx-lm';

export type MlxServerStatus =
  | { kind: 'idle' }
  | { kind: 'starting' }
  | { kind: 'running'; handle: ServerHandle }
  | { kind: 'stopping' }
  | { kind: 'error'; message: string };

export type MlxServersApi = {
  /** Snapshot of every modelId → status. Stable reference per render. */
  statuses: ReadonlyMap<string, MlxServerStatus>;
  /** Cheap accessor; returns `'idle'` for an unknown id. */
  statusOf: (modelId: string) => MlxServerStatus;
  /** Returns the live ServerHandle for the modelId or `null`. */
  handleOf: (modelId: string) => ServerHandle | null;
  /**
   * Start a server. Resolves once the supervisor reports `/health`
   * OK or the start failed; the status transitions are visible via
   * `statuses` / `statusOf` while the call is in flight. Returns
   * the handle on success so the caller can use it for the
   * follow-on "select this model" call without going through state
   * again.
   */
  start: (modelId: string) => Promise<ServerHandle | null>;
  /**
   * Stop a running server. Resolves once the supervisor reports
   * exit. Idempotent for unknown / already-stopped handles — the
   * IPC `NotFound` rejection collapses to `idle` state.
   */
  stop: (modelId: string) => Promise<void>;
  /**
   * Clear an `error` status without trying to start/stop. Used by
   * tests and by future "dismiss error" UI; the panel currently
   * just shows the error until the next Start click.
   */
  clearError: (modelId: string) => void;
};

export function useMlxServers(): MlxServersApi {
  const [statuses, setStatuses] = useState<ReadonlyMap<string, MlxServerStatus>>(
    () => new Map(),
  );

  // Keep a ref of the latest map so concurrent IPC handlers can
  // read the in-flight state without retriggering renders. React
  // setState inside an async function can lose intermediate
  // updates if we read from a stale closure capture; the ref keeps
  // truth. `setStatus` writes the ref SYNCHRONOUSLY (before the
  // batched React state update flushes) so an async continuation
  // that just awaited an IPC promise — recovery adoption, a
  // resolving start/stop — reads its own write instead of a
  // one-render-stale snapshot (Codex #154 P2).
  const statusesRef = useRef(statuses);

  // D46 Codex MEDIUM fix: track whether the host component has
  // unmounted (project close, window destroy, etc.) so two things
  // happen correctly:
  //
  //   1) The unmount-cleanup effect fires `providers.stopServer`
  //      for every `running` handle, so the supervised Python
  //      children don't outlive the UI that started them.
  //   2) A `start()` that resolves AFTER unmount — common when
  //      mlx-lm spends 10–15 s loading weights and the user closes
  //      the project mid-load — immediately stops the freshly
  //      returned handle. Without this race guard the handle id
  //      goes nowhere and the child runs to completion as an
  //      orphan.
  //
  // The ref is the source of truth; React setState is skipped
  // entirely once `unmountedRef.current === true` because the
  // host component is gone.
  const unmountedRef = useRef(false);

  const setStatus = useCallback((modelId: string, status: MlxServerStatus) => {
    if (unmountedRef.current) return;
    // Build from the ref (always current) rather than a functional
    // updater: the ref must be readable synchronously by async
    // continuations, and an updater's `prev` only exists once React
    // flushes. Sequential calls each build on the previous write, so
    // batching loses nothing.
    const next = new Map(statusesRef.current);
    if (status.kind === 'idle') {
      next.delete(modelId);
    } else {
      next.set(modelId, status);
    }
    statusesRef.current = next;
    setStatuses(next);
  }, []);

  useEffect(() => {
    return () => {
      // Flip the ref BEFORE firing stops so any in-flight
      // setStatus from a resolving start/stop skips state updates
      // on a dead component.
      unmountedRef.current = true;
      // Fire-and-forget stop for every live handle. We don't
      // await — the component is going away regardless, and
      // `stopServer` only matters on the Rust side (the
      // supervisor's registry, the child PID). A failed stop
      // (NotFound, transient IO) leaves the child running, which
      // is no worse than the pre-fix behaviour.
      const snapshot = statusesRef.current;
      for (const status of snapshot.values()) {
        if (status.kind === 'running') {
          stopServer({ handleId: status.handle.id }).catch((err: unknown) => {
            // Best-effort logging. The component is gone, so we
            // can't surface this in the UI.
            console.error(
              'useMlxServers: unmount stop failed for handle %s: %s',
              status.handle.id,
              err instanceof Error ? err.message : String(err),
            );
          });
        }
      }
    };
    // Run-once on mount; the cleanup fires on unmount. Re-running
    // would tear down still-running servers on a hot-reload, which
    // we explicitly don't want during dev iteration.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Thermos I1: adopt servers the Rust supervisor still manages.
  // A webview reload / remount skips the unmount stops above, so
  // without this a running child's handle is lost to the UI and
  // the server can no longer be stopped from Plume. The registry
  // only ever holds children THIS process started, so adoption
  // cannot claim foreign processes. Servers without a recorded
  // inventory id are skipped — the panel keys its rows by modelId
  // and an unkeyable row would be unrenderable; those remain
  // reachable via the Rust exit sweep.
  //
  // Codex #154 P2: `start`/`stop` await this promise before reading
  // state, so a click landing in the short window while the listing
  // is still in flight cannot race adoption — without that ordering
  // a Start would flip the model to `starting`, adoption would skip
  // the recovered handle, and the original running child would be
  // stranded again. The promise ALWAYS resolves (failure is an
  // honest skip), so it can never wedge the buttons.
  const recoveryRef = useRef<Promise<void> | null>(null);

  useEffect(() => {
    recoveryRef.current = listServers()
      .then((response) => {
        if (unmountedRef.current) return;
        for (const server of response.servers) {
          if (!server.modelId) continue;
          if (statusesRef.current.get(server.modelId)) continue;
          setStatus(server.modelId, {
            kind: 'running',
            handle: { id: server.handleId, port: server.port, pid: server.pid },
          });
        }
      })
      .catch((err: unknown) => {
        // Honest skip: recovery is best-effort and the panel keeps
        // working for fresh starts. The exit sweep still covers the
        // unadopted children on quit.
        console.error(
          'useMlxServers: recovering managed servers failed:',
          err instanceof Error ? err.message : String(err),
        );
      })
      .finally(() => {
        recoveryRef.current = null;
      });
    // Run-once on mount, same lifetime posture as the unmount
    // cleanup effect above.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const statusOf = useCallback(
    (modelId: string): MlxServerStatus =>
      statusesRef.current.get(modelId) ?? { kind: 'idle' },
    [],
  );

  const handleOf = useCallback((modelId: string): ServerHandle | null => {
    const status = statusesRef.current.get(modelId);
    return status?.kind === 'running' ? status.handle : null;
  }, []);

  const start = useCallback(
    async (modelId: string): Promise<ServerHandle | null> => {
      // Codex #154 P2: let in-flight recovery land first so a click
      // during the mount window sees an adopted `running` status
      // (and returns its handle below) instead of spawning a second
      // server for the same model and stranding the first.
      if (recoveryRef.current) await recoveryRef.current;
      // Re-entry guard. A double-click on Start shouldn't fire two
      // spawns; only `idle` / `error` are valid entry points.
      const current = statusesRef.current.get(modelId)?.kind ?? 'idle';
      if (current === 'starting' || current === 'stopping' || current === 'running') {
        return current === 'running'
          ? (statusesRef.current.get(modelId) as Extract<
              MlxServerStatus,
              { kind: 'running' }
            >).handle
          : null;
      }
      setStatus(modelId, { kind: 'starting' });
      try {
        const handle = await startServer({
          providerId: MLX_LM_PROVIDER_ID,
          modelId,
        });
        // D46 Codex MEDIUM fix: if the host component unmounted
        // while `providers.startServer` was loading weights (10–15s
        // common, longer for big models), the resolved handle would
        // otherwise leak — React state updates are no-ops on a dead
        // component, and the supervisor's child would run to
        // completion as an orphan. Detect the race here and fire a
        // matching `stopServer` BEFORE returning.
        if (unmountedRef.current) {
          stopServer({ handleId: handle.id }).catch((err: unknown) => {
            console.error(
              'useMlxServers: race-stop after unmount failed for handle %s: %s',
              handle.id,
              err instanceof Error ? err.message : String(err),
            );
          });
          return null;
        }
        setStatus(modelId, { kind: 'running', handle });
        return handle;
      } catch (err: unknown) {
        const message = friendlyStartError(err);
        setStatus(modelId, { kind: 'error', message });
        return null;
      }
    },
    [setStatus],
  );

  const stop = useCallback(
    async (modelId: string): Promise<void> => {
      // Same recovery ordering as `start`: an early Stop click must
      // see the adopted handle, not a stale `idle` no-op.
      if (recoveryRef.current) await recoveryRef.current;
      const current = statusesRef.current.get(modelId);
      if (!current || current.kind === 'idle') return;
      if (current.kind !== 'running') {
        // `starting` / `stopping` / `error`: nothing live to kill.
        // For `error` we just clear; for `starting` the in-flight
        // start will reach its terminal state and overwrite ours.
        // For `stopping` it's already in flight.
        if (current.kind === 'error') setStatus(modelId, { kind: 'idle' });
        return;
      }
      const handleId = current.handle.id;
      setStatus(modelId, { kind: 'stopping' });
      try {
        await stopServer({ handleId });
        setStatus(modelId, { kind: 'idle' });
      } catch (err: unknown) {
        // `NotFound` on the backend means the supervisor doesn't
        // recognize this handle — either it was already stopped
        // or it belongs to a different Plume instance. Either way
        // the UI's bookkeeping is stale; collapse to idle so the
        // user can start a fresh server.
        if (isIpcError(err) && err.kind === 'NotFound') {
          setStatus(modelId, { kind: 'idle' });
          return;
        }
        const message = friendlyStopError(err);
        setStatus(modelId, { kind: 'error', message });
      }
    },
    [setStatus],
  );

  const clearError = useCallback(
    (modelId: string) => {
      const current = statusesRef.current.get(modelId);
      if (current?.kind === 'error') setStatus(modelId, { kind: 'idle' });
    },
    [setStatus],
  );

  return useMemo<MlxServersApi>(
    () => ({ statuses, statusOf, handleOf, start, stop, clearError }),
    [statuses, statusOf, handleOf, start, stop, clearError],
  );
}

function friendlyStartError(err: unknown): string {
  if (isIpcError(err)) {
    if (err.kind === 'NeedsApproval') {
      return 'Trust the project to start a Plume-managed server.';
    }
    if (err.kind === 'BadArgument') {
      return ipcErrorMessage(err);
    }
    if (err.kind === 'NotFound') {
      return 'Model not in inventory. Refresh providers and try again.';
    }
    if (err.kind === 'Internal') {
      // The supervisor surfaces "spawn failed (is python installed? …)"
      // here; the message already contains the actionable hint.
      return ipcErrorMessage(err);
    }
    return ipcErrorMessage(err);
  }
  return err instanceof Error ? err.message : String(err);
}

function friendlyStopError(err: unknown): string {
  if (isIpcError(err)) return ipcErrorMessage(err);
  return err instanceof Error ? err.message : String(err);
}
