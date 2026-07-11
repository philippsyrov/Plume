// D129: TypeScript mirror of the schema-v1 benchmark record defined in
// docs/MODEL_BENCHMARKS.md § "JSONL result contract". That document is
// the binding contract; when this file and the doc disagree, the doc
// wins and this file is the bug.
//
// Erasable-syntax TypeScript only (no enums/namespaces): these modules
// run under `vite-node` and must also survive plain type stripping.

export const SCHEMA_VERSION = 1;

// ---- Closed vocabularies -------------------------------------------------

export const POPULATIONS = ['cold', 'warm'] as const;
export const COLD_METHODS = ['processRestart', 'bestEffortCold'] as const;
export const MEASUREMENT_PATHS = ['rawRuntime', 'plumeOrchestration'] as const;
export const COMPARISON_PARITIES = ['strictArtifact', 'equivalentSource'] as const;
export const COUNT_SOURCES = ['runtimeReported', 'harnessTokenizer', 'unavailable'] as const;
export const TIMING_METHODS = ['runtimeReported', 'clientObserved', 'unavailable'] as const;
export const THERMAL_STATES = ['nominal', 'fair', 'serious', 'critical', 'unknown'] as const;
export const STATUSES = ['passed', 'failed', 'unsupported', 'cancelled', 'timedOut', 'error'] as const;
export const STREAM_OUTCOMES = ['completed', 'malformed', 'cancelled', 'timedOut', 'crashed', 'unavailable'] as const;
export const SUITE_IDS = [
  'short-chat',
  'long-context-retrieval',
  'code-explanation',
  'single-file-bug-fix',
  'multi-file-navigation',
  'tool-calling-agent-loop',
  'cancellation-restart',
] as const;

export type Population = (typeof POPULATIONS)[number];
export type ColdMethod = (typeof COLD_METHODS)[number];
export type MeasurementPath = (typeof MEASUREMENT_PATHS)[number];
export type SuiteId = (typeof SUITE_IDS)[number];
export type StreamOutcome = (typeof STREAM_OUTCOMES)[number];
export type Status = (typeof STATUSES)[number];

// ---- Bounds (docs/MODEL_BENCHMARKS.md § Field rules) ---------------------

export const MAX_ID_CHARS = 64;
export const MAX_TIMESTAMP_CHARS = 32;
export const MAX_IDENTIFIER_CHARS = 256; // runtime, model, suite, case ids
export const MAX_LONG_STRING_CHARS = 512; // version/digest/config/provenance/errorClass
export const MAX_EXCLUSION_REASON_CHARS = 256;
export const MIN_PLANNED_REPETITIONS = 3;
export const MAX_PLANNED_REPETITIONS = 30;
export const MAX_STOP_SEQUENCES = 16;
export const MAX_STOP_SEQUENCE_CHARS = 256;
export const MAX_ARTIFACT_REFS = 16;
export const MAX_ARTIFACT_REF_CHARS = 512;
export const MAX_EVIDENCE_ARRAY_ITEMS = 128;
export const MAX_EVIDENCE_STRING_CHARS = 256;
export const MAX_RECORD_BYTES = 64 * 1024;
export const ARTIFACT_ROOT = 'benchmark-artifacts';
export const ARTIFACT_COMPONENT_RE = /^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$/;
export const ID_RE = /^[A-Za-z0-9_-]{1,64}$/;
// RFC 3339 UTC ("Z") with optional fractional seconds.
export const TIMESTAMP_RE = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d{1,9})?Z$/;

// ---- Record shape --------------------------------------------------------

export interface RunBlock {
  id: string;
  groupId: string;
  pairId: string | null;
  timestampUtc: string;
  population: Population;
  coldMethod: ColdMethod | null;
  repetition: number;
  plannedRepetitions: number;
  measurementPath: MeasurementPath;
}

export interface PlumeBlock {
  gitSha: string;
  dirty: boolean;
}

export interface HostBlock {
  machine: string | null;
  appleChip: string | null;
  unifiedMemoryBytes: number | null;
  cpuCoreCount: number | null;
  gpuCoreCount: number | null;
  os: string | null;
  osBuild: string | null;
  powerMode: string | null;
  powerSource: string | null;
  thermalStart: string | null;
}

