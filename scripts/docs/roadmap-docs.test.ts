// @vitest-environment node

import { mkdtempSync, mkdirSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { describe, expect, it } from 'vitest';

import { checkRoadmapDocs, type DocsCheckResult } from './roadmap-docs.ts';

type InventoryRecord = {
  id: string;
  track: string;
  status: string;
  currentBehavior: string;
  missingBehavior: string;
  frontendReachability: string;
  backendReachability: string;
  automatedEvidence: string[];
  manualOrHardwareEvidence: string;
  dependencies: string[];
  implementationPaths: string[];
  sourceDocuments: string[];
  nextCommissionedSlice: string;
  lastVerifiedCommit: string;
  lastVerifiedDate: string;
};

type GitRunner = (args: string[]) => { ok: boolean; stdout: string };

function validRecord(overrides: Partial<InventoryRecord> = {}): InventoryRecord {
  return {
    id: 'chat.streaming',
    track: 'local-chat',
    status: 'shipped',
    currentBehavior: 'Chat streams tokens.',
    missingBehavior: 'More providers remain.',
    frontendReachability: 'Chat panel.',
    backendReachability: 'chat.send.',
    automatedEvidence: ['src/chat.test.ts'],
    manualOrHardwareEvidence: 'not required',
    dependencies: ['trusted project'],
    implementationPaths: ['src/chat.ts'],
    sourceDocuments: ['docs/IPC_CONTRACT.md'],
    nextCommissionedSlice: 'Keep adapters aligned',
    lastVerifiedCommit: 'abc123',
    lastVerifiedDate: '2026-07-13',
    ...overrides,
  };
}

function writeFixture(records: unknown[], options: { research?: string; archive?: string } = {}): string {
  const root = mkdtempSync(join(tmpdir(), 'plume-roadmap-docs-'));
  mkdirSync(join(root, 'docs/research'), { recursive: true });
  mkdirSync(join(root, 'docs/archive'), { recursive: true });
  mkdirSync(join(root, 'src'), { recursive: true });
  writeFileSync(join(root, 'src/chat.test.ts'), 'test evidence\n');
  writeFileSync(join(root, 'src/chat.ts'), 'implementation\n');
  writeFileSync(join(root, 'docs/IPC_CONTRACT.md'), '# IPC contract\n');
  writeFileSync(
    join(root, 'docs/FEATURE_INVENTORY.md'),
    `# Inventory\n\n\`\`\`inventory-json\n${JSON.stringify(records, null, 2)}\n\`\`\`\n`,
  );
  writeFileSync(join(root, 'docs/research/README.md'), '# Research\n');
  writeFileSync(join(root, 'docs/archive/README.md'), '# Archive\n');
  if (options.research !== undefined) writeFileSync(join(root, 'docs/research/example.md'), options.research);
  if (options.archive !== undefined) writeFileSync(join(root, 'docs/archive/example.md'), options.archive);
  return root;
}

function unchangedGit(args: string[]): { ok: boolean; stdout: string } {
  if (args[0] === 'merge-base') return { ok: true, stdout: '' };
  return { ok: true, stdout: '' };
}

function check(root: string, git: GitRunner = unchangedGit): DocsCheckResult {
  return checkRoadmapDocs({ root, git });
}

describe('checkRoadmapDocs', () => {
  it('rejects an unknown inventory status', () => {
    const result = check(writeFixture([validRecord({ status: 'planned' })]));

    expect(result.errors).toContain("inventory record chat.streaming has unknown status 'planned'");
  });

  it('rejects a shipped record with empty automated evidence', () => {
    const result = check(writeFixture([validRecord({ automatedEvidence: [] })]));

    expect(result.errors).toContain('shipped inventory record chat.streaming must name automatedEvidence');
  });

  it('accepts a researched record with empty implementation paths', () => {
    const record = validRecord({
      id: 'knowledge.workspace',
      status: 'researched',
      automatedEvidence: [],
      implementationPaths: [],
    });

    expect(check(writeFixture([record]))).toEqual({ errors: [], warnings: [] });
  });

  it('requires shipped evidence paths to be non-empty safe existing regular files', () => {
    const outside = mkdtempSync(join(tmpdir(), 'plume-roadmap-outside-'));
    writeFileSync(join(outside, 'evidence.test.ts'), 'outside\n');
    const records = [
      validRecord({ id: 'empty', automatedEvidence: [''] }),
      validRecord({ id: 'missing', automatedEvidence: ['src/missing.test.ts'] }),
      validRecord({ id: 'directory', automatedEvidence: ['src'] }),
      validRecord({ id: 'lexical-escape', automatedEvidence: ['../outside.test.ts'] }),
      validRecord({ id: 'symlink-escape', automatedEvidence: ['linked/evidence.test.ts'] }),
    ];
    const root = writeFixture(records);
    symlinkSync(outside, join(root, 'linked'));

    const result = check(root);

    for (const id of records.map((record) => record.id)) {
      expect(result.errors).toEqual(
        expect.arrayContaining([expect.stringContaining(`inventory record ${id} automatedEvidence`)]),
      );
    }
  });

  it.each(['shipped', 'partial', 'scaffold'])(
    'requires %s implementation paths to be non-empty safe existing files or directories',
    (status) => {
      const missing = validRecord({ id: `${status}-empty`, status, implementationPaths: [] });
      const unsafe = validRecord({
        id: `${status}-unsafe`,
        status,
        implementationPaths: ['../outside'],
      });
      const result = check(writeFixture([missing, unsafe]));

      expect(result.errors).toEqual(
        expect.arrayContaining([
          expect.stringContaining(`inventory record ${status}-empty must name implementationPaths`),
          expect.stringContaining(`inventory record ${status}-unsafe implementationPaths`),
        ]),
      );
    },
  );

  it('validates local source documents without making network requests', () => {
    const outside = mkdtempSync(join(tmpdir(), 'plume-roadmap-outside-'));
    writeFileSync(join(outside, 'source.md'), '# Outside\n');
    const records = [
      validRecord({ id: 'empty-source', sourceDocuments: [''] }),
      validRecord({ id: 'missing-source', sourceDocuments: ['docs/MISSING.md'] }),
      validRecord({ id: 'source-directory', sourceDocuments: ['docs'] }),
      validRecord({ id: 'source-escape', sourceDocuments: ['../SOURCE.md'] }),
      validRecord({ id: 'source-symlink', sourceDocuments: ['linked/source.md'] }),
      validRecord({
        id: 'remote-sources',
        sourceDocuments: ['https://example.com/spec', 'http://example.com/notes'],
      }),
    ];
    const root = writeFixture(records);
    symlinkSync(outside, join(root, 'linked'));

    const result = check(root);

    for (const id of ['empty-source', 'missing-source', 'source-directory', 'source-escape', 'source-symlink']) {
      expect(result.errors).toEqual(
        expect.arrayContaining([expect.stringContaining(`inventory record ${id} sourceDocuments`)]),
      );
    }
    expect(result.errors.some((error) => error.includes('remote-sources'))).toBe(false);
  });

  it('rejects inventory records whose keys do not exactly match the contract', () => {
    const { lastVerifiedDate: _omitted, ...missingKey } = validRecord();
    const unexpectedKey = { ...validRecord({ id: 'chat.extra' }), extraClaim: true };
    const result = check(writeFixture([missingKey, unexpectedKey]));

    expect(result.errors).toEqual(
      expect.arrayContaining([
        expect.stringContaining('chat.streaming'),
        expect.stringContaining('lastVerifiedDate'),
        expect.stringContaining('chat.extra'),
        expect.stringContaining('extraClaim'),
      ]),
    );
  });

  it('rejects research notes without sourceDate or hygiene metadata', () => {
    const research = [
      '```research-metadata',
      JSON.stringify({ family: 'example', sources: [], refreshTrigger: 'release' }),
      '```',
    ].join('\n');
    const result = check(writeFixture([validRecord({ status: 'researched' })], { research }));

    expect(result.errors).toEqual(
      expect.arrayContaining([
        expect.stringContaining('docs/research/example.md'),
        expect.stringContaining('sourceDate'),
        expect.stringContaining('hygiene'),
      ]),
    );
  });

  it('rejects an unknown research hygiene value', () => {
    const research = [
      '```research-metadata',
      JSON.stringify({
        family: 'example',
        sourceDate: '2026-07-13',
        hygiene: 'copied-source',
        sources: [],
        refreshTrigger: 'release',
      }),
      '```',
    ].join('\n');
    const result = check(writeFixture([validRecord({ status: 'researched' })], { research }));

    expect(result.errors).toContain(
      "docs/research/example.md has unknown research hygiene 'copied-source'",
    );
  });

  it('requires non-empty research metadata, a real calendar date, and non-empty sources', () => {
    const research = [
      '```research-metadata',
      JSON.stringify({
        family: '   ',
        sourceDate: '2026-02-30',
        hygiene: '',
        sources: [],
        refreshTrigger: '',
      }),
      '```',
    ].join('\n');
    const result = check(writeFixture([validRecord({ status: 'researched' })], { research }));

    expect(result.errors).toEqual(
      expect.arrayContaining([
        expect.stringContaining('key family must be non-empty'),
        expect.stringContaining('key sourceDate must be a real YYYY-MM-DD calendar date'),
        expect.stringContaining('key hygiene must be non-empty'),
        expect.stringContaining('key sources must be a non-empty array of strings'),
        expect.stringContaining('key refreshTrigger must be non-empty'),
      ]),
    );
  });

  it('resolves local research sources relative to the note and rejects escapes', () => {
    const outside = mkdtempSync(join(tmpdir(), 'plume-roadmap-outside-'));
    writeFileSync(join(outside, 'source.md'), '# Outside\n');
    const research = [
      '```research-metadata',
      JSON.stringify({
        family: 'example',
        sourceDate: '2026-07-13',
        hygiene: 'official-public',
        sources: ['', 'missing.md', '../../../outside.md', 'linked/source.md'],
        refreshTrigger: 'release',
      }),
      '```',
    ].join('\n');
    const root = writeFixture([validRecord({ status: 'researched' })], { research });
    symlinkSync(outside, join(root, 'docs/research/linked'));

    const result = check(root);

    expect(result.errors.filter((error) => error.includes('docs/research/example.md sources'))).toHaveLength(4);
  });

  it('accepts existing local and HTTP research sources without network access', () => {
    const research = [
      '```research-metadata',
      JSON.stringify({
        family: 'example',
        sourceDate: '2024-02-29',
        hygiene: 'official-public',
        sources: ['../IPC_CONTRACT.md', 'https://example.com/spec', 'http://example.com/notes'],
        refreshTrigger: 'release',
      }),
      '```',
    ].join('\n');

    expect(check(writeFixture([validRecord({ status: 'researched' })], { research }))).toEqual({
      errors: [],
      warnings: [],
    });
  });

  it('rejects archive notes without a Replacement line', () => {
    const result = check(
      writeFixture([validRecord({ status: 'researched' })], { archive: '# Old design\n' }),
    );

    expect(result.errors).toContain('docs/archive/example.md must contain a line beginning Replacement:');
  });

  it('does not warn when owned paths are unchanged since an ancestor commit', () => {
    const calls: string[][] = [];
    const git: GitRunner = (args) => {
      calls.push(args);
      return { ok: true, stdout: '' };
    };

    expect(check(writeFixture([validRecord()]), git)).toEqual({ errors: [], warnings: [] });
    expect(calls).toEqual([
      ['merge-base', '--is-ancestor', 'abc123', 'HEAD'],
      ['diff', '--name-only', 'abc123..HEAD', '--', 'src/chat.ts'],
    ]);
  });

  it('warns with the inventory id and each changed owned path', () => {
    const git: GitRunner = (args) =>
      args[0] === 'merge-base'
        ? { ok: true, stdout: '' }
        : { ok: true, stdout: 'src/chat.ts\n' };
    const result = check(writeFixture([validRecord()]), git);

    expect(result.warnings).toContain(
      'inventory record chat.streaming may be stale: owned path changed since abc123: src/chat.ts',
    );
  });

  it('warns and never path-diffs a non-ancestor lastVerifiedCommit', () => {
    const calls: string[][] = [];
    const git: GitRunner = (args) => {
      calls.push(args);
      return { ok: false, stdout: '' };
    };
    const result = check(writeFixture([validRecord()]), git);

    expect(result.warnings).toContain(
      'inventory record chat.streaming cannot verify abc123: commit is missing or is not an ancestor of HEAD',
    );
    expect(calls).toEqual([['merge-base', '--is-ancestor', 'abc123', 'HEAD']]);
  });
});

describe('check-roadmap-docs CLI', () => {
  const runCli = (root: string) =>
    spawnSync(
      join(process.cwd(), 'node_modules/.bin/vite-node'),
      [join(process.cwd(), 'scripts/check-roadmap-docs.ts')],
      { cwd: root, encoding: 'utf8' },
    );

  it('exits zero when the checker reports warnings alone', () => {
    const result = runCli(writeFixture([validRecord()]));

    expect(result.status).toBe(0);
    expect(result.stderr).toContain('warning:');
    expect(result.stderr).not.toContain('error:');
  });

  it('exits one when the checker reports an error', () => {
    const result = runCli(writeFixture([validRecord({ status: 'planned' })]));

    expect(result.status).toBe(1);
    expect(result.stderr).toContain('error:');
  });
});
