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
    case 'Internal':
      return `Internal error: ${err.details}`;
    case 'Version':
      return `IPC version mismatch (frontend ${err.details.wanted}, backend ${err.details.speaks}).`;
  }
}
