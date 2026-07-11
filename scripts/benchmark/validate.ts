// D129: schema-v1 record validation for docs/MODEL_BENCHMARKS.md
// § "JSONL result contract". Two modes:
//
//   * `producer` — what the harness runs before writing a record.
//     Unknown fields are errors ("a producer must not add unversioned
//     fields").
//   * `reader` — what the summarizer runs on records it ingests.
//     Unknown fields are warnings (preserved, ignored for analysis);
//     a newer major schema version is refused.
//
// Bounds violations never truncate: a record that breaks a bound is
// rejected whole.

import { validateSuiteEvidence } from './evidence.ts';
import {
  ARTIFACT_COMPONENT_RE,
  ARTIFACT_ROOT,
  COLD_METHODS,
  COMPARISON_PARITIES,
  COUNT_SOURCES,
  ID_RE,
  MAX_ARTIFACT_REFS,
  MAX_ARTIFACT_REF_CHARS,
  MAX_EXCLUSION_REASON_CHARS,
  MAX_IDENTIFIER_CHARS,
  MAX_LONG_STRING_CHARS,
  MAX_PLANNED_REPETITIONS,
  MAX_RECORD_BYTES,
  MAX_STOP_SEQUENCES,
  MAX_STOP_SEQUENCE_CHARS,
  MAX_TIMESTAMP_CHARS,
  MEASUREMENT_PATHS,
  MIN_PLANNED_REPETITIONS,
  POPULATIONS,
  SCHEMA_VERSION,
  STATUSES,
  STREAM_OUTCOMES,
  SUITE_IDS,
  THERMAL_STATES,
  TIMESTAMP_RE,
  TIMING_METHODS,
  TOP_LEVEL_FIELDS,
} from './types.ts';
import type { BenchmarkRecord } from './types.ts';

export type ValidationMode = 'producer' | 'reader';

export interface ValidationResult {
  ok: boolean;
  errors: string[];
  warnings: string[];
}

type Obj = Record<string, unknown>;

const isObj = (v: unknown): v is Obj =>
  typeof v === 'object' && v !== null && !Array.isArray(v);
const isBool = (v: unknown): v is boolean => typeof v === 'boolean';
const isNonNegInt = (v: unknown): v is number =>
  typeof v === 'number' && Number.isInteger(v) && v >= 0;
const isNonNegFinite = (v: unknown): v is number =>
  typeof v === 'number' && Number.isFinite(v) && v >= 0;
const isPrintableAscii = (v: string): boolean => /^[\x20-\x7E]*$/.test(v);

class Checker {
  errors: string[] = [];

  fail(path: string, message: string): void {
    this.errors.push(`${path}: ${message}`);
  }

  /// Object with exactly `fields` — reports both unknown and missing.
  exactFields(path: string, value: Obj, fields: readonly string[]): void {
    for (const key of Object.keys(value)) {
      if (!fields.includes(key)) this.fail(`${path}.${key}`, 'not a documented field');
    }
    for (const key of fields) {
      if (!(key in value)) this.fail(`${path}.${key}`, 'required and missing');
    }
  }

  boundedString(path: string, value: unknown, max: number, nullable: boolean): void {
    if (value === null) {
      if (!nullable) this.fail(path, 'must not be null');
      return;
    }
    if (typeof value !== 'string' || value.length === 0 || value.length > max) {
      this.fail(path, `must be a 1..${max} character string${nullable ? ' or null' : ''}`);
    }
  }

  enumValue(path: string, value: unknown, allowed: readonly string[], nullable: boolean): void {
    if (value === null && nullable) return;
    if (typeof value !== 'string' || !allowed.includes(value)) {
      this.fail(path, `must be one of ${allowed.join(', ')}${nullable ? ' or null' : ''}`);
    }
  }

  nullableNonNegNumber(path: string, value: unknown): void {
    if (value !== null && !isNonNegFinite(value)) {
      this.fail(path, 'must be a finite non-negative number or null');
    }
  }

  nullableNonNegInt(path: string, value: unknown): void {
    if (value !== null && !isNonNegInt(value)) {
      this.fail(path, 'must be a finite non-negative integer or null');
    }
  }

