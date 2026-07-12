// D129A: one-command MLX smoke matrix (wrapped by
// scripts/benchmark-mlx-smoke.sh). Discovers the interpreter and a
// local MLX checkpoint the same way Plume's smoke scripts do, builds
// a VERIFIED config (real model-dir digest, probed mlx-lm version,
// quantization read from the checkpoint's own config.json), runs a
// tiny warm+cold matrix on the short-chat fixture, and summarizes.
//
// Mechanics validation only: results land in benchmark-artifacts/
// (gitignored), are never committed, and are not performance claims.
// Nothing is downloaded or installed; missing prerequisites refuse
// with a diagnostic.

import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';

import { digestModelDir, probeMlxLmVersion } from './model-identity.ts';
import { runPlan } from './run-suite.ts';
import { fail as failWith, readQuantization, resolveModelDir, resolvePython, REPO_ROOT } from './smoke-support.ts';
import { readRecords, renderMarkdown } from './summarize-lib.ts';
import type { HarnessConfig } from './runtime-factory.ts';

const OUT_DIR = path.join(REPO_ROOT, 'benchmark-artifacts', 'mlx-smoke');
const FIXTURE = path.join(REPO_ROOT, 'benchmarks', 'fixtures', 'short-chat', 'pong-001');
const PREFIX = 'benchmark-mlx-smoke';

function fail(message: string): never {
  failWith(PREFIX, message);
}

async function main(): Promise<void> {
  const python = resolvePython(PREFIX);
  const modelDir = resolveModelDir(PREFIX);
  const version = probeMlxLmVersion(python);
  process.stderr.write(`interpreter: ${python} (mlx-lm ${version ?? 'unknown'})\n`);
  process.stderr.write(`checkpoint:  ${modelDir}\n`);
  process.stderr.write('digesting checkpoint (verified identity)…\n');
  const digest = digestModelDir(modelDir);
  const quant = readQuantization(modelDir);

  const config: HarnessConfig = {
    measurementPath: 'rawRuntime',
    runtime: {
      path: 'plume-mlx-lm',
      name: 'mlx-lm',
      version: null, // filled by the factory from the probed import
      engine: 'mlx-lm',
      backend: 'MLX',
      transport: 'openai-sse',
      server: {
        command: [python, '-m', 'mlx_lm', 'server', '--model', modelDir],
        modelDir,
        startupTimeoutMs: 120_000,
      },
      configuration: {
        digest: null,
        mtp: null,
        speculativeDecoding: null,
        promptCache: null,
        kvCacheQuantization: null,
        contextTokens: null,
        batchSize: null,
        threads: null,
        gpuLayers: null,
      },
    },
    model: {
      sourceId: `local/${path.basename(modelDir)}`,
      // No upstream revision is verifiable for a local folder; the
      // digest IS the immutable identity, so it doubles as revision.
      sourceRevision: digest.slice(0, 71),
      artifact: {
        format: 'mlx',
        sha256: digest,
        quantizationMethod: quant.method,
        quantizationBits: quant.bits,
        quantizationGroupSize: quant.groupSize,
        conversionProvenance: null,
        conversionConfigDigest: null,
      },
      comparisonParity: 'strictArtifact',
      context: { pointTokens: 4096, configuredTokens: 4096, acceptedTokens: null, maxOutputTokens: 64 },
      sampling: {
        temperature: 0.0,
        topP: 1.0,
        topK: null,
        minP: null,
        repeatPenalty: null,
        seed: 42,
        maxOutputTokens: 64,
        stopSequences: [],
      },
    },
  };

  mkdirSync(OUT_DIR, { recursive: true });
  const configPath = path.join(OUT_DIR, 'config.json');
  writeFileSync(configPath, JSON.stringify(config, null, 2));
  const outFile = path.join(OUT_DIR, 'records.jsonl');
  writeFileSync(outFile, ''); // fresh matrix per run

  const plan = {
    config: configPath,
    outFile,
    groups: [
      { groupId: 'grp_mlx_smoke_warm', fixture: FIXTURE, population: 'warm' as const, repetitions: 3 },
      { groupId: 'grp_mlx_smoke_cold', fixture: FIXTURE, population: 'cold' as const, repetitions: 3 },
    ],
  };
  process.stderr.write('running warm (1 session, primed) + cold (fresh server per attempt) matrix…\n');
  const written = await runPlan(plan, config);
  process.stderr.write(`recorded ${written} attempts → ${outFile}\n\n`);

  const result = readRecords(readFileSync(outFile, 'utf8'));
  if (result.lineErrors.length > 0) {
    fail(`recorded lines failed reader validation:\n${result.lineErrors.join('\n')}`);
  }
  process.stdout.write(renderMarkdown(result.records));
}

main().catch((err: unknown) => {
  fail(err instanceof Error ? err.message : String(err));
});
