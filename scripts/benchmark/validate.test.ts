// D129: schema-v1 record validation tests. The contradiction battery
// is the load-bearing part — it pins the equality invariants between
// duplicated common fields and suiteEvidence that the D128 review
// deferred to this validator.

import { describe, expect, it } from 'vitest';

import {
  asCancellationRestart,
  asCodeExplanation,
  asLongContextRetrieval,
  asMultiFileNavigation,
  asShortChat,
  asToolCallingAgentLoop,
  makeValidRecord,
} from './example-record.ts';
import { artifactRefError, parseRecordLine, serializeRecord, validateRecord } from './validate.ts';
import type { BenchmarkRecord } from './types.ts';

function expectValid(record: BenchmarkRecord): void {
  const result = validateRecord(record, 'producer');
  expect(result.errors).toEqual([]);
  expect(result.ok).toBe(true);
}

function expectError(record: unknown, needle: string): void {
  const result = validateRecord(record, 'producer');
  expect(result.ok).toBe(false);
  expect(result.errors.join('\n')).toContain(needle);
}

describe('valid records', () => {
  it('accepts one valid record per suite kind', () => {
    expectValid(makeValidRecord());
    expectValid(asShortChat(makeValidRecord()));
    expectValid(asLongContextRetrieval(makeValidRecord()));
    expectValid(asCodeExplanation(makeValidRecord()));
    expectValid(asMultiFileNavigation(makeValidRecord()));
    expectValid(asToolCallingAgentLoop(makeValidRecord()));
    expectValid(asCancellationRestart(makeValidRecord()));
  });
});

describe('contradictions between suiteEvidence and common fields', () => {
  it('rejects diffValid contradicting outcome.validDiff', () => {
    const record = makeValidRecord();
    record.outcome.validDiff = true;
    (record.suiteEvidence as { diffValid: boolean | null }).diffValid = false;
    expectError(record, 'suiteEvidence.diffValid: contradicts outcome.validDiff');
  });

  it('rejects applySucceeded contradicting outcome.patchApplySuccess', () => {
    const record = makeValidRecord();
    record.outcome.patchApplySuccess = false;
    expectError(record, 'suiteEvidence.applySucceeded: contradicts outcome.patchApplySuccess');
  });

  it('rejects verifierSucceeded contradicting outcome.verificationSuccess', () => {
    const record = makeValidRecord();
    record.outcome.verificationSuccess = null;
    expectError(record, 'suiteEvidence.verifierSucceeded: contradicts outcome.verificationSuccess');
  });

  it('rejects cancellation latency disagreeing between outcome and evidence', () => {
    const record = asCancellationRestart(makeValidRecord());
    record.outcome.cancellationLatencyMs = 81.0;
    expectError(record, 'suiteEvidence.cancellationLatencyMs: contradicts outcome.cancellationLatencyMs');
  });

  it('rejects finalAssembledPromptTokens disagreeing between tokens and evidence', () => {
    const record = asLongContextRetrieval(makeValidRecord());
    record.tokens.finalAssembledPromptTokens = 999;
    expectError(record, 'suiteEvidence.finalAssembledPromptTokens: contradicts tokens.finalAssembledPromptTokens');
  });

  it('rejects requested/accepted context disagreeing with model.context', () => {
    const requested = asLongContextRetrieval(makeValidRecord());
    (requested.suiteEvidence as { requestedContextTokens: number | null }).requestedContextTokens = 1;
    expectError(requested, 'suiteEvidence.requestedContextTokens: contradicts model.context.configuredTokens');

    const accepted = asLongContextRetrieval(makeValidRecord());
    accepted.model.context.acceptedTokens = 2048;
    expectError(accepted, 'suiteEvidence.acceptedContextTokens: contradicts model.context.acceptedTokens');
  });

  it('rejects taskSucceeded contradicting outcome.finalTaskSuccess', () => {
    const record = asToolCallingAgentLoop(makeValidRecord());
    record.outcome.finalTaskSuccess = false;
    expectError(record, 'suiteEvidence.taskSucceeded: contradicts outcome.finalTaskSuccess');
  });

  it('rejects terminalStreamOutcome contradicting outcome.stream', () => {
    const record = asShortChat(makeValidRecord());
    (record.suiteEvidence as { terminalStreamOutcome: string | null }).terminalStreamOutcome = 'malformed';
    expectError(record, 'suiteEvidence.terminalStreamOutcome: contradicts outcome.stream');
  });

  it('rejects runtimeCrashed contradicting outcome.crash', () => {
    const record = asCancellationRestart(makeValidRecord());
    (record.suiteEvidence as { runtimeCrashed: boolean | null }).runtimeCrashed = true;
    expectError(record, 'suiteEvidence.runtimeCrashed: contradicts outcome.crash');
  });

  it('rejects restartRecovery that the restart evidence does not derive', () => {
    const record = asCancellationRestart(makeValidRecord());
    const evidence = record.suiteEvidence as { restartHealthy: boolean | null; followUpPassed: boolean | null };
    evidence.restartHealthy = true;
    evidence.followUpPassed = false;
    record.outcome.restartRecovery = true;
    expectError(record, 'outcome.restartRecovery: contradicts suiteEvidence restart fields');
  });

  it('rejects restartRecovery: true when restart evidence is unproven', () => {
    const record = asCancellationRestart(makeValidRecord());
    record.outcome.restartRecovery = true;
    expectError(record, 'outcome.restartRecovery: true although suiteEvidence restart fields do not prove recovery');
  });

  it('rejects toolCallValid disagreeing with the recorded tool calls', () => {
    const record = asToolCallingAgentLoop(makeValidRecord());
    (record.suiteEvidence as { toolCalls: Array<{ index: number; tool: string; valid: boolean; allowed: boolean }> })
      .toolCalls[1] = { index: 1, tool: 'propose_diff', valid: false, allowed: true };
    expectError(record, 'outcome.toolCallValid: contradicts suiteEvidence.toolCalls');
  });

  it('rejects toolCallValid: null when calls were attempted', () => {
    const record = asToolCallingAgentLoop(makeValidRecord());
    record.outcome.toolCallValid = null;
    expectError(record, 'outcome.toolCallValid: null although suiteEvidence.toolCalls records attempted calls');
  });

  it('rejects correctFileDiscovery disagreeing with the path verdicts', () => {
    const record = asMultiFileNavigation(makeValidRecord());
    (record.suiteEvidence as { missingRequiredPaths: string[] }).missingRequiredPaths = ['src/missed.ts'];
    expectError(record, 'outcome.correctFileDiscovery: contradicts suiteEvidence path verdicts');
  });

  it('accepts agreeing duplicated values (no false positives)', () => {
    const record = makeValidRecord();
    record.outcome.validDiff = false;
    record.outcome.patchApplySuccess = false;
    record.outcome.verificationSuccess = false;
    record.outcome.finalTaskSuccess = false;
    record.outcome.status = 'failed';
    const evidence = record.suiteEvidence as {
      diffValid: boolean | null;
      applySucceeded: boolean | null;
      verifierSucceeded: boolean | null;
    };
    evidence.diffValid = false;
    evidence.applySucceeded = false;
    evidence.verifierSucceeded = false;
    expectValid(record);
  });
});

