// @vitest-environment node
//
// D131: catalog + preset tests — the shipped catalog must load
// cleanly; every refusal class refuses; expansion binds to fakes
// deterministically (fake checkpoint, env-declared fake sidecar) and
// re-verifies the catalog's pins against "disk". runMatrix runs a
// tiny fake-runtime matrix end to end.

import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterAll, describe, expect, it } from 'vitest';

import { expandPreset, loadCatalog } from './catalog.ts';
import type { Catalog } from './catalog.ts';
import { digestModelDir } from './model-identity.ts';
import { runMatrix } from './matrix.ts';
import { readRecords, summarizePairs } from './summarize-lib.ts';
import { fakeConfig, fixtureDir, withPlumeEnv } from './test-support.ts';

const dir = mkdtempSync(path.join(os.tmpdir(), 'plume-catalog-'));
afterAll(() => rmSync(dir, { recursive: true, force: true }));

// ---- fakes ----------------------------------------------------------------

// A fake checkpoint whose config.json carries the MLX quantization
// shape the catalog pins.
const modelsRoot = path.join(dir, 'models-root');
const fakeFolder = 'Fake-Coder-4bit';
mkdirSync(path.join(modelsRoot, fakeFolder), { recursive: true });
writeFileSync(
  path.join(modelsRoot, fakeFolder, 'config.json'),
  JSON.stringify({ model_type: 'synthetic', quantization: { bits: 4, group_size: 64 } }),
);
writeFileSync(path.join(modelsRoot, fakeFolder, 'model.safetensors'), 'tiny fake weights');
const fakeDigest = digestModelDir(path.join(modelsRoot, fakeFolder));

// Env-declared fake sidecar (same posture as the D129C test fake).
const fakeSidecar = path.join(dir, 'fake-plume-bench.mjs');
writeFileSync(
  fakeSidecar,
  `#!/usr/bin/env node
if (process.argv[2] === 'identity') {
  const sha = process.env.PLUME_BENCH_GIT_SHA ?? null;
  const dirtyRaw = process.env.PLUME_BENCH_DIRTY ?? null;
  console.log(JSON.stringify({ ok: true, gitSha: sha, dirty: dirtyRaw === null ? null : dirtyRaw === 'true', maxOutputTokens: 4096 }));
  process.exit(0);
}
process.exit(2);
`,
);
chmodSync(fakeSidecar, 0o755);

interface CatalogJson {
  models?: unknown;
  presets?: unknown;
  schemaVersion?: unknown;
}

let counter = 0;
function writeCatalog(models: CatalogJson | unknown[], presets: CatalogJson | unknown[]): string {
  counter += 1;
  const catalogDir = path.join(dir, `catalog-${counter}`);
  mkdirSync(catalogDir, { recursive: true });
  const modelsDoc = Array.isArray(models) ? { schemaVersion: 1, models } : models;
  const presetsDoc = Array.isArray(presets) ? { schemaVersion: 1, presets } : presets;
  writeFileSync(path.join(catalogDir, 'models.json'), JSON.stringify(modelsDoc));
  writeFileSync(path.join(catalogDir, 'presets.json'), JSON.stringify(presetsDoc));
  return catalogDir;
}

function fakeModelEntry(overrides?: Record<string, unknown>): Record<string, unknown> {
  return {
    id: 'fake-coder-4bit',
    displayName: 'Fake Coder (4-bit)',
    folder: fakeFolder,
    engine: 'mlx-lm',
    maxContextTokens: 32768,
    artifact: {
      format: 'mlx',
      sha256: fakeDigest,
      quantizationMethod: 'affine',
      quantizationBits: 4,
      quantizationGroupSize: 64,
    },
    ...overrides,
  };
}

function fakePreset(overrides?: Record<string, unknown>): Record<string, unknown> {
  return {
    id: 'fake-paired',
    description: 'fake paired preset',
    model: 'fake-coder-4bit',
    measurementPaths: ['rawRuntime', 'plumeOrchestration'],
    generation: 'plumePosture',
    contextTokens: 8192,
    suites: [{ fixture: 'short-chat/pong-001', populations: ['warm', 'cold'], repetitions: 3 }],
    ...overrides,
  };
}

function loadFake(models?: unknown[], presets?: unknown[]): Catalog {
  return loadCatalog(writeCatalog(models ?? [fakeModelEntry()], presets ?? [fakePreset()]));
}

const DEPS = {
  python: '/usr/bin/python3-stand-in',
  resolveFolder: (folder: string): string => path.join(modelsRoot, folder),
  sidecar: fakeSidecar,
};

// ---- the shipped catalog ---------------------------------------------------