  nullableBool(path: string, value: unknown): void {
    if (value !== null && !isBool(value)) {
      this.fail(path, 'must be boolean or null');
    }
  }
}

/// Validate one artifact reference against the path grammar in
/// docs/MODEL_BENCHMARKS.md § "Field rules". Lexical checks only;
/// the runner separately refuses symlinks that resolve outside the
/// allowed root at write time.
export function artifactRefError(ref: string): string | null {
  if (ref.length === 0 || ref.length > MAX_ARTIFACT_REF_CHARS) {
    return `must be a 1..${MAX_ARTIFACT_REF_CHARS} character string`;
  }
  if (!isPrintableAscii(ref)) return 'must be printable ASCII';
  if (ref.includes('\\')) return 'must not contain backslashes';
  if (ref.includes('\0')) return 'must not contain NUL bytes';
  if (ref.startsWith('/')) return 'must not be absolute';
  const components = ref.split('/');
  if (components[0] !== ARTIFACT_ROOT) return `must start with ${ARTIFACT_ROOT}/`;
  if (components.length < 2) return 'must reference a path inside the artifact root';
  for (const component of components.slice(1)) {
    if (component === '' || component === '.' || component === '..') {
      return 'must not contain empty, "." or ".." components';
    }
    if (!ARTIFACT_COMPONENT_RE.test(component)) {
      return `component "${component}" violates the artifact path grammar`;
    }
  }
  return null;
}

export function validateRecord(value: unknown, mode: ValidationMode): ValidationResult {
  const c = new Checker();
  const warnings: string[] = [];

  if (!isObj(value)) {
    return { ok: false, errors: ['record: must be a JSON object'], warnings };
  }

  // Top-level field set. Readers tolerate (and warn on) unknown
  // fields; producers must not emit them. Missing documented fields
  // are errors in both modes.
  for (const key of Object.keys(value)) {
    if (!(TOP_LEVEL_FIELDS as readonly string[]).includes(key)) {
      if (mode === 'producer') c.fail(key, 'not a documented top-level field');
      else warnings.push(`${key}: unknown top-level field ignored for analysis`);
    }
  }
  for (const key of TOP_LEVEL_FIELDS) {
    if (!(key in value)) c.fail(key, 'required top-level field missing');
  }

  const version = value['schemaVersion'];
  if (!isNonNegInt(version) || version < 1) {
    c.fail('schemaVersion', 'must be a positive integer');
  } else if (version > SCHEMA_VERSION) {
    c.fail('schemaVersion', `version ${version} is newer than supported ${SCHEMA_VERSION} — refusing to guess a mapping`);
  } else if (version < SCHEMA_VERSION) {
    c.fail('schemaVersion', `version ${version} requires an explicit migrator`);
  }

  if (isObj(value['run'])) checkRun(c, value['run']);
  else c.fail('run', 'must be an object');
  if (isObj(value['plume'])) checkPlume(c, value['plume']);
  else c.fail('plume', 'must be an object');
  if (isObj(value['host'])) checkHost(c, value['host']);
  else c.fail('host', 'must be an object');
  if (isObj(value['runtime'])) checkRuntime(c, value['runtime']);
  else c.fail('runtime', 'must be an object');
  if (isObj(value['model'])) checkModel(c, value['model']);
  else c.fail('model', 'must be an object');
  if (isObj(value['suite'])) checkSuite(c, value['suite']);
  else c.fail('suite', 'must be an object');
  if (isObj(value['tokens'])) checkTokens(c, value['tokens']);
  else c.fail('tokens', 'must be an object');
  if (isObj(value['timing'])) checkTiming(c, value['timing']);
  else c.fail('timing', 'must be an object');
  if (isObj(value['resources'])) checkResources(c, value['resources']);
  else c.fail('resources', 'must be an object');
  if (isObj(value['outcome'])) checkOutcome(c, value['outcome']);
  else c.fail('outcome', 'must be an object');

  // includeInSummary / exclusionReason coupling: an excluded attempt
  // carries a bounded reason; an included one carries null.
  const include = value['includeInSummary'];
  const reason = value['exclusionReason'];
  if (!isBool(include)) c.fail('includeInSummary', 'must be boolean');
  if (reason !== null) {
    c.boundedString('exclusionReason', reason, MAX_EXCLUSION_REASON_CHARS, false);
    if (typeof reason === 'string' && !isPrintableAscii(reason)) {
      c.fail('exclusionReason', 'must be printable ASCII');
    }
  }
  if (include === true && reason !== null) {
    c.fail('exclusionReason', 'must be null when includeInSummary is true');
  }
  if (include === false && reason === null) {
    c.fail('exclusionReason', 'an excluded attempt requires a non-null reason');
  }

  const artifacts = value['artifacts'];
  if (!Array.isArray(artifacts)) {
    c.fail('artifacts', 'must be an array');
  } else {
    if (artifacts.length > MAX_ARTIFACT_REFS) {
      c.fail('artifacts', `at most ${MAX_ARTIFACT_REFS} references`);
    }
    artifacts.forEach((ref, i) => {
      if (typeof ref !== 'string') {
        c.fail(`artifacts[${i}]`, 'must be a string');
      } else {
        const err = artifactRefError(ref);
        if (err !== null) c.fail(`artifacts[${i}]`, err);
      }
    });
  }

  // Cross-block rules need structurally sound blocks; each guard
  // re-checks the parts it reads.
  checkCrossField(c, value);

  const suite = value['suite'];
  c.errors.push(
    ...validateSuiteEvidence(value['suiteEvidence'], {
      suiteId: isObj(suite) ? suite['id'] : undefined,
      outcome: value['outcome'],
      tokens: value['tokens'],
      modelContext: isObj(value['model']) ? (value['model'] as Obj)['context'] : undefined,
    }),
  );

  return { ok: c.errors.length === 0, errors: c.errors, warnings };
}

