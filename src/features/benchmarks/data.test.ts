// D132: data layer of the benchmark results viewer. The fs IPC surface
// is mocked with an in-memory tree; records come from the harness's
// own canonical example record, so these tests exercise the REAL
// reader validation and summarizer — only the transport is fake.

import { beforeEach, describe, expect, it, vi } from 'vitest';

import { makeValidRecord } from '../../../scripts/benchmark/example-record.ts';
import type { BenchmarkRecord } from '../../../scripts/benchmark/types.ts';

const mocks = vi.hoisted(() => ({
  listDir: vi.fn(),
  readFile: vi.fn(),
}));

vi.mock('../../lib/api/fs', () => ({
  listDir: mocks.listDir,
  readFile: mocks.readFile,
}));

import { loadBenchmarkEvidence } from './data';

/// In-memory project tree: keys are repo-relative file paths. listDir
/// and readFile behave like the real verbs: NotFound for unknown
/// paths, utf-8 file content otherwise.
function installTree(tree: Record<string, string>): void {
  const filePaths = Object.keys(tree);
  mocks.listDir.mockImplementation((dir: string) => {
    const prefix = `${dir}/`;
    const children = new Map<string, 'file' | 'dir'>();
    for (const filePath of filePaths) {
      if (!filePath.startsWith(prefix)) continue;
      const rest = filePath.slice(prefix.length);
      const name = rest.split('/')[0] ?? rest;
      children.set(name, rest.includes('/') ? 'dir' : 'file');
    }
    if (children.size === 0) {
      return Promise.reject({ kind: 'NotFound', details: dir });
    }
    return Promise.resolve(
      [...children.entries()].map(([name, kind]) => ({
        name,
        path: `/project/${dir}/${name}`,
        kind,
        size: kind === 'file' ? 1 : null,
        modifiedMs: 0,
      })),
    );
  });
  mocks.readFile.mockImplementation((path: string) => {
    const content = tree[path];
    if (content === undefined) {
      return Promise.reject({ kind: 'NotFound', details: path });
    }
    return Promise.resolve({ content, encoding: 'utf-8', bytes: content.length });
  });
}

function recordLine(mutate?: (record: BenchmarkRecord) => void): string {
  const record = makeValidRecord();
  if (mutate) mutate(record);
  return JSON.stringify(record);
}

const VALID_MODELS_JSON = JSON.stringify({
  schemaVersion: 1,
  models: [
    {
      id: 'fake-model',
      displayName: 'Fake Model',
      folder: 'Fake-Folder',
      engine: 'mlx-lm',
      maxContextTokens: 8192,
      artifact: {
        format: 'mlx',
        sha256: `sha256:${'ab'.repeat(32)}`,
        quantizationMethod: 'affine',
        quantizationBits: 4,
        quantizationGroupSize: 64,
      },
    },
  ],
});

function presetsJson(fixture: string): string {
  return JSON.stringify({
    schemaVersion: 1,
    presets: [
      {
        id: 'fake-preset',
        description: 'A fake preset for viewer tests',
        model: 'fake-model',
        measurementPaths: ['rawRuntime'],
        generation: {
          temperature: 0,
          topP: 1,
          topK: null,
          minP: null,
          repeatPenalty: null,
          seed: 42,
          maxOutputTokens: 64,
          stopSequences: [],
        },
        contextTokens: 4096,
        suites: [{ fixture, populations: ['warm'], repetitions: 3 }],
      },
    ],
  });
}

const CATALOG_TREE = {
  'benchmarks/catalog/models.json': VALID_MODELS_JSON,
  'benchmarks/catalog/presets.json': presetsJson('short-chat/pong-001'),
  'benchmarks/fixtures/short-chat/pong-001/manifest.json': '{}',
};

beforeEach(() => {
  mocks.listDir.mockReset();
  mocks.readFile.mockReset();
});