describe('the shipped catalog', () => {
  it('loads cleanly with producer strictness', () => {
    const catalog = loadCatalog();
    expect(catalog.models.has('qwen2.5-coder-3b-instruct-4bit')).toBe(true);
    expect([...catalog.presets.keys()]).toContain('pong-paired-smoke');
    expect([...catalog.presets.keys()]).toContain('full-matrix-3b');
  });
});

// ---- loader refusals --------------------------------------------------------

describe('catalog loader refusals', () => {
  it('refuses an unknown field anywhere (closed schema)', () => {
    expect(() => loadFake([fakeModelEntry({ vibe: 'immaculate' })])).toThrow(/unknown field "vibe"/);
    expect(() => loadFake(undefined, [fakePreset({ turbo: true })])).toThrow(/unknown field "turbo"/);
  });

  it('refuses duplicate ids and dangling model references', () => {
    expect(() => loadFake([fakeModelEntry(), fakeModelEntry()])).toThrow(/duplicate model id/);
    expect(() => loadFake(undefined, [fakePreset({ model: 'no-such-model' })])).toThrow(/not in the catalog/);
  });

  it('refuses an unpinned or malformed artifact digest', () => {
    const artifact = { ...(fakeModelEntry()['artifact'] as Record<string, unknown>), sha256: 'trust-me' };
    expect(() => loadFake([fakeModelEntry({ artifact })])).toThrow(/pinned "sha256:/);
  });

  it('refuses a plumeOrchestration preset with explicit client sampling', () => {
    const generation = {
      temperature: 0.0, topP: 1.0, topK: null, minP: null, repeatPenalty: null,
      seed: 42, maxOutputTokens: 64, stopSequences: [],
    };
    expect(() => loadFake(undefined, [fakePreset({ generation })])).toThrow(/must use generation "plumePosture"/);
  });

  it('refuses missing fixtures, bad repetitions, and non-subset suite paths', () => {
    expect(() =>
      loadFake(undefined, [fakePreset({ suites: [{ fixture: 'short-chat/nope-999', populations: ['warm'], repetitions: 3 }] })]),
    ).toThrow(/does not exist under benchmarks\/fixtures/);
    expect(() =>
      loadFake(undefined, [fakePreset({ suites: [{ fixture: 'short-chat/pong-001', populations: ['warm'], repetitions: 2 }] })]),
    ).toThrow(/repetitions must be 3\.\.30/);
    expect(() =>
      loadFake(undefined, [
        fakePreset({
          measurementPaths: ['rawRuntime'],
          generation: {
            temperature: 0.0, topP: null, topK: null, minP: null, repeatPenalty: null,
            seed: null, maxOutputTokens: 64, stopSequences: [],
          },
          suites: [
            {
              fixture: 'short-chat/pong-001',
              populations: ['warm'],
              repetitions: 3,
              measurementPaths: ['plumeOrchestration'],
            },
          ],
        }),
      ]),
    ).toThrow(/subset of the preset's/);
  });

  it('refuses an engine the harness cannot verify', () => {
    expect(() => loadFake([fakeModelEntry({ engine: 'llama-cpp' })])).toThrow(/engine must be "mlx-lm"/);
  });
});

// ---- expansion --------------------------------------------------------------

describe('expandPreset', () => {
  it('binds a paired preset: both paths, shared pairIds, plume posture from the sidecar cap', async () => {
    const runs = await withPlumeEnv(async () => expandPreset(loadFake(), 'fake-paired', DEPS));
    // 2 paths × (warm + cold) = 4 runs.
    expect(runs).toHaveLength(4);
    const rawWarm = runs.find((r) => r.config.measurementPath === 'rawRuntime' && r.population === 'warm');
    const plumeWarm = runs.find((r) => r.config.measurementPath === 'plumeOrchestration' && r.population === 'warm');
    if (rawWarm === undefined || plumeWarm === undefined) throw new Error('expected warm runs on both paths');
    // Paired: repetition i shares the pairId across paths.
    expect(rawWarm.pairIdFor(2)).toBe(plumeWarm.pairIdFor(2));
    expect(rawWarm.pairIdFor(1)).not.toBe(rawWarm.pairIdFor(2));
    // Plume posture from the verified handshake.
    expect(rawWarm.config.model.sampling.temperature).toBeNull();
    expect(rawWarm.config.model.sampling.maxOutputTokens).toBe(4096);
    expect(plumeWarm.config.plumeBench?.binary).toBe(fakeSidecar);
    // The raw config carries the sidecar too — agent-suite diff
    // mechanics go through Plume's validator on both paths.
    expect(rawWarm.config.plumeBench?.binary).toBe(fakeSidecar);
    expect(rawWarm.config.model.artifact.sha256).toBe(fakeDigest);
  });

  it('leaves single-path suites unpaired and honors suite-level path overrides', async () => {
    const preset = fakePreset({
      id: 'fake-mixed',
      suites: [
        { fixture: 'short-chat/pong-001', populations: ['warm'], repetitions: 3 },
        { fixture: 'single-file-bug-fix/bug-001', populations: ['warm'], repetitions: 3, measurementPaths: ['rawRuntime'] },
      ],
    });
    const runs = await withPlumeEnv(async () => expandPreset(loadFake(undefined, [preset]), 'fake-mixed', DEPS));
    expect(runs).toHaveLength(3); // pong on both paths + bug-fix raw-only
    const bugFix = runs.find((r) => r.fixtureDir.includes('single-file-bug-fix'));
    if (bugFix === undefined) throw new Error('expected the bug-fix run');
    expect(bugFix.config.measurementPath).toBe('rawRuntime');
    expect(bugFix.pairIdFor(1)).toBeNull();
  });

  it('refuses when the checkpoint on disk is not the cataloged artifact', async () => {
    const artifact = { ...(fakeModelEntry()['artifact'] as Record<string, unknown>), sha256: 'sha256:' + '0'.repeat(64) };
    const catalog = loadFake([fakeModelEntry({ artifact })]);
    await expect(withPlumeEnv(async () => expandPreset(catalog, 'fake-paired', DEPS))).rejects.toThrow(
      /catalog pin mismatch/,
    );
  });

  it('refuses when the checkpoint quantization contradicts the catalog', async () => {
    const artifact = {
      ...(fakeModelEntry()['artifact'] as Record<string, unknown>),
      quantizationBits: 8,
      quantizationGroupSize: 32,
    };
    const catalog = loadFake([fakeModelEntry({ artifact })]);
    await expect(withPlumeEnv(async () => expandPreset(catalog, 'fake-paired', DEPS))).rejects.toThrow(
      /catalog quantization mismatch/,
    );
  });

  it('refuses plumePosture without the sidecar', () => {
    expect(() => expandPreset(loadFake(), 'fake-paired', { python: DEPS.python, resolveFolder: DEPS.resolveFolder })).toThrow(
      /sidecar is required/,
    );
  });

  it('refuses a context window that cannot fit the output reserve', async () => {
    const preset = fakePreset({ id: 'fake-tight', contextTokens: 4096 });
    // Plume cap is 4096 — equal to the window, leaving no room.
    const catalog = loadFake(undefined, [preset]);
    await expect(withPlumeEnv(async () => expandPreset(catalog, 'fake-tight', DEPS))).rejects.toThrow(
      /cannot fit the output reserve/,
    );
  });

  it('refuses an unknown preset id with the available ids', () => {
    expect(() => expandPreset(loadFake(), 'nope', DEPS)).toThrow(/unknown preset "nope"/);
  });
});

// ---- runMatrix end to end (fake runtime) -----------------------------------

describe('runMatrix', () => {
  it('runs warm and cold groups through the shared loop and stamps groupIds and pairIds', async () => {
    const outFile = path.join(dir, 'matrix-records.jsonl');
    writeFileSync(outFile, '');
    const config = fakeConfig('short-chat-pass');
    const written = await withPlumeEnv(() =>
      runMatrix(
        [
          {
            label: 'fake warm',
            config,
            groupId: 'grp_matrix_warm',
            fixtureDir: fixtureDir('short-chat', 'fact-001'),
            population: 'warm',
            repetitions: 3,
            pairIdFor: (repetition) => `pair_matrix_${repetition}`,
          },
          {
            label: 'fake cold',
            config,
            groupId: 'grp_matrix_cold',
            fixtureDir: fixtureDir('short-chat', 'fact-001'),
            population: 'cold',
            repetitions: 3,
            pairIdFor: () => null,
          },
        ],
        outFile,
        () => {},
      ),
    );
    expect(written).toBe(6);
    const result = readRecords(readFileSync(outFile, 'utf8'));
    expect(result.lineErrors).toEqual([]);
    expect(result.records).toHaveLength(6);
    const warm = result.records.filter((r) => r.run.population === 'warm');
    expect(warm.map((r) => r.run.pairId)).toEqual(['pair_matrix_1', 'pair_matrix_2', 'pair_matrix_3']);
    const cold = result.records.filter((r) => r.run.population === 'cold');
    expect(cold.every((r) => r.run.pairId === null)).toBe(true);
    // Half-pairs (warm only ran one path here) are correctly invalid.
    const pairs = summarizePairs(result.records);
    expect(pairs.every((p) => !p.valid)).toBe(true);
  });
});