/// Serialize a validated record to its JSONL line, refusing (never
/// truncating) a record that would exceed the 64 KiB bound.
export function serializeRecord(record: BenchmarkRecord): string {
  const line = JSON.stringify(record);
  const bytes = new TextEncoder().encode(line).length;
  if (bytes > MAX_RECORD_BYTES) {
    throw new Error(`record is ${bytes} bytes; the contract caps a serialized record at ${MAX_RECORD_BYTES}`);
  }
  return line;
}

/// Parse one JSONL line. Returns the parsed value or an error string.
export function parseRecordLine(line: string): { value: unknown } | { error: string } {
  const bytes = new TextEncoder().encode(line).length;
  if (bytes > MAX_RECORD_BYTES) {
    return { error: `line is ${bytes} bytes; the contract caps a serialized record at ${MAX_RECORD_BYTES}` };
  }
  try {
    return { value: JSON.parse(line) };
  } catch (err) {
    return { error: `line is not valid JSON: ${err instanceof Error ? err.message : String(err)}` };
  }
}

// ---- Per-block checks ------------------------------------------------------

const RUN_FIELDS = [
  'id', 'groupId', 'pairId', 'timestampUtc', 'population', 'coldMethod',
  'repetition', 'plannedRepetitions', 'measurementPath',
] as const;

function checkRun(c: Checker, run: Obj): void {
  c.exactFields('run', run, RUN_FIELDS);
  for (const field of ['id', 'groupId'] as const) {
    const v = run[field];
    if (typeof v !== 'string' || !ID_RE.test(v)) {
      c.fail(`run.${field}`, 'must be ASCII [A-Za-z0-9_-], at most 64 characters');
    }
  }
  const pairId = run['pairId'];
  if (pairId !== null && (typeof pairId !== 'string' || !ID_RE.test(pairId))) {
    c.fail('run.pairId', 'must be ASCII [A-Za-z0-9_-], at most 64 characters, or null');
  }
  const ts = run['timestampUtc'];
  if (typeof ts !== 'string' || ts.length > MAX_TIMESTAMP_CHARS || !TIMESTAMP_RE.test(ts)) {
    c.fail('run.timestampUtc', `must be an RFC 3339 UTC timestamp of at most ${MAX_TIMESTAMP_CHARS} characters`);
  }
  c.enumValue('run.population', run['population'], POPULATIONS, false);
  c.enumValue('run.coldMethod', run['coldMethod'], COLD_METHODS, true);
  if (run['population'] === 'cold' && run['coldMethod'] === null) {
    c.fail('run.coldMethod', 'required for cold attempts');
  }
  if (run['population'] === 'warm' && run['coldMethod'] !== null) {
    c.fail('run.coldMethod', 'must be null for warm attempts');
  }
  const planned = run['plannedRepetitions'];
  if (!isNonNegInt(planned) || planned < MIN_PLANNED_REPETITIONS || planned > MAX_PLANNED_REPETITIONS) {
    c.fail('run.plannedRepetitions', `must be ${MIN_PLANNED_REPETITIONS}..${MAX_PLANNED_REPETITIONS}`);
  }
  const repetition = run['repetition'];
  if (!isNonNegInt(repetition) || repetition < 1 || (isNonNegInt(planned) && repetition > planned)) {
    c.fail('run.repetition', 'must be 1..plannedRepetitions');
  }
  c.enumValue('run.measurementPath', run['measurementPath'], MEASUREMENT_PATHS, false);
}

