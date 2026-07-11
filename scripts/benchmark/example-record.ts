// D129: canonical valid schema-v1 records for tests. One fully valid
// record per suite kind, mirroring the shape example in
// docs/MODEL_BENCHMARKS.md — with every cross-field and contradiction
// invariant satisfied, so tests can break exactly one thing at a time.

import type {
  BenchmarkRecord,
  CancellationRestartEvidence,
  CodeExplanationEvidence,
  LongContextRetrievalEvidence,
  MultiFileNavigationEvidence,
  ShortChatEvidence,
  ToolCallingAgentLoopEvidence,
} from './types.ts';

const SHA256_EXAMPLE = `sha256:${'ab'.repeat(32)}`;

/// A valid `single-file-bug-fix` record (the doc's own example kind).
export function makeValidRecord(): BenchmarkRecord {
  return {
    schemaVersion: 1,
    run: {
      id: 'bench_01J00000000000000000000000',
      groupId: 'grp_01J00000000000000000000000',
      pairId: null,
      timestampUtc: '2026-07-11T12:00:00Z',
      population: 'warm',
      coldMethod: null,
      repetition: 1,
      plannedRepetitions: 5,
      measurementPath: 'rawRuntime',
    },
    plume: { gitSha: '0123456789abcdef0123456789abcdef01234567', dirty: false },
    host: {
      machine: 'Mac Studio (M3 Ultra)',
      appleChip: 'Apple M3 Ultra',
      unifiedMemoryBytes: 137438953472,
      cpuCoreCount: 32,
      gpuCoreCount: 80,
      os: 'macOS 26.0',
      osBuild: '25A000',
      powerMode: 'automatic',
      powerSource: 'ac',
      thermalStart: 'nominal',
    },
    runtime: {
      path: 'fake-runtime',
      name: 'plume-fake-runtime',
      version: '1',
      engine: 'plume-fake-runtime',
      backend: 'scripted',
      configuration: {
        digest: SHA256_EXAMPLE,
        mtp: null,
        speculativeDecoding: null,
        promptCache: null,
        kvCacheQuantization: null,
        contextTokens: 32768,
        batchSize: null,
        threads: null,
        gpuLayers: null,
      },
      transport: 'stdio-jsonl',
    },
    model: {
      sourceId: 'plume/fake-model',
      sourceRevision: 'scripted-v1',
      artifact: {
        format: 'scripted',
        sha256: SHA256_EXAMPLE,
        quantizationMethod: null,
        quantizationBits: null,
        quantizationGroupSize: null,
        conversionProvenance: null,
        conversionConfigDigest: null,
      },
      comparisonParity: 'strictArtifact',
      context: { pointTokens: 4096, configuredTokens: 4096, acceptedTokens: 4096, maxOutputTokens: 512 },
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
    suite: { id: 'single-file-bug-fix', caseId: 'bug-001', fixtureRevision: 'v1', fixtureDigest: SHA256_EXAMPLE },
    suiteEvidence: {
      kind: 'single-file-bug-fix',
      targetFile: 'src/example.ts',
      diffValid: true,
      applySucceeded: true,
      verifierSucceeded: true,
      rollbackSucceeded: true,
    },
    tokens: {
      tokenizer: { identity: null, revision: null, digest: null, chatTemplate: null, chatTemplateDigest: null },
      countSource: 'runtimeReported',
      finalAssembledPromptTokens: 1200,
      promptBytes: 4800,
      outputTokens: 84,
    },
    timing: {
      method: 'runtimeReported',
      timeToFirstTokenMs: 12.5,
      promptEvaluationMs: 10.0,
      generationDurationMs: 40.0,
      promptTokensPerSecond: 120000.0,
      generationTokensPerSecond: 2100.0,
      endToEndMs: 55.0,
    },
    resources: { peakUnifiedMemoryBytes: 1024, swapDeltaBytes: 0, thermalEnd: 'nominal', wallEnergyJoules: null },
    includeInSummary: true,
    exclusionReason: null,
    outcome: {
      status: 'passed',
      toolCallValid: null,
      correctFileDiscovery: null,
      validDiff: true,
      patchApplySuccess: true,
      verificationSuccess: true,
      finalTaskSuccess: true,
      stream: 'completed',
      timeout: false,
      timeoutLimitMs: 30000,
      cancellationLatencyMs: null,
      crash: false,
      restartRecovery: null,
      errorClass: null,
    },
    artifacts: [],
  };
}

/// Reset every suite-scoped outcome metric to null; each morph below
/// re-enables only what its suite exercises.
function clearSuiteMetrics(record: BenchmarkRecord): void {
  record.outcome.toolCallValid = null;
  record.outcome.correctFileDiscovery = null;
  record.outcome.validDiff = null;
  record.outcome.patchApplySuccess = null;
  record.outcome.verificationSuccess = null;
  record.outcome.cancellationLatencyMs = null;
  record.outcome.restartRecovery = null;
}

export function asShortChat(record: BenchmarkRecord): BenchmarkRecord {
  clearSuiteMetrics(record);
  record.suite.id = 'short-chat';
  record.suite.caseId = 'fact-001';
  const evidence: ShortChatEvidence = {
    kind: 'short-chat',
    replyClassification: 'exact-match',
    terminalStreamOutcome: 'completed',
  };
  record.suiteEvidence = evidence;
  return record;
}

export function asLongContextRetrieval(record: BenchmarkRecord): BenchmarkRecord {
  clearSuiteMetrics(record);
  record.suite.id = 'long-context-retrieval';
  record.suite.caseId = 'keys-001';
  const evidence: LongContextRetrievalEvidence = {
    kind: 'long-context-retrieval',
    requestedContextTokens: record.model.context.configuredTokens,
    acceptedContextTokens: record.model.context.acceptedTokens,
    finalAssembledPromptTokens: record.tokens.finalAssembledPromptTokens,
    retrievedKeys: ['alpha', 'bravo'],
    missingKeys: [],
    incorrectDecoyKeys: [],
    truncated: false,
  };
  record.suiteEvidence = evidence;
  return record;
}

export function asCodeExplanation(record: BenchmarkRecord): BenchmarkRecord {
  clearSuiteMetrics(record);
  record.suite.id = 'code-explanation';
  record.suite.caseId = 'explain-001';
  const evidence: CodeExplanationEvidence = {
    kind: 'code-explanation',
    rubricItems: [
      { id: 'names-the-off-by-one', passed: true },
      { id: 'no-prohibited-claims', passed: true },
    ],
    responseCharacters: 240,
  };
  record.suiteEvidence = evidence;
  return record;
}

export function asMultiFileNavigation(record: BenchmarkRecord): BenchmarkRecord {
  clearSuiteMetrics(record);
  record.suite.id = 'multi-file-navigation';
  record.suite.caseId = 'nav-001';
  record.outcome.correctFileDiscovery = true;
  record.outcome.validDiff = true;
  record.outcome.patchApplySuccess = true;
  record.outcome.verificationSuccess = true;
  const evidence: MultiFileNavigationEvidence = {
    kind: 'multi-file-navigation',
    discoveredPaths: ['src/a.ts', 'src/b.ts'],
    missingRequiredPaths: [],
    claimedForbiddenPaths: [],
    diffValid: true,
    applySucceeded: true,
    verifierSucceeded: true,
  };
  record.suiteEvidence = evidence;
  return record;
}

export function asToolCallingAgentLoop(record: BenchmarkRecord): BenchmarkRecord {
  clearSuiteMetrics(record);
  record.suite.id = 'tool-calling-agent-loop';
  record.suite.caseId = 'loop-001';
  record.outcome.toolCallValid = true;
  record.outcome.validDiff = true;
  record.outcome.patchApplySuccess = true;
  record.outcome.verificationSuccess = true;
  record.outcome.finalTaskSuccess = true;
  const evidence: ToolCallingAgentLoopEvidence = {
    kind: 'tool-calling-agent-loop',
    toolCallLimit: 8,
    toolCalls: [
      { index: 0, tool: 'read_file', valid: true, allowed: true },
      { index: 1, tool: 'propose_diff', valid: true, allowed: true },
    ],
    discoveredPaths: ['src/a.ts'],
    diffValid: true,
    applySucceeded: true,
    verifierSucceeded: true,
    taskSucceeded: true,
  };
  record.suiteEvidence = evidence;
  return record;
}

export function asCancellationRestart(record: BenchmarkRecord): BenchmarkRecord {
  clearSuiteMetrics(record);
  record.suite.id = 'cancellation-restart';
  record.suite.caseId = 'cancel-001';
  // A deliberate-cancel invocation: the oracle passing IS the cancel
  // reaching a terminal cancelled stream in time. No crash happened,
  // so restart/recovery stays unproven (null) on this record.
  record.outcome.status = 'passed';
  record.outcome.stream = 'cancelled';
  record.outcome.cancellationLatencyMs = 80.0;
  record.includeInSummary = false;
  record.exclusionReason = 'deliberate-cancellation';
  const evidence: CancellationRestartEvidence = {
    kind: 'cancellation-restart',
    cancellationLatencyMs: 80.0,
    terminalStreamOutcome: 'cancelled',
    runtimeCrashed: false,
    restartHealthy: null,
    followUpPassed: null,
  };
  record.suiteEvidence = evidence;
  return record;
}