export interface RuntimeConfigurationBlock {
  digest: string | null;
  mtp: boolean | null;
  speculativeDecoding: boolean | null;
  promptCache: boolean | null;
  kvCacheQuantization: string | null;
  contextTokens: number | null;
  batchSize: number | null;
  threads: number | null;
  gpuLayers: number | null;
}

export interface RuntimeBlock {
  path: string;
  name: string;
  version: string | null;
  engine: string;
  backend: string;
  configuration: RuntimeConfigurationBlock;
  transport: string;
}

export interface ModelArtifactBlock {
  format: string;
  sha256: string;
  quantizationMethod: string | null;
  quantizationBits: number | null;
  quantizationGroupSize: number | null;
  conversionProvenance: string | null;
  conversionConfigDigest: string | null;
}

export interface ContextBlock {
  pointTokens: number;
  configuredTokens: number;
  acceptedTokens: number | null;
  maxOutputTokens: number;
}

export interface SamplingBlock {
  temperature: number | null;
  topP: number | null;
  topK: number | null;
  minP: number | null;
  repeatPenalty: number | null;
  seed: number | null;
  maxOutputTokens: number;
  stopSequences: string[];
}

export interface ModelBlock {
  sourceId: string;
  sourceRevision: string;
  artifact: ModelArtifactBlock;
  comparisonParity: (typeof COMPARISON_PARITIES)[number];
  context: ContextBlock;
  sampling: SamplingBlock;
}

export interface SuiteBlock {
  id: SuiteId;
  caseId: string;
  fixtureRevision: string;
  fixtureDigest: string;
}

export interface TokenizerBlock {
  identity: string | null;
  revision: string | null;
  digest: string | null;
  chatTemplate: string | null;
  chatTemplateDigest: string | null;
}

export interface TokensBlock {
  tokenizer: TokenizerBlock;
  countSource: (typeof COUNT_SOURCES)[number];
  finalAssembledPromptTokens: number | null;
  promptBytes: number | null;
  outputTokens: number | null;
}

export interface TimingBlock {
  method: (typeof TIMING_METHODS)[number];
  timeToFirstTokenMs: number | null;
  promptEvaluationMs: number | null;
  generationDurationMs: number | null;
  promptTokensPerSecond: number | null;
  generationTokensPerSecond: number | null;
  endToEndMs: number | null;
}

export interface ResourcesBlock {
  peakUnifiedMemoryBytes: number | null;
  swapDeltaBytes: number | null;
  thermalEnd: string | null;
  wallEnergyJoules: number | null;
}

export interface OutcomeBlock {
  status: Status;
  toolCallValid: boolean | null;
  correctFileDiscovery: boolean | null;
  validDiff: boolean | null;
  patchApplySuccess: boolean | null;
  verificationSuccess: boolean | null;
  finalTaskSuccess: boolean | null;
  stream: StreamOutcome;
  timeout: boolean;
  timeoutLimitMs: number | null;
  cancellationLatencyMs: number | null;
  crash: boolean;
  restartRecovery: boolean | null;
  errorClass: string | null;
}

// ---- Suite evidence (docs § Suite evidence extension) --------------------

export interface ShortChatEvidence {
  kind: 'short-chat';
  replyClassification: string | null;
  terminalStreamOutcome: StreamOutcome | null;
}

export interface LongContextRetrievalEvidence {
  kind: 'long-context-retrieval';
  requestedContextTokens: number | null;
  acceptedContextTokens: number | null;
  finalAssembledPromptTokens: number | null;
  retrievedKeys: string[];
  missingKeys: string[];
  incorrectDecoyKeys: string[];
  truncated: boolean | null;
}

export interface RubricItemEvidence {
  id: string;
  passed: boolean;
}

export interface CodeExplanationEvidence {
  kind: 'code-explanation';
  rubricItems: RubricItemEvidence[];
  responseCharacters: number | null;
}