function checkPlume(c: Checker, plume: Obj): void {
  c.exactFields('plume', plume, ['gitSha', 'dirty']);
  const sha = plume['gitSha'];
  if (typeof sha !== 'string' || !/^[0-9a-f]{40}$/.test(sha)) {
    c.fail('plume.gitSha', 'must be a 40-character lowercase git SHA');
  }
  if (!isBool(plume['dirty'])) c.fail('plume.dirty', 'must be boolean');
}

const HOST_FIELDS = [
  'machine', 'appleChip', 'unifiedMemoryBytes', 'cpuCoreCount', 'gpuCoreCount',
  'os', 'osBuild', 'powerMode', 'powerSource', 'thermalStart',
] as const;

function checkHost(c: Checker, host: Obj): void {
  c.exactFields('host', host, HOST_FIELDS);
  for (const field of ['machine', 'appleChip', 'os', 'osBuild', 'powerMode', 'powerSource'] as const) {
    c.boundedString(`host.${field}`, host[field], MAX_IDENTIFIER_CHARS, true);
  }
  c.nullableNonNegInt('host.unifiedMemoryBytes', host['unifiedMemoryBytes']);
  c.nullableNonNegInt('host.cpuCoreCount', host['cpuCoreCount']);
  c.nullableNonNegInt('host.gpuCoreCount', host['gpuCoreCount']);
  c.enumValue('host.thermalStart', host['thermalStart'], THERMAL_STATES, true);
}

const RUNTIME_FIELDS = ['path', 'name', 'version', 'engine', 'backend', 'configuration', 'transport'] as const;
const RUNTIME_CONFIG_FIELDS = [
  'digest', 'mtp', 'speculativeDecoding', 'promptCache', 'kvCacheQuantization',
  'contextTokens', 'batchSize', 'threads', 'gpuLayers',
] as const;

function checkRuntime(c: Checker, runtime: Obj): void {
  c.exactFields('runtime', runtime, RUNTIME_FIELDS);
  c.boundedString('runtime.path', runtime['path'], MAX_IDENTIFIER_CHARS, false);
  c.boundedString('runtime.name', runtime['name'], MAX_IDENTIFIER_CHARS, false);
  c.boundedString('runtime.version', runtime['version'], MAX_LONG_STRING_CHARS, true);
  c.boundedString('runtime.engine', runtime['engine'], MAX_IDENTIFIER_CHARS, false);
  c.boundedString('runtime.backend', runtime['backend'], MAX_IDENTIFIER_CHARS, false);
  c.boundedString('runtime.transport', runtime['transport'], MAX_IDENTIFIER_CHARS, false);
  const config = runtime['configuration'];
  if (!isObj(config)) {
    c.fail('runtime.configuration', 'must be an object');
    return;
  }
  c.exactFields('runtime.configuration', config, RUNTIME_CONFIG_FIELDS);
  c.boundedString('runtime.configuration.digest', config['digest'], MAX_LONG_STRING_CHARS, true);
  c.nullableBool('runtime.configuration.mtp', config['mtp']);
  c.nullableBool('runtime.configuration.speculativeDecoding', config['speculativeDecoding']);
  c.nullableBool('runtime.configuration.promptCache', config['promptCache']);
  c.boundedString('runtime.configuration.kvCacheQuantization', config['kvCacheQuantization'], MAX_LONG_STRING_CHARS, true);
  c.nullableNonNegInt('runtime.configuration.contextTokens', config['contextTokens']);
  c.nullableNonNegInt('runtime.configuration.batchSize', config['batchSize']);
  c.nullableNonNegInt('runtime.configuration.threads', config['threads']);
  c.nullableNonNegInt('runtime.configuration.gpuLayers', config['gpuLayers']);
}

