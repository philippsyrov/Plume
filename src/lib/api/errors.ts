// IPC error types. Mirrors `IpcError` in `docs/IPC_CONTRACT.md` and
// `src-tauri/src/error.rs`. Components match on `kind`; never parse
// `message` or `details` for control flow.

export type IpcError =
  | { kind: 'PathEscape'; details: string }
  | { kind: 'NotFound'; details: string }
  | { kind: 'NeedsApproval' }
  | { kind: 'Cancelled' }
  | { kind: 'ProviderDown'; details: { provider: string; reason: string } }
  | { kind: 'BadArgument'; details: string }
  | { kind: 'Blocked'; details: string }
  // A durable store has no room for the write. Its own kind, not a
  // `Blocked`, because it is the one refusal the user can clear themselves
  // — and because a follow-up usage read cannot re-derive it: the refusal is
  // decided on projected size, so the store is often still below its cap.
  | { kind: 'StorageFull'; details: { usedBytes: number; capBytes: number } }
  | { kind: 'Internal'; details: string }
  | { kind: 'Version'; details: { wanted: number; speaks: number } };

export function isIpcError(value: unknown): value is IpcError {
  if (typeof value !== 'object' || value === null) return false;
  const k = (value as { kind?: unknown }).kind;
  return (
    k === 'PathEscape' ||
    k === 'NotFound' ||
    k === 'NeedsApproval' ||
    k === 'Cancelled' ||
    k === 'ProviderDown' ||
    k === 'BadArgument' ||
    k === 'Blocked' ||
    k === 'StorageFull' ||
    k === 'Internal' ||
    k === 'Version'
  );
}

/// Render an IpcError for display. Per-kind text — never just the
/// raw `details` blob, which is meant for logs and field-by-field UI.
export function ipcErrorMessage(err: IpcError): string {
  switch (err.kind) {
    case 'PathEscape':
      return `Path is outside the project: ${err.details}`;
    case 'NotFound':
      return `Not found: ${err.details}`;
    case 'NeedsApproval':
      return 'This operation requires your approval.';
    case 'Cancelled':
      return 'Operation cancelled.';
    case 'ProviderDown':
      return `Provider ${err.details.provider} is unavailable: ${err.details.reason}`;
    case 'BadArgument':
      return `Invalid argument: ${err.details}`;
    case 'Blocked':
      return `Blocked: ${err.details}`;
    case 'StorageFull':
      return `This chat store has no room for that (${megabytes(err.details.usedBytes)} MB of ${megabytes(err.details.capBytes)} MB used).`;
    case 'Internal':
      return `Internal error: ${err.details}`;
    case 'Version':
      return `IPC version mismatch (frontend ${err.details.wanted}, backend ${err.details.speaks}).`;
  }
}

/** Whole megabytes — the surface reports scale, not exact bytes. */
function megabytes(bytes: number): number {
  return Math.round(bytes / (1024 * 1024));
}
