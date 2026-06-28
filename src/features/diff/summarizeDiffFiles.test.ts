import { describe, expect, it } from 'vitest';

import { changedFilesSummary, summarizeDiffFiles } from './summarizeDiffFiles';

describe('summarizeDiffFiles', () => {
  it('reads a single modified file, stripping the a/ b/ prefixes', () => {
    const diff = '--- a/src/x.txt\n+++ b/src/x.txt\n@@ -1 +1 @@\n-a\n+b\n';
    expect(summarizeDiffFiles(diff)).toEqual([{ path: 'src/x.txt', kind: 'modify' }]);
  });

  it('classifies a create (--- /dev/null) and reports the new path', () => {
    const diff = '--- /dev/null\n+++ b/new.py\n@@ -0,0 +1 @@\n+print(1)\n';
    expect(summarizeDiffFiles(diff)).toEqual([{ path: 'new.py', kind: 'create' }]);
  });

  it('classifies a delete (+++ /dev/null) and reports the old path', () => {
    const diff = '--- a/gone.py\n+++ /dev/null\n@@ -1 +0,0 @@\n-print(1)\n';
    expect(summarizeDiffFiles(diff)).toEqual([{ path: 'gone.py', kind: 'delete' }]);
  });

  it('reads multiple files in one diff', () => {
    const diff =
      '--- a/one.ts\n+++ b/one.ts\n@@ -1 +1 @@\n-1\n+2\n' +
      '--- a/two.ts\n+++ b/two.ts\n@@ -1 +1 @@\n-3\n+4\n';
    expect(summarizeDiffFiles(diff)).toEqual([
      { path: 'one.ts', kind: 'modify' },
      { path: 'two.ts', kind: 'modify' },
    ]);
  });

  it('drops a trailing tab-delimited timestamp from the header path', () => {
    const diff = '--- a/x.txt\t2026-01-01 00:00:00\n+++ b/x.txt\t2026-01-02 00:00:00\n@@ -1 +1 @@\n-a\n+b\n';
    expect(summarizeDiffFiles(diff)).toEqual([{ path: 'x.txt', kind: 'modify' }]);
  });

  it('does not treat removed/added hunk content as a new file header', () => {
    // A removed line whose content begins with "-- " is NOT immediately
    // followed by a "+++ " line, so it must not register as a file.
    const diff = '--- a/notes.md\n+++ b/notes.md\n@@ -1,2 +1,2 @@\n--- old rule\n+keeps\n context\n';
    expect(summarizeDiffFiles(diff)).toEqual([{ path: 'notes.md', kind: 'modify' }]);
  });

  it('returns an empty list for text with no file headers', () => {
    expect(summarizeDiffFiles('not a diff at all')).toEqual([]);
  });
});

describe('changedFilesSummary', () => {
  it('is empty for no files', () => {
    expect(changedFilesSummary([])).toBe('');
  });

  it('reads one modified file without a kind tag', () => {
    expect(changedFilesSummary([{ path: 'x.txt', kind: 'modify' }])).toBe('1 file · x.txt');
  });

  it('tags non-modify kinds', () => {
    expect(changedFilesSummary([{ path: 'new.py', kind: 'create' }])).toBe('1 file · new.py (create)');
  });

  it('pluralizes and caps the name list at three with a +N more', () => {
    const files = [
      { path: 'a', kind: 'modify' as const },
      { path: 'b', kind: 'modify' as const },
      { path: 'c', kind: 'modify' as const },
      { path: 'd', kind: 'modify' as const },
      { path: 'e', kind: 'modify' as const },
    ];
    expect(changedFilesSummary(files)).toBe('5 files · a, b, c, +2 more');
  });
});