const MODEL_FIELDS = ['sourceId', 'sourceRevision', 'artifact', 'comparisonParity', 'context', 'sampling'] as const;
const ARTIFACT_FIELDS = [
  'format', 'sha256', 'quantizationMethod', 'quantizationBits', 'quantizationGroupSize',
  'conversionProvenance', 'conversionConfigDigest',
] as const;
const CONTEXT_FIELDS = ['pointTokens', 'configuredTokens', 'acceptedTokens', 'maxOutputTokens'] as const;
const SAMPLING_FIELDS = [
  'temperature', 'topP', 'topK', 'minP', 'repeatPenalty', 'seed', 'maxOutputTokens', 'stopSequences',
] as const;

function checkModel(c: Checker, model: Obj): void {
  c.exactFields('model', model, MODEL_FIELDS);
  c.boundedString('model.sourceId', model['sourceId'], MAX_IDENTIFIER_CHARS, false);
  c.boundedString('model.sourceRevision', model['sourceRevision'], MAX_IDENTIFIER_CHARS, false);
  c.enumValue('model.comparisonParity', model['comparisonParity'], COMPARISON_PARITIES, false);

  const artifact = model['artifact'];
  if (isObj(artifact)) {
    c.exactFields('model.artifact', artifact, ARTIFACT_FIELDS);
    c.boundedString('model.artifact.format', artifact['format'], MAX_IDENTIFIER_CHARS, false);
    const sha = artifact['sha256'];
    if (typeof sha !== 'string' || !/^sha256:[0-9a-f]{64}$/.test(sha)) {
      c.fail('model.artifact.sha256', 'must be "sha256:" + 64 lowercase hex characters');
    }
    c.boundedString('model.artifact.quantizationMethod', artifact['quantizationMethod'], MAX_IDENTIFIER_CHARS, true);
    c.nullableNonNegInt('model.artifact.quantizationBits', artifact['quantizationBits']);
    c.nullableNonNegInt('model.artifact.quantizationGroupSize', artifact['quantizationGroupSize']);
    c.boundedString('model.artifact.conversionProvenance', artifact['conversionProvenance'], MAX_LONG_STRING_CHARS, true);
    c.boundedString('model.artifact.conversionConfigDigest', artifact['conversionConfigDigest'], MAX_LONG_STRING_CHARS, true);
  } else {
    c.fail('model.artifact', 'must be an object');
  }

  const context = model['context'];
  if (isObj(context)) {
    c.exactFields('model.context', context, CONTEXT_FIELDS);
    for (const field of ['pointTokens', 'configuredTokens', 'maxOutputTokens'] as const) {
      if (!isNonNegInt(context[field])) c.fail(`model.context.${field}`, 'must be a finite non-negative integer');
    }
    c.nullableNonNegInt('model.context.acceptedTokens', context['acceptedTokens']);
  } else {
    c.fail('model.context', 'must be an object');
  }

  const sampling = model['sampling'];
  if (isObj(sampling)) {
    c.exactFields('model.sampling', sampling, SAMPLING_FIELDS);
    c.nullableNonNegNumber('model.sampling.temperature', sampling['temperature']);
    c.nullableNonNegNumber('model.sampling.topP', sampling['topP']);
    c.nullableNonNegInt('model.sampling.topK', sampling['topK']);
    c.nullableNonNegNumber('model.sampling.minP', sampling['minP']);
    c.nullableNonNegNumber('model.sampling.repeatPenalty', sampling['repeatPenalty']);
    c.nullableNonNegInt('model.sampling.seed', sampling['seed']);
    if (!isNonNegInt(sampling['maxOutputTokens'])) {
      c.fail('model.sampling.maxOutputTokens', 'required and must be a finite non-negative integer');
    }
    const stops = sampling['stopSequences'];
    if (!Array.isArray(stops)) {
      c.fail('model.sampling.stopSequences', 'must be an array');
    } else {
      if (stops.length > MAX_STOP_SEQUENCES) {
        c.fail('model.sampling.stopSequences', `at most ${MAX_STOP_SEQUENCES} strings`);
      }
      stops.forEach((s, i) => {
        if (typeof s !== 'string' || s.length === 0 || s.length > MAX_STOP_SEQUENCE_CHARS) {
          c.fail(`model.sampling.stopSequences[${i}]`, `must be a 1..${MAX_STOP_SEQUENCE_CHARS} character string`);
        }
      });
    }
  } else {
    c.fail('model.sampling', 'must be an object');
  }
}

