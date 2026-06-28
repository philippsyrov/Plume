// D101: a tiny, dependency-free summary of which files a unified diff
// touches, for the "changed files" line shown above Apply in the single-step
// panel. It operates purely on the diff text — the diff is only ever rendered
// after the backend's `patch.validate` accepted it, so the `---`/`+++` header
// pairs are trustworthy.
//
// This is NOT a general diff parser and is NOT a security boundary: it reads
// file headers only, enough for a human-readable summary. The authoritative
// touched-file set (including rename detection) lives in the server-side
// validator that gates every apply; this is a UI hint that mirrors it.

export type ChangedFileKind = 'create' | 'delete' | 'modify';

export type ChangedFile = {
  /** Project-relative path, with any `a/`/`b/` prefix stripped. */
  path: string;
  kind: ChangedFileKind;
};

/** Strip an `a/`/`b/` prefix and any trailing tab-delimited timestamp a
 *  unified-diff header may carry. `/dev/null` is returned verbatim so the
 *  caller can detect create/delete. */
function headerPath(raw: string): string {
  const noTimestamp = raw.split('\t')[0].trim();
  if (noTimestamp === '/dev/null') return '/dev/null';
  return noTimestamp.replace(/^[ab]\//, '');
}

export function summarizeDiffFiles(diff: string): ChangedFile[] {
  const lines = diff.split('\n');
  const files: ChangedFile[] = [];
  for (let i = 0; i < lines.length; i++) {
    const minus = lines[i];
    // A file header is a `--- ` line immediately followed by a `+++ ` line.
    // Requiring the pair makes a stray `-- ...` content line inside a hunk
    // (which `classifyDiffLine` would also read as a header) far less likely
    // to false-match here.
    if (!minus.startsWith('--- ')) continue;
    const plus = lines[i + 1];
    if (!plus || !plus.startsWith('+++ ')) continue;

    const oldPath = headerPath(minus.slice(4));
    const newPath = headerPath(plus.slice(4));
    const kind: ChangedFileKind =
      oldPath === '/dev/null' ? 'create' : newPath === '/dev/null' ? 'delete' : 'modify';
    // On delete the new side is `/dev/null`, so the meaningful path is the old.
    files.push({ path: kind === 'delete' ? oldPath : newPath, kind });
    i++; // consumed the `+++` line too
  }
  return files;
}

const MAX_NAMES = 3;

/** One tiny line: "2 files · a.ts, b.ts (create)" — non-modify kinds are
 *  tagged so a create/delete reads unambiguously. Caps the name list so the
 *  summary stays one short line even for a wide diff. Empty string for no
 *  detectable files (the caller renders nothing). */
export function changedFilesSummary(files: ChangedFile[]): string {
  if (files.length === 0) return '';
  const fileWord = files.length === 1 ? 'file' : 'files';
  const shown = files
    .slice(0, MAX_NAMES)
    .map((f) => (f.kind === 'modify' ? f.path : `${f.path} (${f.kind})`));
  const extra = files.length - shown.length;
  const names = extra > 0 ? `${shown.join(', ')}, +${extra} more` : shown.join(', ');
  return `${files.length} ${fileWord} · ${names}`;
}
