// D129: one benchmark invocation → one schema-v1 JSONL record. This
// is the engine behind scripts/benchmark-model.sh. It drives exactly
// one runtime invocation on the `rawRuntime` measurement path, judges
// it with the fixture's oracle, assembles the record, and refuses to
// write anything that fails producer validation — an invalid record
// is a harness bug, never data.
//
// D129A: sessions come from `resolveRuntime` (runtime-factory.ts),
// which verifies real-runtime identity before anything runs and
// decides the timing method the records carry.
//
// D129C: `plumeOrchestration` is a real measurement path — the
// verified server plus the `plume_bench orchestrate` sidecar built
// from Plume's own modules; diff mechanics can run through Plume's
// real Rust patch validator (`plume_bench patch-check`).
//
// D129B: real-transport runs sample machine resource probes
// (resource-probes.ts) around exactly the measured request; probe
// failures record null and never fail or delay the run.

import { appendFileSync, mkdirSync, readFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { randomUUID } from 'node:crypto';

import { loadFixture } from './fixtures.ts';
import type { LoadedFixture } from './fixtures.ts';
import {
  judgeCodeExplanation,
  judgeLongContextRetrieval,
  judgeMultiFileNavigation,
  judgeShortChat,
  judgeSingleFileBugFix,
  judgeToolCallingAgentLoop,
} from './oracles.ts';
import type { DiffMechanicsOptions, OracleVerdict } from './oracles.ts';
import { plumeIdentity, verifySidecarIdentity } from './model-identity.ts';
import { NULL_READINGS, startResourceSampler } from './resource-probes.ts';
import type { ResourceReadings, ResourceSampler } from './resource-probes.ts';
import { resolveRuntime } from './runtime-factory.ts';
import type { BenchmarkRuntime, HarnessConfig, ResolvedRuntime } from './runtime-factory.ts';
import type { InvocationResult } from './runtime-client.ts';
import { serializeRecord, validateRecord } from './validate.ts';
import type { BenchmarkRecord, CancellationRestartEvidence } from './types.ts';

export type { HarnessConfig, HarnessRuntimeConfig } from './runtime-factory.ts';

export interface RunOneOptions {
  config: HarnessConfig;
  fixtureDir: string;
  population: 'cold' | 'warm';
  repetition: number;
  plannedRepetitions: number;
  outFile: string;
  runId?: string;
  groupId?: string;
  pairId?: string | null;
  timestampUtc?: string;
  /// An already-primed live session (suite runner, warm groups). When
  /// absent, runOne owns the process lifecycle itself: cold = fresh
  /// spawn per invocation; warm = fresh spawn + one unrecorded priming
  /// request in the SAME process before the measured request.
  session?: BenchmarkRuntime;
  /// Test seam: replaces startResourceSampler for the measured
  /// request, bypassing the transport gate. Production callers never
  /// set this — real runtimes get the real sampler, the fake runtime
  /// gets none (deterministic records).
  samplerFactory?: () => Promise<ResourceSampler>;
}

export function loadHarnessConfig(configPath: string): HarnessConfig {
  const parsed: unknown = JSON.parse(readFileSync(configPath, 'utf8'));
  const config = parsed as HarnessConfig;
  if (config.measurementPath !== 'rawRuntime' && config.measurementPath !== 'plumeOrchestration') {
    throw new Error(
      `config ${configPath}: measurementPath must be "rawRuntime" or "plumeOrchestration" (D129C)`,
    );
  }
  if (config.measurementPath === 'plumeOrchestration') {
    // The plume path needs the plume_bench sidecar (Plume's real
    // modules) and only works over the managed-server transport.
    if (typeof config.plumeBench?.binary !== 'string' || config.plumeBench.binary.length === 0) {
      throw new Error(
        `config ${configPath}: plumeOrchestration requires plumeBench.binary — the harness refuses ` +
          'to fake the Plume path without the real sidecar',
      );
    }
    if (config.runtime?.transport !== 'openai-sse') {
      throw new Error(
        `config ${configPath}: plumeOrchestration requires runtime.transport "openai-sse" ` +
          '(Plume talks to a managed mlx_lm.server)',
      );
    }
  }
  if (typeof config.runtime?.transport !== 'string') {
    throw new Error(`config ${configPath}: runtime.transport is required`);
  }
  return config;
}


/// Host manifest from portable Node APIs. Fields the API cannot answer
/// are null — never inferred (docs/MODEL_BENCHMARKS.md § Field rules).
function hostManifest(): BenchmarkRecord['host'] {
  return {
    machine: null,
    appleChip: null,
    unifiedMemoryBytes: os.totalmem(),
    cpuCoreCount: os.cpus().length,
    gpuCoreCount: null,
    os: `${os.type()} ${os.release()}`,
    osBuild: null,
    powerMode: null,
    powerSource: null,
    thermalStart: null,
  };
}

function assemblePrompt(fixture: LoadedFixture): string {
  const manifest = fixture.manifest;
  if (manifest.suiteId === 'long-context-retrieval' && manifest.paddingFile !== undefined) {
    const padding = readFileSync(path.join(fixture.dir, manifest.paddingFile), 'utf8');
    return `${padding}\n\n${manifest.prompt}`;
  }
  return manifest.prompt;
}

function judge(
  fixture: LoadedFixture,
  invocation: InvocationResult,
  record: BenchmarkRecord,
  mechanics?: DiffMechanicsOptions,
): OracleVerdict {
  switch (fixture.manifest.suiteId) {
    case 'short-chat':
      return judgeShortChat(fixture.manifest, invocation);
    case 'long-context-retrieval':
      return judgeLongContextRetrieval(fixture.manifest, invocation, {
        requested: record.model.context.configuredTokens,
        accepted: record.model.context.acceptedTokens,
        finalAssembledPromptTokens: record.tokens.finalAssembledPromptTokens,
        truncated: invocation.report?.truncated ?? null,
      });
    case 'code-explanation':
      return judgeCodeExplanation(fixture.manifest, invocation);
    case 'single-file-bug-fix':
      return judgeSingleFileBugFix(fixture.dir, fixture.manifest, invocation, mechanics);
    case 'multi-file-navigation':
      return judgeMultiFileNavigation(fixture.dir, fixture.manifest, invocation, mechanics);
    case 'tool-calling-agent-loop':
      return judgeToolCallingAgentLoop(fixture.dir, fixture.manifest, invocation, mechanics);
    case 'cancellation-restart':
      throw new Error('cancellation-restart is judged inline by runOne');
  }
}

function rate(count: number | null | undefined, durationMs: number | null | undefined): number | null {
  if (typeof count !== 'number' || typeof durationMs !== 'number' || durationMs <= 0) return null;
  return count / (durationMs / 1000);
}

/// Run one invocation and append its record to `outFile`. Returns the
/// record.
export async function runOne(options: RunOneOptions): Promise<BenchmarkRecord> {
  const fixture = loadFixture(options.fixtureDir);
  const manifest = fixture.manifest;
  const config = options.config;
  const resolved = await resolveRuntime(config);
  const prompt = assemblePrompt(fixture);
  // D129C: a configured plume_bench routes diff mechanics through
  // Plume's real Rust patch modules (any measurement path may opt in;
  // plumeOrchestration requires it via loadHarnessConfig).
  const mechanics: DiffMechanicsOptions | undefined =
    config.plumeBench !== undefined ? { patchCheck: [config.plumeBench.binary, 'patch-check'] } : undefined;
  // Provenance: any run whose diff mechanics (or orchestration) go
  // through plume_bench verifies the binary's embedded build identity
  // against the Plume identity this record will carry — a stale or
  // foreign sidecar refuses before anything is measured. (The factory
  // repeats this per launch for the orchestration path.)
  if (config.plumeBench !== undefined) verifySidecarIdentity(config.plumeBench.binary);

  const isCancellation = manifest.suiteId === 'cancellation-restart';
  const invokeOptions = {
    prompt,
    timeoutMs: manifest.timeoutMs,
    ...(isCancellation && manifest.cancelAfterTokens !== undefined
      ? { cancelAfterTokens: manifest.cancelAfterTokens }
      : {}),
  };

  // D129B: resource probes wrap exactly the MEASURED request — not
  // priming, not session/model load — matching the contract's "request
  // start through terminal completion" window. Start probes finish
  // before the request is sent; end probes run after the terminal
  // event; a broken sampler records nulls, never fails the run.
  const samplerFactory =
    options.samplerFactory ?? (resolved.supportsResourceProbes ? () => startResourceSampler() : null);
  let readings: ResourceReadings = NULL_READINGS;
  const measuredInvoke = async (session: BenchmarkRuntime): Promise<InvocationResult> => {
    let sampler: ResourceSampler | null = null;
    if (samplerFactory !== null) {
      try {
        sampler = await samplerFactory();
      } catch (err) {
        console.error('resource sampler failed to start (recording nulls):', err instanceof Error ? err.message : String(err));
      }
    }
    try {
      return await session.invoke(invokeOptions);
    } finally {
      if (sampler !== null) {
        try {
          readings = await sampler.stop();
        } catch (err) {
          console.error('resource sampler failed to stop (recording nulls):', err instanceof Error ? err.message : String(err));
        }
      }
    }
  };

  // Population honesty: a warm measurement may only run in a process
  // that is already loaded and primed. With an external session the
  // suite runner primed it; otherwise this invocation owns a session
  // and primes it itself. Cold is a fresh spawn (processRestart).
  let invocation: InvocationResult;
  if (options.session !== undefined) {
    invocation = await measuredInvoke(options.session);
  } else {
    const session = await resolved.createSession();
    try {
      if (options.population === 'warm') {
        await session.invoke({ prompt, timeoutMs: manifest.timeoutMs }); // unrecorded priming
      }
      invocation = await measuredInvoke(session);
    } finally {
      await session.close();
    }
  }

  const report = invocation.report;
  const tokenCounts =
    report !== null && typeof report.promptTokens === 'number' && typeof report.outputTokens === 'number'
      ? { prompt: report.promptTokens, output: report.outputTokens }
      : null;

  const record: BenchmarkRecord = {
    schemaVersion: 1,
    run: {
      id: options.runId ?? `bench_${randomUUID()}`,
      groupId: options.groupId ?? `grp_${randomUUID()}`,
      pairId: options.pairId ?? null,
      timestampUtc: options.timestampUtc ?? new Date().toISOString(),
      population: options.population,
      coldMethod: options.population === 'cold' ? 'processRestart' : null,
      repetition: options.repetition,
      plannedRepetitions: options.plannedRepetitions,
      measurementPath: config.measurementPath,
    },
    plume: plumeIdentity(),
    host: { ...hostManifest(), thermalStart: readings.thermalStart },
    runtime: resolved.block,
    model: {
      ...config.model,
      context: { ...config.model.context, acceptedTokens: report?.acceptedContextTokens ?? null },
    },
    suite: {
      id: manifest.suiteId,
      caseId: manifest.caseId,
      fixtureRevision: manifest.fixtureRevision,
      fixtureDigest: fixture.manifestDigest,
    },
    // Placeholder; replaced by the oracle below.
    suiteEvidence: { kind: 'short-chat', replyClassification: null, terminalStreamOutcome: null },
    tokens: {
      tokenizer: { identity: null, revision: null, digest: null, chatTemplate: null, chatTemplateDigest: null },
      countSource: tokenCounts !== null ? 'runtimeReported' : 'unavailable',
      finalAssembledPromptTokens: tokenCounts !== null ? tokenCounts.prompt : null,
      promptBytes: Buffer.byteLength(prompt, 'utf8'),
      outputTokens: tokenCounts !== null ? tokenCounts.output : null,
    },
    timing:
      report !== null
        ? {
            method: resolved.timingMethod,
            timeToFirstTokenMs: report.ttftMs ?? null,
            promptEvaluationMs: report.promptEvaluationMs ?? null,
            generationDurationMs: report.generationDurationMs ?? null,
            promptTokensPerSecond: tokenCounts !== null ? rate(tokenCounts.prompt, report.promptEvaluationMs) : null,
            generationTokensPerSecond: tokenCounts !== null ? rate(tokenCounts.output, report.generationDurationMs) : null,
            endToEndMs: report.endToEndMs ?? null,
          }
        : {
            method: 'unavailable',
            timeToFirstTokenMs: null,
            promptEvaluationMs: null,
            generationDurationMs: null,
            promptTokensPerSecond: null,
            generationTokensPerSecond: null,
            endToEndMs: null,
          },
    resources: {
      peakUnifiedMemoryBytes: readings.peakUnifiedMemoryBytes,
      swapDeltaBytes: readings.swapDeltaBytes,
      thermalEnd: readings.thermalEnd,
      wallEnergyJoules: readings.wallEnergyJoules,
    },
    includeInSummary: true,
    exclusionReason: null,
    outcome: {
      status: 'failed',
      toolCallValid: null,
      correctFileDiscovery: null,
      validDiff: null,
      patchApplySuccess: null,
      verificationSuccess: null,
      finalTaskSuccess: null,
      stream: invocation.terminal,
      timeout: invocation.terminal === 'timedOut',
      timeoutLimitMs: manifest.timeoutMs,
      cancellationLatencyMs: null,
      crash: invocation.terminal === 'crashed',
      restartRecovery: null,
      errorClass: null,
    },
    artifacts: [],
  };

  if (isCancellation) {
    await judgeCancellationRestart(record, invocation, resolved, manifest.timeoutMs);
  } else {
    const verdict = judge(fixture, invocation, record, mechanics);
    record.suiteEvidence = verdict.evidence;
    Object.assign(record.outcome, verdict.outcome);
    record.outcome.finalTaskSuccess = verdict.passed;
    record.outcome.status = statusFor(invocation, verdict.passed);
    record.outcome.errorClass = errorClassFor(invocation);
  }

  const validation = validateRecord(record, 'producer');
  if (!validation.ok) {
    throw new Error(`harness bug: assembled record fails producer validation:\n${validation.errors.join('\n')}`);
  }
  mkdirSync(path.dirname(path.resolve(options.outFile)), { recursive: true });
  appendFileSync(options.outFile, serializeRecord(record) + '\n', 'utf8');
  return record;
}

function statusFor(invocation: InvocationResult, passed: boolean): BenchmarkRecord['outcome']['status'] {
  switch (invocation.terminal) {
    case 'completed':
      return passed ? 'passed' : 'failed';
    case 'timedOut':
      return 'timedOut';
    case 'cancelled':
      return 'cancelled';
    case 'malformed':
    case 'crashed':
      return 'error';
  }
}

function errorClassFor(invocation: InvocationResult): string | null {
  switch (invocation.terminal) {
    case 'malformed':
      return 'malformed-stream';
    case 'crashed':
      return 'runtime-crash';
    default:
      return null;
  }
}

/// The cancellation-restart suite judges two behaviors: a deliberate
/// cancel (acknowledged in time = pass, excluded from latency
/// summaries) and a crash followed by restart + health + follow-up
/// (all three = recovery = pass). Restart mechanics are
/// transport-specific and live in the resolved runtime.
async function judgeCancellationRestart(
  record: BenchmarkRecord,
  invocation: InvocationResult,
  resolved: ResolvedRuntime,
  timeoutMs: number,
): Promise<void> {
  // Harness-measured (monotonic, cancel-send → terminal event or
  // conclusive close). Runtime-reported numbers are never read.
  const latency = invocation.terminal === 'cancelled' ? invocation.cancellationLatencyMs : null;
  let restartHealthy: boolean | null = null;
  let followUpPassed: boolean | null = null;
  let restartRecovery: boolean | null = null;

  if (invocation.terminal === 'crashed') {
    const recovery = await resolved.crashRestart(timeoutMs);
    restartHealthy = recovery.healthy;
    followUpPassed = recovery.followUpPassed;
    restartRecovery = restartHealthy && followUpPassed;
  }

  const passed = invocation.terminal === 'cancelled' ? latency !== null : restartRecovery === true;

  const evidence: CancellationRestartEvidence = {
    kind: 'cancellation-restart',
    cancellationLatencyMs: latency,
    terminalStreamOutcome: invocation.terminal,
    runtimeCrashed: invocation.terminal === 'crashed',
    restartHealthy,
    followUpPassed,
  };
  record.suiteEvidence = evidence;
  record.outcome.cancellationLatencyMs = latency;
  record.outcome.restartRecovery = restartRecovery;
  record.outcome.finalTaskSuccess = passed;
  record.outcome.errorClass = errorClassFor(invocation);
  if (invocation.terminal === 'cancelled') {
    record.outcome.status = passed ? 'passed' : 'failed';
    record.includeInSummary = false;
    record.exclusionReason = 'deliberate-cancellation';
  } else if (invocation.terminal === 'crashed') {
    record.outcome.status = passed ? 'passed' : 'error';
  } else {
    record.outcome.status = statusFor(invocation, false);
  }
}

/// Warm priming: one unrecorded invocation with the same
/// configuration, in the SAME session the measured requests will use.
export async function runPriming(session: BenchmarkRuntime, fixtureDir: string): Promise<void> {
  const fixture = loadFixture(fixtureDir);
  await session.invoke({
    prompt: assemblePrompt(fixture),
    timeoutMs: fixture.manifest.timeoutMs,
  });
}
