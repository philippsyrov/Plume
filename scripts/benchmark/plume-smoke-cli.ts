// D129C: one-command PAIRED smoke matrix (wrapped by
// scripts/benchmark-plume-smoke.sh) — the same verified checkpoint
// measured on both paths so the summarizer can derive Plume's real
// orchestration overhead:
//
//   rawRuntime          harness → mlx_lm.server directly
//   plumeOrchestration  harness → plume_bench (Plume's real modules)
//                                → the same mlx_lm.server config
//
// Each repetition shares a pairId across the two paths; both configs
// declare Plume's actual generation posture (no client sampling
// controls, the product's explicit max_tokens cap — read live from
// the sidecar's health handshake) so the pairs are overhead-valid per
// docs/MODEL_BENCHMARKS.md. Mechanics validation only: records land
// in gitignored benchmark-artifacts/ and are never a claim.

import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';

import { digestModelDir, probeMlxLmVersion } from './model-identity.ts';
import { runOne, runPriming } from './run-model.ts';
import { resolveRuntime } from './runtime-factory.ts';
import type { HarnessConfig } from './runtime-factory.ts';
import { fail as failWith, readQuantization, resolveModelDir, resolvePython, REPO_ROOT } from './smoke-support.ts';
import { readRecords, renderMarkdown } from './summarize-lib.ts';

const OUT_DIR = path.join(REPO_ROOT, 'benchmark-artifacts', 'plume-smoke');
const FIXTURE = path.join(REPO_ROOT, 'benchmarks', 'fixtures', 'short-chat', 'pong-001');
const PREFIX = 'benchmark-plume-smoke';
const REPS = 3;

function fail(message: string): never {
  failWith(PREFIX, message);
}

/// The built sidecar: PLUME_BENCH_BIN → the workspace debug target.
/// The shell wrapper builds it; a missing binary refuses.
function resolveSidecar(): string {
  const candidate =
    process.env['PLUME_BENCH_BIN'] ?? path.join(REPO_ROOT, 'src-tauri', 'target', 'debug', 'plume_bench');
  if (!existsSync(candidate)) {
    fail(
      `plume_bench sidecar not found at ${candidate} — build it: ` +
        './scripts/dev-env.sh cargo build --manifest-path src-tauri/Cargo.toml --bin plume_bench',
    );
  }
  return candidate;
}

/// Plume's real output cap, from the sidecar's health handshake (the
/// factory re-verifies this; reading it here just builds a config
/// that will pass).
function sidecarCap(binary: string, modelDir: string): number {
  const out = execFileSync(binary, ['orchestrate', '--health', '--port', '1', '--model', modelDir], {
    encoding: 'utf8',
    timeout: 15_000,
  });
  const parsed = JSON.parse(out.trim()) as { ok?: unknown; maxOutputTokens?: unknown };
  if (parsed.ok !== true || typeof parsed.maxOutputTokens !== 'number') {
    fail(`sidecar health reply malformed: ${out.trim()}`);
  }
  return parsed.maxOutputTokens;
}

function buildConfig(
  measurementPath: 'rawRuntime' | 'plumeOrchestration',
  python: string,
  modelDir: string,
  digest: string,
  cap: number,
  sidecar: string,
): HarnessConfig {
  const quant = readQuantization(modelDir);
  return {
    measurementPath,
    ...(measurementPath === 'plumeOrchestration' ? { plumeBench: { binary: sidecar } } : {}),
    runtime: {
      path: 'plume-mlx-lm',
      name: 'mlx-lm',
      version: null,
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
      // Pair-parity posture on BOTH paths: no client sampling
      // controls (Plume sends none; the raw client omits null
      // fields), the product's own explicit output cap on the wire
      // for both, and a context window that fits prompt + cap.
      context: { pointTokens: 8192, configuredTokens: 8192, acceptedTokens: null, maxOutputTokens: cap },
      sampling: {
        temperature: null,
        topP: null,
        topK: null,
        minP: null,
        repeatPenalty: null,
        seed: null,
        maxOutputTokens: cap,
        stopSequences: [],
      },
    },
  };
}

interface PathRun {
  label: string;
  config: HarnessConfig;
  groupPrefix: string;
}

async function main(): Promise<void> {
  const python = resolvePython(PREFIX);
  const modelDir = resolveModelDir(PREFIX);
  const sidecar = resolveSidecar();
  const version = probeMlxLmVersion(python);
  const cap = sidecarCap(sidecar, modelDir);
  process.stderr.write(`interpreter: ${python} (mlx-lm ${version ?? 'unknown'})\n`);
  process.stderr.write(`checkpoint:  ${modelDir}\n`);
  process.stderr.write(`sidecar:     ${sidecar} (max_tokens ${cap})\n`);
  process.stderr.write('digesting checkpoint (verified identity)…\n');
  const digest = digestModelDir(modelDir);

  mkdirSync(OUT_DIR, { recursive: true });
  const outFile = path.join(OUT_DIR, 'records.jsonl');
  writeFileSync(outFile, ''); // fresh matrix per run

  const paths: PathRun[] = [
    {
      label: 'rawRuntime',
      config: buildConfig('rawRuntime', python, modelDir, digest, cap, sidecar),
      groupPrefix: 'grp_plume_smoke_raw',
    },
    {
      label: 'plumeOrchestration',
      config: buildConfig('plumeOrchestration', python, modelDir, digest, cap, sidecar),
      groupPrefix: 'grp_plume_smoke_plume',
    },
  ];
  for (const run of paths) {
    const configPath = path.join(OUT_DIR, `config-${run.label}.json`);
    writeFileSync(configPath, JSON.stringify(run.config, null, 2));
  }

  // Warm: one primed session per path, REPS measured requests each,
  // repetition i on both paths sharing pair_warm_i.
  for (const run of paths) {
    process.stderr.write(`warm ${run.label} (1 primed session, ${REPS} reps)…\n`);
    const session = await (await resolveRuntime(run.config)).createSession();
    try {
      await runPriming(session, FIXTURE);
      for (let repetition = 1; repetition <= REPS; repetition += 1) {
        await runOne({
          config: run.config,
          fixtureDir: FIXTURE,
          population: 'warm',
          repetition,
          plannedRepetitions: REPS,
          outFile,
          groupId: `${run.groupPrefix}_warm`,
          pairId: `pair_warm_${repetition}`,
          session,
        });
      }
    } finally {
      await session.close();
    }
  }

  // Cold: fresh processes per attempt on both paths, paired by rep.
  for (const run of paths) {
    process.stderr.write(`cold ${run.label} (fresh spawn per attempt, ${REPS} reps)…\n`);
    for (let repetition = 1; repetition <= REPS; repetition += 1) {
      await runOne({
        config: run.config,
        fixtureDir: FIXTURE,
        population: 'cold',
        repetition,
        plannedRepetitions: REPS,
        outFile,
        groupId: `${run.groupPrefix}_cold`,
        pairId: `pair_cold_${repetition}`,
      });
    }
  }

  const result = readRecords(readFileSync(outFile, 'utf8'));
  if (result.lineErrors.length > 0) {
    fail(`recorded lines failed reader validation:\n${result.lineErrors.join('\n')}`);
  }
  process.stderr.write(`recorded ${result.records.length} attempts → ${outFile}\n\n`);
  process.stdout.write(renderMarkdown(result.records));
}

main().catch((err: unknown) => {
  fail(err instanceof Error ? err.message : String(err));
});