export interface SingleFileBugFixEvidence {
  kind: 'single-file-bug-fix';
  targetFile: string | null;
  diffValid: boolean | null;
  applySucceeded: boolean | null;
  verifierSucceeded: boolean | null;
  rollbackSucceeded: boolean | null;
}

export interface MultiFileNavigationEvidence {
  kind: 'multi-file-navigation';
  discoveredPaths: string[];
  missingRequiredPaths: string[];
  claimedForbiddenPaths: string[];
  diffValid: boolean | null;
  applySucceeded: boolean | null;
  verifierSucceeded: boolean | null;
}

export interface ToolCallEvidence {
  index: number;
  tool: string;
  valid: boolean;
  allowed: boolean;
}

export interface ToolCallingAgentLoopEvidence {
  kind: 'tool-calling-agent-loop';
  toolCallLimit: number | null;
  toolCalls: ToolCallEvidence[];
  discoveredPaths: string[];
  diffValid: boolean | null;
  applySucceeded: boolean | null;
  verifierSucceeded: boolean | null;
  taskSucceeded: boolean | null;
}

export interface CancellationRestartEvidence {
  kind: 'cancellation-restart';
  cancellationLatencyMs: number | null;
  terminalStreamOutcome: StreamOutcome | null;
  runtimeCrashed: boolean | null;
  restartHealthy: boolean | null;
  followUpPassed: boolean | null;
}

export type SuiteEvidence =
  | ShortChatEvidence
  | LongContextRetrievalEvidence
  | CodeExplanationEvidence
  | SingleFileBugFixEvidence
  | MultiFileNavigationEvidence
  | ToolCallingAgentLoopEvidence
  | CancellationRestartEvidence;

export interface BenchmarkRecord {
  schemaVersion: number;
  run: RunBlock;
  plume: PlumeBlock;
  host: HostBlock;
  runtime: RuntimeBlock;
  model: ModelBlock;
  suite: SuiteBlock;
  suiteEvidence: SuiteEvidence;
  tokens: TokensBlock;
  timing: TimingBlock;
  resources: ResourcesBlock;
  includeInSummary: boolean;
  exclusionReason: string | null;
  outcome: OutcomeBlock;
  artifacts: string[];
}

/// The exact field set of a suiteEvidence object per kind. The
/// validator enforces "exactly these fields", making suiteEvidence a
/// closed discriminated union — not an unknown-field escape hatch.
export const EVIDENCE_FIELDS: Record<SuiteId, readonly string[]> = {
  'short-chat': ['kind', 'replyClassification', 'terminalStreamOutcome'],
  'long-context-retrieval': [
    'kind',
    'requestedContextTokens',
    'acceptedContextTokens',
    'finalAssembledPromptTokens',
    'retrievedKeys',
    'missingKeys',
    'incorrectDecoyKeys',
    'truncated',
  ],
  'code-explanation': ['kind', 'rubricItems', 'responseCharacters'],
  'single-file-bug-fix': [
    'kind',
    'targetFile',
    'diffValid',
    'applySucceeded',
    'verifierSucceeded',
    'rollbackSucceeded',
  ],
  'multi-file-navigation': [
    'kind',
    'discoveredPaths',
    'missingRequiredPaths',
    'claimedForbiddenPaths',
    'diffValid',
    'applySucceeded',
    'verifierSucceeded',
  ],
  'tool-calling-agent-loop': [
    'kind',
    'toolCallLimit',
    'toolCalls',
    'discoveredPaths',
    'diffValid',
    'applySucceeded',
    'verifierSucceeded',
    'taskSucceeded',
  ],
  'cancellation-restart': [
    'kind',
    'cancellationLatencyMs',
    'terminalStreamOutcome',
    'runtimeCrashed',
    'restartHealthy',
    'followUpPassed',
  ],
};

export const TOP_LEVEL_FIELDS = [
  'schemaVersion',
  'run',
  'plume',
  'host',
  'runtime',
  'model',
  'suite',
  'suiteEvidence',
  'tokens',
  'timing',
  'resources',
  'includeInSummary',
  'exclusionReason',
  'outcome',
  'artifacts',
] as const;
