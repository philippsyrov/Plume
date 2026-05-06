// Typed wrapper around `@tauri-apps/api/core` invoke. Adds the
// `IpcRequest` envelope so every command stamps `ipcVersion`
// the same way. See `docs/IPC_CONTRACT.md`.

import { invoke } from '@tauri-apps/api/core';

import { isIpcError, type IpcError } from './errors';

export const IPC_VERSION = 1 as const;

export type IpcRequest<T> = {
  ipcVersion: typeof IPC_VERSION;
  payload: T;
};

/// Invoke a Tauri command using Plume's envelope. The Rust handler
/// is expected to take a single `req: IpcRequest<T>` argument; Tauri
/// looks up arguments by name, hence the wrapping `{ req }`.
///
/// Rejections from the backend land in the catch path with shape
/// `IpcError`. Anything else (network glitch, missing Tauri runtime
/// in pure-vite dev) is wrapped as `Internal`.
export async function invokeIpc<TPayload, TResponse>(
  command: string,
  payload: TPayload,
): Promise<TResponse> {
  const req: IpcRequest<TPayload> = {
    ipcVersion: IPC_VERSION,
    payload,
  };
  try {
    return await invoke<TResponse>(command, { req });
  } catch (raw) {
    throw normalizeError(raw);
  }
}

function normalizeError(raw: unknown): IpcError {
  if (isIpcError(raw)) return raw;
  if (raw instanceof Error) {
    return { kind: 'Internal', details: raw.message };
  }
  if (typeof raw === 'string') {
    return { kind: 'Internal', details: raw };
  }
  return { kind: 'Internal', details: 'unknown ipc error' };
}