describe('loadBenchmarkEvidence', () => {
  it('reports absent artifacts and absent catalog when neither exists', async () => {
    installTree({});
    const evidence = await loadBenchmarkEvidence();
    expect(evidence.artifacts).toEqual({ kind: 'absent' });
    expect(evidence.catalog).toEqual({ kind: 'absent' });
  });

  it('walks nested run directories and summarizes valid records', async () => {
    const lines = [1, 2, 3].map((rep) =>
      recordLine((r) => {
        r.run.id = `bench_0${rep}`;
        r.run.repetition = rep;
      }),
    );
    installTree({
      ...CATALOG_TREE,
      'benchmark-artifacts/presets/fake-preset/records.jsonl': lines.join('\n'),
    });
    const evidence = await loadBenchmarkEvidence();
    if (evidence.artifacts.kind !== 'loaded') throw new Error('expected loaded artifacts');
    expect(evidence.artifacts.files).toHaveLength(1);
    const file = evidence.artifacts.files[0]!;
    expect(file.path).toBe('benchmark-artifacts/presets/fake-preset/records.jsonl');
    expect(file.runLabel).toBe('presets/fake-preset');
    expect(file.readError).toBeNull();
    expect(file.records).toHaveLength(3);
    expect(file.lineErrors).toEqual([]);
    // Three completed included attempts → the real summarizer produces stats.
    expect(file.groups).toHaveLength(1);
    expect(file.groups[0]!.endToEndMs?.median).toBe(55.0);
    // The example record uses the scripted fake engine — bannered.
    expect(file.hasFakeRuntime).toBe(true);
  });

  it('excludes invalid lines with visible errors and keeps the valid rest', async () => {
    const good = recordLine();
    installTree({
      'benchmark-artifacts/run/records.jsonl': `${good}\nnot json at all\n`,
    });
    const evidence = await loadBenchmarkEvidence();
    if (evidence.artifacts.kind !== 'loaded') throw new Error('expected loaded artifacts');
    const file = evidence.artifacts.files[0]!;
    expect(file.records).toHaveLength(1);
    expect(file.lineErrors).toHaveLength(1);
    expect(file.lineErrors[0]).toContain('line 2');
  });

  it('computes pair summaries from paired raw/plume records', async () => {
    const raw = recordLine((r) => {
      r.run.id = 'bench_raw';
      r.run.pairId = 'pair_pong_warm_1';
      r.timing.endToEndMs = 50;
    });
    const plume = recordLine((r) => {
      r.run.id = 'bench_plume';
      r.run.groupId = 'grp_plume';
      r.run.pairId = 'pair_pong_warm_1';
      r.run.measurementPath = 'plumeOrchestration';
      r.timing.endToEndMs = 62;
    });
    installTree({ 'benchmark-artifacts/run/records.jsonl': `${raw}\n${plume}` });
    const evidence = await loadBenchmarkEvidence();
    if (evidence.artifacts.kind !== 'loaded') throw new Error('expected loaded artifacts');
    const pairs = evidence.artifacts.files[0]!.pairs;
    expect(pairs).toEqual([
      { pairId: 'pair_pong_warm_1', valid: true, reason: null, extraOverheadMs: 12 },
    ]);
  });

  it('refuses a whole file whose read fails, without dropping it from the list', async () => {
    installTree({ 'benchmark-artifacts/run/records.jsonl': recordLine() });
    mocks.readFile.mockImplementation((path: string) =>
      path.endsWith('records.jsonl')
        ? Promise.reject({ kind: 'BadArgument', details: 'file too large for display read' })
        : Promise.reject({ kind: 'NotFound', details: path }),
    );
    const evidence = await loadBenchmarkEvidence();
    if (evidence.artifacts.kind !== 'loaded') throw new Error('expected loaded artifacts');
    const file = evidence.artifacts.files[0]!;
    expect(file.readError).toContain('file too large');
    expect(file.records).toEqual([]);
  });

  it('refuses a binary file as not UTF-8', async () => {
    installTree({ 'benchmark-artifacts/run/records.jsonl': 'x' });
    mocks.readFile.mockResolvedValue({ content: '', encoding: 'binary', bytes: 4 });
    const evidence = await loadBenchmarkEvidence();
    if (evidence.artifacts.kind !== 'loaded') throw new Error('expected loaded artifacts');
    expect(evidence.artifacts.files[0]!.readError).toContain('UTF-8');
  });

  it('refuses a whole file on a newer schema version', async () => {
    const newer = recordLine((r) => {
      (r as { schemaVersion: number }).schemaVersion = 2;
    });
    installTree({ 'benchmark-artifacts/run/records.jsonl': newer });
    const evidence = await loadBenchmarkEvidence();
    if (evidence.artifacts.kind !== 'loaded') throw new Error('expected loaded artifacts');
    const file = evidence.artifacts.files[0]!;
    expect(file.readError).toContain('newer than supported');
    expect(file.records).toEqual([]);
  });

  it('loads a valid catalog through the shared strict parser', async () => {
    installTree(CATALOG_TREE);
    const evidence = await loadBenchmarkEvidence();
    if (evidence.catalog.kind !== 'loaded') throw new Error('expected loaded catalog');
    expect([...evidence.catalog.catalog.models.keys()]).toEqual(['fake-model']);
    expect([...evidence.catalog.catalog.presets.keys()]).toEqual(['fake-preset']);
  });

  it('surfaces the strict loader refusal for an agent-suite preset', async () => {
    installTree({
      ...CATALOG_TREE,
      'benchmarks/catalog/presets.json': presetsJson('single-file-bug-fix/bug-001'),
      'benchmarks/fixtures/single-file-bug-fix/bug-001/manifest.json': '{}',
    });
    const evidence = await loadBenchmarkEvidence();
    if (evidence.catalog.kind !== 'error') throw new Error('expected catalog error');
    // The distinctive D131 refusal message proves the viewer runs the
    // SAME loader as the CLI, not a lenient display-grade copy.
    expect(evidence.catalog.message).toContain('cannot be honestly measured by any current path');
  });

  it('refuses a catalog whose fixture does not exist on disk', async () => {
    installTree({
      'benchmarks/catalog/models.json': VALID_MODELS_JSON,
      'benchmarks/catalog/presets.json': presetsJson('short-chat/no-such-case'),
    });
    const evidence = await loadBenchmarkEvidence();
    if (evidence.catalog.kind !== 'error') throw new Error('expected catalog error');
    expect(evidence.catalog.message).toContain('does not exist under benchmarks/fixtures');
  });

  it('treats a missing catalog with present artifacts as absent catalog only', async () => {
    installTree({ 'benchmark-artifacts/run/records.jsonl': recordLine() });
    const evidence = await loadBenchmarkEvidence();
    expect(evidence.catalog).toEqual({ kind: 'absent' });
    expect(evidence.artifacts.kind).toBe('loaded');
  });
});
