// @vitest-environment node

import { mkdtempSync, mkdirSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

import { checkMarkdownLinks } from './markdown-links.ts';

describe('checkMarkdownLinks', () => {
  it('accepts relative files and GitHub-style heading anchors', () => {
    const root = mkdtempSync(join(tmpdir(), 'plume-links-'));
    mkdirSync(join(root, 'docs'));
    writeFileSync(join(root, 'README.md'), '[Safety](docs/SAFETY.md#hard-links)');
    writeFileSync(join(root, 'docs/SAFETY.md'), '# Safety\n\n## Hard links\n');
    expect(checkMarkdownLinks(root, ['README.md', 'docs/SAFETY.md'])).toEqual([]);
  });

  it('reports missing files and missing anchors with the source path', () => {
    const root = mkdtempSync(join(tmpdir(), 'plume-links-'));
    writeFileSync(join(root, 'README.md'), '[Nope](docs/NOPE.md) [Bad](#missing)');
    expect(checkMarkdownLinks(root, ['README.md']).map((issue) => issue.kind)).toEqual([
      'missingFile',
      'missingAnchor',
    ]);
  });

  it('rejects repository escapes and ignores external URLs and fenced code', () => {
    const root = mkdtempSync(join(tmpdir(), 'plume-links-'));
    writeFileSync(
      join(root, 'README.md'),
      '[escape](../secret.md) [web](https://example.com)\n```md\n[x](missing.md)\n```',
    );
    expect(checkMarkdownLinks(root, ['README.md'])).toMatchObject([
      { kind: 'pathEscape', source: 'README.md' },
    ]);
  });

  it('sorts distinct issue fields canonically instead of preserving discovery order', () => {
    const root = mkdtempSync(join(tmpdir(), 'plume-links-'));
    writeFileSync(join(root, 'README.md'), '[Compact](missinga.md) [Hyphenated](missing-a.md)');
    expect(
      checkMarkdownLinks(root, ['README.md']).map(({ source, target, kind }) => ({
        source,
        target,
        kind,
      })),
    ).toEqual([
      { source: 'README.md', target: 'missing-a.md', kind: 'missingFile' },
      { source: 'README.md', target: 'missinga.md', kind: 'missingFile' },
    ]);
  });
});
