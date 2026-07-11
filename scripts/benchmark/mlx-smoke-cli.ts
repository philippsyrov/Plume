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

import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';

import { digestModelDirCached, probeMlxLmVersion } from './model-identity.ts';
import { runPlan } from './run-suite.ts';
import { readRecords, renderMarkdown } from './summarize-lib.ts';
import type { HarnessConfig } from './runtime-factory.ts';

const REPO_ROOT = path.resolve(__dirname, '..', '..');
const OUT_DIR = path.join(REPO_ROOT, 'benchmark-artifacts', 'mlx-smoke');
const FIXTURE = path.join(REPO_ROOT, 'benchmarks', 'fixtures', 'short-chat', 'pong-001');

function fail(message: string): never {
  process.stderr.write(`benchmark-mlx-smoke: ${message}\n`);
  process.exit(1);
}

/// Interpreter resolution, mirroring scripts/smoke-qwen-mlx.sh:
/// PLUME_MLX_PYTHON → ~/.venvs/mlx-env/bin/python → python3.
function resolvePython(): string {
  const candidates = [
    process.env['PLUME_MLX_PYTHON'],
    path.join(os.homedir(), '.venvs', 'mlx-env', 'bin', 'python'),
    'python3',
  ].filter((c): c is string => c !== undefined && c.length > 0);
  for (const candidate of candidates) {
    if (probeMlxLmVersion(candidate) !== null) return candidate;
  }
  fail(
    'no interpreter with an importable mlx_lm found ' +
      '(tried PLUME_MLX_PYTHON, ~/.venvs/mlx-env/bin/python, python3). Nothing was installed.',
  );
}

/// Model discovery, mirroring the smoke scripts: PLUME_MODEL_DIR →
/// <repo>/plume-models → ~/plume-models, preferring the documented
/// Qwen2.5-Coder 3B 4-bit target, else any folder with a config.json
/// and a .safetensors file.
function resolveModelDir(): string {
  // When running from a git worktree under <checkout>/.claude/worktrees/,
  // the model dir convention lives in the primary checkout.
  const worktreeMarker = `${path.sep}.claude${path.sep}worktrees${path.sep}`;
  const markerIndex = REPO_ROOT.indexOf(worktreeMarker);
  const primaryCheckout = markerIndex === -1 ? null : REPO_ROOT.slice(0, markerIndex);
  const roots = [
    process.env['PLUME_MODEL_DIR'],
    path.join(REPO_ROOT, 'plume-models'),
    primaryCheckout === null ? undefined : path.join(primaryCheckout, 'plume-models'),
    path.join(os.homedir(), 'plume-models'),
  ].filter((r): r is string => r !== undefined && r.length > 0);
  const preferred = 'Qwen2.5-Coder-3B-Instruct-4bit';
  for (const root of roots) {
    const candidate = path.join(root, preferred);
    if (existsSync(path.join(candidate, 'config.json'))) return candidate;
  }
  for (const root of roots) {
    if (!existsSync(root)) continue;
    for (const entry of readdirSync(root)) {
      const candidate = path.join(root, entry);
      if (
        existsSync(path.join(candidate, 'config.json')) &&
        existsSync(path.join(candidate, 'model.safetensors'))
      ) {
        return candidate;
      }
    }
  }
  fail('no MLX checkpoint found (tried PLUME_MODEL_DIR, <repo>/plume-models, ~/plume-models). Nothing was downloaded.');
}

interface CheckpointQuantization {
  method: string | null;
  bits: number | null;
  groupSize: number | null;
}

/// Quantization identity read from the checkpoint's own config.json —
/// recorded only when the file actually states it.
function readQuantization(modelDir: string): CheckpointQuantization {
  try {
    const config = JSON.parse(readFileSync(path.join(modelDir, 'config.json'), 'utf8')) as {
      quantization?: { bits?: number; group_size?: number };
    };
    const q = config.quantization;
    if (q === undefined) return { method: null, bits: null, groupSize: null };
    return {
      method: 'affine',
      bits: typeof q.bits === 'number' ? q.bits : null,
      groupSize: typeof q.group_size === 'number' ? q.group_size : null,
    };
  } catch {
    return { method: null, bits: null, groupSize: null };
  }
}

async function main(): Promise<void> {
  const python = resolvePython();
  const modelDir = resolveModelDir();
  const version = probeMlxLmVersion(python);
  process.stderr.write(`interpreter: ${python} (mlx-lm ${version ?? 'unknown'})\n`);
  process.stderr.write(`checkpoint:  ${modelDir}\n`);
  process.stderr.write('digesting checkpoint (verified identity)…\n');
  const digest = digestModelDirCached(modelDir);
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
