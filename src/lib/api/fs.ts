// Typed wrapper for fs IPC commands. Both verbs require a currently
// open trusted project; the backend resolves the canonical root from
// session state. Frontend never supplies a project root.
//
// Display reads only — see `docs/IPC_CONTRACT.md` § fs. The prompt-read
// path will land separately and never share output with these.

import { invokeIpc } from './ipc';

export type FileKind = 'file' | 'dir' | 'symlink';

export type FileEntry = {
  name: string;
  path: string;
  kind: FileKind;
  size: number | null;
  modifiedMs: number;
};

export type FileEncoding = 'utf-8' | 'binary';

export type FileContent = {
  content: string;
  encoding: FileEncoding;
  bytes: number;
};

type PathPayload = { path: string };

/// List direct children. Pass `''` or `'.'` for the project root,
/// a relative path under the root, or an absolute path inside the
/// root. The backend rejects anything that escapes.
export function listDir(path: string): Promise<FileEntry[]> {
  return invokeIpc<PathPayload, FileEntry[]>('fs_list', { path });
}

/// Read a file for display. Returns `encoding: 'binary'` with empty
/// content when the file isn't UTF-8.
export function readFile(path: string): Promise<FileContent> {
  return invokeIpc<PathPayload, FileContent>('fs_read', { path });
}