function checkSuite(c: Checker, suite: Obj): void {
  c.exactFields('suite', suite, ['id', 'caseId', 'fixtureRevision', 'fixtureDigest']);
  c.enumValue('suite.id', suite['id'], SUITE_IDS, false);
  c.boundedString('suite.caseId', suite['caseId'], MAX_IDENTIFIER_CHARS, false);
  c.boundedString('suite.fixtureRevision', suite['fixtureRevision'], MAX_IDENTIFIER_CHARS, false);
  c.boundedString('suite.fixtureDigest', suite['fixtureDigest'], MAX_LONG_STRING_CHARS, false);
}

const TOKENS_FIELDS = ['tokenizer', 'countSource', 'finalAssembledPromptTokens', 'promptBytes', 'outputTokens'] as const;
const TOKENIZER_FIELDS = ['identity', 'revision', 'digest', 'chatTemplate', 'chatTemplateDigest'] as const;

function checkTokens(c: Checker, tokens: Obj): void {
  c.exactFields('tokens', tokens, TOKENS_FIELDS);
  const tokenizer = tokens['tokenizer'];
  if (isObj(tokenizer)) {
    c.exactFields('tokens.tokenizer', tokenizer, TOKENIZER_FIELDS);
    c.boundedString('tokens.tokenizer.identity', tokenizer['identity'], MAX_IDENTIFIER_CHARS, true);
    c.boundedString('tokens.tokenizer.revision', tokenizer['revision'], MAX_IDENTIFIER_CHARS, true);
    c.boundedString('tokens.tokenizer.digest', tokenizer['digest'], MAX_LONG_STRING_CHARS, true);
    c.boundedString('tokens.tokenizer.chatTemplate', tokenizer['chatTemplate'], MAX_IDENTIFIER_CHARS, true);
    c.boundedString('tokens.tokenizer.chatTemplateDigest', tokenizer['chatTemplateDigest'], MAX_LONG_STRING_CHARS, true);
  } else {
    c.fail('tokens.tokenizer', 'must be an object');
  }
  c.enumValue('tokens.countSource', tokens['countSource'], COUNT_SOURCES, false);
  c.nullableNonNegInt('tokens.finalAssembledPromptTokens', tokens['finalAssembledPromptTokens']);
  c.nullableNonNegInt('tokens.promptBytes', tokens['promptBytes']);
  c.nullableNonNegInt('tokens.outputTokens', tokens['outputTokens']);
}

const TIMING_FIELDS = [
  'method', 'timeToFirstTokenMs', 'promptEvaluationMs', 'generationDurationMs',
  'promptTokensPerSecond', 'generationTokensPerSecond', 'endToEndMs',
] as const;

function checkTiming(c: Checker, timing: Obj): void {
  c.exactFields('timing', timing, TIMING_FIELDS);
  c.enumValue('timing.method', timing['method'], TIMING_METHODS, false);
  for (const field of TIMING_FIELDS.slice(1)) {
    c.nullableNonNegNumber(`timing.${field}`, timing[field]);
  }
}

const RESOURCES_FIELDS = ['peakUnifiedMemoryBytes', 'swapDeltaBytes', 'thermalEnd', 'wallEnergyJoules'] as const;