describe('suite scoping of outcome metrics', () => {
  it('rejects a diff verdict on a suite that cannot exercise one', () => {
    const record = asShortChat(makeValidRecord());
    record.outcome.validDiff = true;
    expectError(record, 'outcome.validDiff: must be null for suite "short-chat"');
  });

  it('rejects cancellation latency outside the cancellation suite', () => {
    const record = asCodeExplanation(makeValidRecord());
    record.outcome.cancellationLatencyMs = 10;
    expectError(record, 'outcome.cancellationLatencyMs: must be null for suite "code-explanation"');
  });
});

describe('schema version handling', () => {
  it('refuses a newer major version instead of guessing', () => {
    const record = makeValidRecord() as unknown as Record<string, unknown>;
    record['schemaVersion'] = 2;
    expectError(record, 'newer than supported');
    const reader = validateRecord(record, 'reader');
    expect(reader.ok).toBe(false);
  });

  it('rejects a non-positive version', () => {
    const record = makeValidRecord() as unknown as Record<string, unknown>;
    record['schemaVersion'] = 0;
    expectError(record, 'schemaVersion: must be a positive integer');
  });
});

describe('unknown and missing fields', () => {
  it('producer rejects an unversioned top-level field; reader only warns', () => {
    const record = makeValidRecord() as unknown as Record<string, unknown>;
    record['vibes'] = 'immaculate';
    expectError(record, 'vibes: not a documented top-level field');
    const reader = validateRecord(record, 'reader');
    expect(reader.ok).toBe(true);
    expect(reader.warnings.join('\n')).toContain('vibes: unknown top-level field');
  });

  it('rejects a missing required top-level field in both modes', () => {
    const record = makeValidRecord() as unknown as Record<string, unknown>;
    delete record['resources'];
    expectError(record, 'resources: required top-level field missing');
    expect(validateRecord(record, 'reader').ok).toBe(false);
  });

  it('rejects suiteEvidence with an extra, missing, or mismatched-kind field', () => {
    const extra = makeValidRecord() as unknown as { suiteEvidence: Record<string, unknown> };
    extra.suiteEvidence['sneaky'] = 1;
    expectError(extra, 'suiteEvidence.sneaky: not a documented field');

    const missing = makeValidRecord() as unknown as { suiteEvidence: Record<string, unknown> };
    delete missing.suiteEvidence['rollbackSucceeded'];
    expectError(missing, 'suiteEvidence.rollbackSucceeded: required for kind');

    const mismatched = makeValidRecord();
    mismatched.suite.id = 'short-chat';
    expectError(mismatched, 'suiteEvidence.kind: must equal suite.id');
  });
});

