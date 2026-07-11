// D129: shell-entrypoint smoke tests. The three reserved commands
// (benchmark-model.sh, benchmark-suite.sh, summarize-benchmarks.ts)
// are exercised exactly as a user would run them, end to end against
// the fake runtime. Slowish (each spawns vite-node) — kept to three
// invocations.

import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterAll, describe, expect, it } from 'vitest';

import { readRecords } from './summarize-lib.ts';
import { REPO_ROOT } from './test-support.ts';

const tmp = mkdtempSync(path.join(os.tmpdir(), 'plume-bench-cli-'));
afterAll(() => rmSync(tmp, { recursive: true, force: true }));

const run = (file: string, args: string[]): string =>
  execFileSync(file, args, { cwd: REPO_ROOT, encoding: 'utf8', timeout: 120_000 });

describe('reserved command shapes', () => {
  it('benchmark-model.sh records one valid attempt', () => {
    const outFile = path.join(tmp, 'one.jsonl');
    const stdout = run('bash', [
      'scripts/benchmark-model.sh',
      '--config', 'benchmarks/plans/fake-config.json',
      '--fixture', 'benchmarks/fixtures/short-chat/fact-001',
      '--out', outFile,
      '--population', 'warm',
      '--repetition', '1',
      '--planned', '3',
    ]);
    expect(stdout).toContain('recorded');
    const result = readRecords(readFileSync(outFile, 'utf8'));
    expect(result.lineErrors).toEqual([]);
    expect(result.records).toHaveLength(1);
    expect(result.records[0]?.outcome.status).toBe('passed');
  });

  it('benchmark-suite.sh runs a plan and summarize-benchmarks.ts renders it', () => {
    const outFile = path.join(tmp, 'suite.jsonl');
    const plan = {
      config: 'benchmarks/plans/fake-config.json',
      outFile,
      groups: [
        {
          groupId: 'grp_cli_smoke',
          fixture: 'benchmarks/fixtures/short-chat/fact-001',
          population: 'warm',
          repetitions: 3,
        },
      ],
    };
    const planFile = path.join(tmp, 'plan.json');
    writeFileSync(planFile, JSON.stringify(plan));
    const stdout = run('bash', ['scripts/benchmark-suite.sh', planFile]);
    expect(stdout).toContain('recorded 3 attempts');

    const result = readRecords(readFileSync(outFile, 'utf8'));
    expect(result.lineErrors).toEqual([]);
    expect(result.records).toHaveLength(3);
    expect(new Set(result.records.map((r) => r.run.repetition))).toEqual(new Set([1, 2, 3]));

    const markdown = run('npx', ['--no-install', 'vite-node', 'scripts/summarize-benchmarks.ts', '--', outFile]);
    expect(markdown).toContain('HARNESS TEST DATA');
    expect(markdown).toContain('grp_cli_smoke');
    expect(markdown).toContain('| warm |');
  });
});
