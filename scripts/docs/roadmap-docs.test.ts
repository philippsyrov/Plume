// @vitest-environment node

import { mkdtempSync, mkdirSync, writeFileSync } from 'node:fs';
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