describe('run block rules', () => {
  it('enforces population/coldMethod coupling both ways', () => {
    const cold = makeValidRecord();
    cold.run.population = 'cold';
    expectError(cold, 'run.coldMethod: required for cold attempts');

    const warm = makeValidRecord();
    warm.run.coldMethod = 'processRestart';
    expectError(warm, 'run.coldMethod: must be null for warm attempts');
  });

  it('enforces repetition bounds', () => {
    const low = makeValidRecord();
    low.run.plannedRepetitions = 2;
    expectError(low, 'run.plannedRepetitions: must be 3..30');

    const high = makeValidRecord();
    high.run.plannedRepetitions = 31;
    expectError(high, 'run.plannedRepetitions: must be 3..30');

    const over = makeValidRecord();
    over.run.repetition = 6;
    expectError(over, 'run.repetition: must be 1..plannedRepetitions');
  });

  it('rejects malformed ids and timestamps', () => {
    const badId = makeValidRecord();
    badId.run.id = 'has spaces';
    expectError(badId, 'run.id: must be ASCII');

    const badTs = makeValidRecord();
    badTs.run.timestampUtc = '2026-07-11 12:00:00';
    expectError(badTs, 'run.timestampUtc: must be an RFC 3339 UTC timestamp');
  });
});

describe('exclusion accounting', () => {
  it('requires a reason exactly when excluded', () => {
    const excluded = makeValidRecord();
    excluded.includeInSummary = false;
    expectError(excluded, 'exclusionReason: an excluded attempt requires a non-null reason');

    const included = makeValidRecord();
    included.exclusionReason = 'deliberate-cancellation';
    expectError(included, 'exclusionReason: must be null when includeInSummary is true');
  });
});

describe('artifact reference grammar', () => {
  it('accepts a clean reference under the allowed root', () => {
    expect(artifactRefError('benchmark-artifacts/run-01/stream.log')).toBeNull();
  });

  it('rejects grammar violations', () => {
    expect(artifactRefError('/etc/passwd')).toContain('absolute');
    expect(artifactRefError('benchmark-artifacts/../secrets')).toContain('".." components');
    expect(artifactRefError('other-root/file')).toContain('must start with benchmark-artifacts/');
    expect(artifactRefError('benchmark-artifacts')).toContain('inside the artifact root');
    expect(artifactRefError('benchmark-artifacts\\file')).toContain('backslashes');
    expect(artifactRefError('benchmark-artifacts/.hidden')).toContain('violates the artifact path grammar');
  });

  it('caps the reference list at 16', () => {
    const record = makeValidRecord();
    record.artifacts = Array.from({ length: 17 }, (_, i) => `benchmark-artifacts/run/${i}.log`);
    expectError(record, 'artifacts: at most 16 references');
  });
});

describe('cross-field rules', () => {
  it('requires sampling.maxOutputTokens to match the context reserve', () => {
    const record = makeValidRecord();
    record.model.sampling.maxOutputTokens = 256;
    expectError(record, 'model.sampling.maxOutputTokens: must match the reserved value');
  });

  it('forbids token counts and rates when countSource is unavailable', () => {
    const record = makeValidRecord();
    record.tokens.countSource = 'unavailable';
    expectError(record, 'tokens.finalAssembledPromptTokens: must be null when countSource is unavailable');
    expectError(record, 'timing.generationTokensPerSecond: must be null when tokens.countSource is unavailable');
  });

  it('requires the configured limit on a timed-out outcome', () => {
    const record = makeValidRecord();
    record.outcome.timeout = true;
    record.outcome.timeoutLimitMs = null;
    expectError(record, 'outcome.timeoutLimitMs: a timeout means the configured limit was reached');
  });

  it('allows a negative swap delta but rejects a fractional one', () => {
    const negative = makeValidRecord();
    negative.resources.swapDeltaBytes = -4096;
    expectValid(negative);

    const fractional = makeValidRecord();
    fractional.resources.swapDeltaBytes = 0.5;
    expectError(fractional, 'resources.swapDeltaBytes: must be a finite signed integer or null');
  });
});

describe('serialization bounds', () => {
  it('refuses to serialize a record past 64 KiB instead of truncating', () => {
    const record = makeValidRecord();
    record.artifacts = ['benchmark-artifacts/' + 'a'.repeat(490)];
    // Bound-respecting artifacts alone cannot exceed 64 KiB; force it
    // with a bloated (invalid) field to prove serialize refuses.
    (record as unknown as Record<string, unknown>)['bloat'] = 'x'.repeat(70000);
    expect(() => serializeRecord(record)).toThrow(/64|65536/);
  });

  it('parseRecordLine refuses an oversized line and reports bad JSON', () => {
    const oversized = parseRecordLine('"' + 'x'.repeat(70000) + '"');
    expect('error' in oversized && oversized.error).toContain('caps a serialized record');

    const malformed = parseRecordLine('{nope');
    expect('error' in malformed && malformed.error).toContain('not valid JSON');
  });
});