function checkResources(c: Checker, resources: Obj): void {
  c.exactFields('resources', resources, RESOURCES_FIELDS);
  c.nullableNonNegInt('resources.peakUnifiedMemoryBytes', resources['peakUnifiedMemoryBytes']);
  const swap = resources['swapDeltaBytes'];
  // The one signed metric: may be negative, never clamped.
  if (swap !== null && (typeof swap !== 'number' || !Number.isInteger(swap))) {
    c.fail('resources.swapDeltaBytes', 'must be a finite signed integer or null');
  }
  c.enumValue('resources.thermalEnd', resources['thermalEnd'], THERMAL_STATES, true);
  c.nullableNonNegNumber('resources.wallEnergyJoules', resources['wallEnergyJoules']);
}

const OUTCOME_FIELDS = [
  'status', 'toolCallValid', 'correctFileDiscovery', 'validDiff', 'patchApplySuccess',
  'verificationSuccess', 'finalTaskSuccess', 'stream', 'timeout', 'timeoutLimitMs',
  'cancellationLatencyMs', 'crash', 'restartRecovery', 'errorClass',
] as const;

function checkOutcome(c: Checker, outcome: Obj): void {
  c.exactFields('outcome', outcome, OUTCOME_FIELDS);
  c.enumValue('outcome.status', outcome['status'], STATUSES, false);
  c.enumValue('outcome.stream', outcome['stream'], STREAM_OUTCOMES, false);
  for (const field of [
    'toolCallValid', 'correctFileDiscovery', 'validDiff', 'patchApplySuccess',
    'verificationSuccess', 'finalTaskSuccess', 'restartRecovery',
  ] as const) {
    c.nullableBool(`outcome.${field}`, outcome[field]);
  }
  if (!isBool(outcome['timeout'])) c.fail('outcome.timeout', 'must be boolean');
  if (!isBool(outcome['crash'])) c.fail('outcome.crash', 'must be boolean');
  c.nullableNonNegNumber('outcome.timeoutLimitMs', outcome['timeoutLimitMs']);
  c.nullableNonNegNumber('outcome.cancellationLatencyMs', outcome['cancellationLatencyMs']);
  c.boundedString('outcome.errorClass', outcome['errorClass'], MAX_LONG_STRING_CHARS, true);
}

// ---- Cross-block rules -----------------------------------------------------

function checkCrossField(c: Checker, record: Obj): void {
  const model = record['model'];
  if (isObj(model) && isObj(model['context']) && isObj(model['sampling'])) {
    const context = model['context'] as Obj;
    const sampling = model['sampling'] as Obj;
    if (
      isNonNegInt(context['maxOutputTokens']) &&
      isNonNegInt(sampling['maxOutputTokens']) &&
      context['maxOutputTokens'] !== sampling['maxOutputTokens']
    ) {
      c.fail('model.sampling.maxOutputTokens', 'must match the reserved value in model.context.maxOutputTokens');
    }
  }

  // countSource 'unavailable' forbids token counts and token-derived
  // rates — they would be guesses.
  const tokens = record['tokens'];
  const timing = record['timing'];
  if (isObj(tokens) && tokens['countSource'] === 'unavailable') {
    if (tokens['finalAssembledPromptTokens'] !== null) {
      c.fail('tokens.finalAssembledPromptTokens', 'must be null when countSource is unavailable');
    }
    if (tokens['outputTokens'] !== null) {
      c.fail('tokens.outputTokens', 'must be null when countSource is unavailable');
    }
    if (isObj(timing)) {
      if (timing['promptTokensPerSecond'] !== null) {
        c.fail('timing.promptTokensPerSecond', 'must be null when tokens.countSource is unavailable');
      }
      if (timing['generationTokensPerSecond'] !== null) {
        c.fail('timing.generationTokensPerSecond', 'must be null when tokens.countSource is unavailable');
      }
    }
  }

  // A timed-out outcome must carry the configured limit it reached.
  const outcome = record['outcome'];
  if (isObj(outcome) && outcome['timeout'] === true && outcome['timeoutLimitMs'] === null) {
    c.fail('outcome.timeoutLimitMs', 'a timeout means the configured limit was reached — the limit must be recorded');
  }
}
