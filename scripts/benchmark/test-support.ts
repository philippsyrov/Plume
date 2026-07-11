// D129: shared helpers for harness tests — a fake-runtime harness
// config whose artifact digest is the real sha256 of the selected
// case script, plus deterministic env for plume identity.

import { readFileSync } from 'node:fs';
import path from 'node:path';

import { sha256Hex } from './fixtures.ts';
import type { HarnessConfig } from './run-model.ts';

export const REPO_ROOT = path.resolve(__dirname, '..', '..');

export function fixtureDir(suite: string, caseId: string): string {
  return path.join(REPO_ROOT, 'benchmarks', 'fixtures', suite, caseId);
}

export function casePath(name: string): string {
  return path.join(REPO_ROOT, 'benchmarks', 'fake-runtime', 'cases', `${name}.json`);
}

/// Harness config for one fake-runtime case. The "model" is the case
/// script itself: its digest is the artifact identity, so two configs
/// built from the same case compare strict-artifact.
export function fakeConfig(caseName: string): HarnessConfig {
  const caseFile = casePath(caseName);
  const digest = `sha256:${sha256Hex(readFileSync(caseFile))}`;
  return {
    measurementPath: 'rawRuntime',
    runtime: {
      path: 'fake-runtime',
      name: 'plume-fake-runtime',
      version: '1',
      engine: 'plume-fake-runtime',
      backend: 'scripted',
      transport: 'stdio-jsonl',
      command: [process.execPath, path.join(REPO_ROOT, 'benchmarks', 'fake-runtime', 'fake-runtime.mjs'), '--case', caseFile],
      configuration: {
        digest,
        mtp: null,
        speculativeDecoding: null,
        promptCache: null,
        kvCacheQuantization: null,
        contextTokens: 4096,
        batchSize: null,
        threads: null,
        gpuLayers: null,
      },
    },
    model: {
      sourceId: 'plume/fake-model',
      sourceRevision: `scripted-${caseName}`,
      artifact: {
        format: 'scripted',
        sha256: digest,
        quantizationMethod: null,
        quantizationBits: null,
        quantizationGroupSize: null,
        conversionProvenance: null,
        conversionConfigDigest: null,
      },
      comparisonParity: 'strictArtifact',
      context: { pointTokens: 4096, configuredTokens: 4096, acceptedTokens: null, maxOutputTokens: 512 },
      sampling: {
        temperature: 0.0,
        topP: 1.0,
        topK: null,
        minP: null,
        repeatPenalty: 1.0,
        seed: 42,
        maxOutputTokens: 512,
        stopSequences: [],
      },
    },
  };
}

/// Deterministic plume identity for tests (no dependence on the
/// working tree's dirtiness while developing).
export function withPlumeEnv<T>(fn: () => Promise<T>): Promise<T> {
  process.env['PLUME_BENCH_GIT_SHA'] = '0123456789abcdef0123456789abcdef01234567';
  process.env['PLUME_BENCH_DIRTY'] = 'false';
  return fn().finally(() => {
    delete process.env['PLUME_BENCH_GIT_SHA'];
    delete process.env['PLUME_BENCH_DIRTY'];
  });
}
