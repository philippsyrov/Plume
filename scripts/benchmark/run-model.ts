// D129: one benchmark invocation → one schema-v1 JSONL record. This
// is the engine behind scripts/benchmark-model.sh. It drives exactly
// one runtime invocation on the `rawRuntime` measurement path, judges
// it with the fixture's oracle, assembles the record, and refuses to
// write anything that fails producer validation — an invalid record
// is a harness bug, never data.
//
// `plumeOrchestration` is rejected: measuring Plume's own path means
// driving the real app, which no fake-runtime slice can honestly do.

import { execFileSync } from 'node:child_process';
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
import type { OracleVerdict } from './oracles.ts';
import { probeHealth, runInvocation, RuntimeSession } from './runtime-client.ts';
import type { InvocationResult } from './runtime-client.ts';
import { serializeRecord, validateRecord } from './validate.ts';
import type {
  BenchmarkRecord,
  CancellationRestartEvidence,
  ModelBlock,
  RuntimeConfigurationBlock,
} from './types.ts';

export interface HarnessRuntimeConfig {
  path: string;
  name: string;
  version: string | null;
  engine: string;
  backend: string;
  transport: string;
  command: string[];
  configuration: RuntimeConfigurationBlock;
}

export interface HarnessConfig {
  measurementPath: 'rawRuntime';
  runtime: HarnessRuntimeConfig;
  model: ModelBlock;
}

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
  session?: RuntimeSession;
}

export function loadHarnessConfig(configPath: string): HarnessConfig {
  const parsed: unknown = JSON.parse(readFileSync(configPath, 'utf8'));
  const config = parsed as HarnessConfig;
  if (config.measurementPath !== 'rawRuntime') {
    throw new Error(
      `config ${configPath}: measurementPath must be "rawRuntime" — the plumeOrchestration path ` +
        'requires driving the real app and is not implemented in the D129 harness',
    );
  }
  if (!Array.isArray(config.runtime?.command) || config.runtime.command.length === 0) {
    throw new Error(`config ${configPath}: runtime.command must be a non-empty array`);
  }
  return config;
}

function gitValue(args: string[]): string {
  return execFileSync('git', args, { encoding: 'utf8' }).trim();
}

function plumeIdentity(): { gitSha: string; dirty: boolean } {
  const envSha = process.env['PLUME_BENCH_GIT_SHA'];
  const envDirty = process.env['PLUME_BENCH_DIRTY'];
  if (envSha !== undefined && envDirty !== undefined) {
    return { gitSha: envSha, dirty: envDirty === 'true' };
  }
  return {
    gitSha: gitValue(['rev-parse', 'HEAD']),
    dirty: gitValue(['status', '--porcelain']).length > 0,
  };
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

function judge(fixture: LoadedFixture, invocation: InvocationResult, record: BenchmarkRecord): OracleVerdict {
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
      return judgeSingleFileBugFix(fixture.dir, fixture.manifest, invocation);
    case 'multi-file-navigation':
      return judgeMultiFileNavigation(fixture.dir, fixture.manifest, invocation);
    case 'tool-calling-agent-loop':
      return judgeToolCallingAgentLoop(fixture.dir, fixture.manifest, invocation);
    case 'cancellation-restart':
      throw new Error('cancellation-restart is judged inline by runOne');
  }
}

function rate(count: number | null | undefined, durationMs: number | null | undefined): number | null {
  if (typeof count !== 'number' || typeof durationMs !== 'number' || durationMs <= 0) return null;
  return count / (durationMs / 1000);
}

/// Run one invocation and append its record to `outFile`. Returns the
/// record. `recordOnly: false` callers (warm priming) use runPriming.
export async function runOne(options: RunOneOptions): Promise<BenchmarkRecord> {
  const fixture = loadFixture(options.fixtureDir);
  const manifest = fixture.manifest;
  const config = options.config;
  const prompt = assemblePrompt(fixture);

  const isCancellation = manifest.suiteId === 'cancellation-restart';
  const invokeOptions = {
    prompt,
    timeoutMs: manifest.timeoutMs,
    ...(isCancellation && manifest.cancelAfterTokens !== undefined
      ? { cancelAfterTokens: manifest.cancelAfterTokens }
      : {}),
  };

  // Population honesty: a warm measurement may only run in a process
  // that is already loaded and primed. With an external session the
  // suite runner primed it; otherwise this invocation owns a session
  // and primes it itself. Cold is a fresh spawn (processRestart).
  let invocation: InvocationResult;
  if (options.session !== undefined) {
    invocation = await options.session.invoke(invokeOptions);
  } else if (options.population === 'warm') {
    const session = new RuntimeSession(config.runtime.command);
    try {
      await session.invoke({ prompt, timeoutMs: manifest.timeoutMs }); // unrecorded priming
      invocation = await session.invoke(invokeOptions);
    } finally {
      session.close();
    }
  } else {
    invocation = await runInvocation(config.runtime.command, invokeOptions);
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
    host: hostManifest(),
    runtime: {
      path: config.runtime.path,
      name: config.runtime.name,
      version: config.runtime.version,
      engine: config.runtime.engine,
      backend: config.runtime.backend,
      configuration: config.runtime.configuration,
      transport: config.runtime.transport,
    },
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
            method: 'runtimeReported',
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
    resources: { peakUnifiedMemoryBytes: null, swapDeltaBytes: null, thermalEnd: null, wallEnergyJoules: null },
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
    await judgeCancellationRestart(record, invocation, config, manifest.timeoutMs);
  } else {
    const verdict = judge(fixture, invocation, record);
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

/// The cancellation-restart suite judges two scripted behaviors:
/// a deliberate cancel (acknowledged in time = pass, excluded from
/// latency summaries) and a crash followed by restart + health +
/// follow-up (all three = recovery = pass).
async function judgeCancellationRestart(
  record: BenchmarkRecord,
  invocation: InvocationResult,
  config: HarnessConfig,
  timeoutMs: number,
): Promise<void> {
  // Harness-measured (monotonic, cancel-send → terminal event or
  // conclusive close). Runtime-reported numbers are never read.
  const latency = invocation.terminal === 'cancelled' ? invocation.cancellationLatencyMs : null;
  let restartHealthy: boolean | null = null;
  let followUpPassed: boolean | null = null;
  let restartRecovery: boolean | null = null;

  if (invocation.terminal === 'crashed') {
    restartHealthy = await probeHealth(config.runtime.command, timeoutMs);
    if (restartHealthy) {
      const followUp = await runInvocation(
        config.runtime.command,
        { prompt: 'follow-up', timeoutMs },
        true,
      );
      followUpPassed = followUp.terminal === 'completed' && followUp.reply.length > 0;
    } else {
      followUpPassed = false;
    }
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
export async function runPriming(session: RuntimeSession, fixtureDir: string): Promise<void> {
  const fixture = loadFixture(fixtureDir);
  await session.invoke({
    prompt: assemblePrompt(fixture),
    timeoutMs: fixture.manifest.timeoutMs,
  });
}
