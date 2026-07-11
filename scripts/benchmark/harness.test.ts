// D129: end-to-end harness tests — every suite runs one real
// invocation against the scripted fake runtime, and every emitted
// record must survive producer validation (which includes the
// contradiction rules). No model, no network, no ports: the runtime
// is a local node subprocess speaking stdio JSON.

import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterAll, describe, expect, it } from 'vitest';

import { loadHarnessConfig, runOne } from './run-model.ts';
import type { RunOneOptions } from './run-model.ts';
import { loadFixture } from './fixtures.ts';
import { casePath, fakeConfig, fixtureDir, withPlumeEnv } from './test-support.ts';
import { validateRecord } from './validate.ts';
import type { BenchmarkRecord } from './types.ts';

const outDir = mkdtempSync(path.join(os.tmpdir(), 'plume-bench-test-'));
afterAll(() => rmSync(outDir, { recursive: true, force: true }));

let fileCounter = 0;
async function record(caseName: string, suite: string, caseId: string, extra?: Partial<RunOneOptions>): Promise<BenchmarkRecord> {
  fileCounter += 1;
  const outFile = path.join(outDir, `records-${fileCounter}.jsonl`);
  return withPlumeEnv(() =>
    runOne({
      config: fakeConfig(caseName),
      fixtureDir: fixtureDir(suite, caseId),
      population: 'warm',
      repetition: 1,
      plannedRepetitions: 3,
      outFile,
      timestampUtc: '2026-07-11T12:00:00Z',
      ...extra,
    }),
  );
}

describe('short-chat', () => {
  it('records an exact-match pass with runtime-reported timing', async () => {
    const r = await record('short-chat-pass', 'short-chat', 'fact-001');
    expect(r.outcome.status).toBe('passed');
    expect(r.suiteEvidence).toMatchObject({ kind: 'short-chat', replyClassification: 'exact-match', terminalStreamOutcome: 'completed' });
    expect(r.tokens.countSource).toBe('runtimeReported');
    expect(r.timing.method).toBe('runtimeReported');
    expect(r.timing.generationTokensPerSecond).toBe(3 / (1.5 / 1000));
    expect(r.model.context.acceptedTokens).toBe(4096);
    expect(r.suite.fixtureDigest).toMatch(/^sha256:[0-9a-f]{64}$/);
  });

  it('records a mismatch as failed, not as an error', async () => {
    const r = await record('short-chat-wrong', 'short-chat', 'fact-001');
    expect(r.outcome.status).toBe('failed');
    expect(r.suiteEvidence).toMatchObject({ replyClassification: 'mismatch' });
    expect(r.outcome.finalTaskSuccess).toBe(false);
  });

  it('records a malformed stream with nulled metrics', async () => {
    const r = await record('short-chat-malformed', 'short-chat', 'fact-001');
    expect(r.outcome.status).toBe('error');
    expect(r.outcome.stream).toBe('malformed');
    expect(r.outcome.errorClass).toBe('malformed-stream');
    expect(r.tokens.countSource).toBe('unavailable');
    expect(r.timing.method).toBe('unavailable');
    expect(r.timing.endToEndMs).toBeNull();
  });

  it('records a hang as timedOut at the fixture limit', async () => {
    const r = await record('short-chat-hang', 'short-chat', 'fact-001');
    expect(r.outcome.status).toBe('timedOut');
    expect(r.outcome.timeout).toBe(true);
    expect(r.outcome.timeoutLimitMs).toBe(1200);
    expect(r.outcome.stream).toBe('timedOut');
  });
});

describe('long-context-retrieval', () => {
  it('records retrieved keys and no decoys on a pass', async () => {
    const r = await record('long-context-pass', 'long-context-retrieval', 'keys-001');
    expect(r.outcome.status).toBe('passed');
    expect(r.suiteEvidence).toMatchObject({
      retrievedKeys: ['harbor-master', 'ledger-total'],
      missingKeys: [],
      incorrectDecoyKeys: [],
      truncated: false,
      requestedContextTokens: 4096,
      finalAssembledPromptTokens: 1800,
    });
  });

  it('fails when a decoy is asserted as a fact', async () => {
    const r = await record('long-context-decoy', 'long-context-retrieval', 'keys-001');
    expect(r.outcome.status).toBe('failed');
    expect(r.suiteEvidence).toMatchObject({
      missingKeys: ['harbor-master'],
      incorrectDecoyKeys: ['decoy-captain'],
    });
  });
});

describe('code-explanation', () => {
  it('records per-rubric verdicts', async () => {
    const r = await record('code-explain-pass', 'code-explanation', 'explain-001');
    expect(r.outcome.status).toBe('passed');
    expect(r.suiteEvidence).toMatchObject({
      rubricItems: [
        { id: 'names-the-off-by-one', passed: true },
        { id: 'mentions-undefined-result', passed: true },
        { id: 'no-recursion-claim', passed: true },
      ],
    });
  });
});

describe('single-file-bug-fix', () => {
  it('validates, applies, verifies, and rolls back a good diff', async () => {
    const r = await record('bug-fix-pass', 'single-file-bug-fix', 'bug-001');
    expect(r.outcome.status).toBe('passed');
    expect(r.suiteEvidence).toMatchObject({
      targetFile: 'src/counter.ts',
      diffValid: true,
      applySucceeded: true,
      verifierSucceeded: true,
      rollbackSucceeded: true,
    });
    expect(r.outcome.validDiff).toBe(true);
    expect(r.outcome.patchApplySuccess).toBe(true);
    expect(r.outcome.verificationSuccess).toBe(true);
  });

  it('records prose instead of a diff as invalid, later steps null', async () => {
    const r = await record('bug-fix-invalid-diff', 'single-file-bug-fix', 'bug-001');
    expect(r.outcome.status).toBe('failed');
    expect(r.suiteEvidence).toMatchObject({ diffValid: false, applySucceeded: null, verifierSucceeded: null });
  });

  it('leaves the committed fixture untouched (disposable copy only)', async () => {
    await record('bug-fix-pass', 'single-file-bug-fix', 'bug-001');
    const pristine = readFileSync(
      path.join(fixtureDir('single-file-bug-fix', 'bug-001'), 'repo', 'src', 'counter.ts'),
      'utf8',
    );
    expect(pristine).toContain('i <= items.length');
    // And the digest still matches, so the next load still runs.
    expect(() => loadFixture(fixtureDir('single-file-bug-fix', 'bug-001'))).not.toThrow();
  });
});

describe('multi-file-navigation', () => {
  it('records discovery and diff mechanics on a pass', async () => {
    const r = await record('nav-pass', 'multi-file-navigation', 'nav-001');
    expect(r.outcome.status).toBe('passed');
    expect(r.suiteEvidence).toMatchObject({
      discoveredPaths: ['src/config.ts', 'src/loader.ts'],
      missingRequiredPaths: [],
      claimedForbiddenPaths: [],
      diffValid: true,
      verifierSucceeded: true,
    });
    expect(r.outcome.correctFileDiscovery).toBe(true);
  });
});

describe('tool-calling-agent-loop', () => {
  it('records per-call validity and the full loop verdict on a pass', async () => {
    const r = await record('loop-pass', 'tool-calling-agent-loop', 'loop-001');
    expect(r.outcome.status).toBe('passed');
    expect(r.outcome.toolCallValid).toBe(true);
    expect(r.suiteEvidence).toMatchObject({
      toolCallLimit: 8,
      toolCalls: [
        { index: 0, tool: 'read_file', valid: true, allowed: true },
        { index: 1, tool: 'propose_diff', valid: true, allowed: true },
      ],
      taskSucceeded: true,
    });
  });

  it('flags a disallowed tool call and fails the loop', async () => {
    const r = await record('loop-invalid-call', 'tool-calling-agent-loop', 'loop-001');
    expect(r.outcome.status).toBe('failed');
    expect(r.outcome.toolCallValid).toBe(false);
    const calls = (r.suiteEvidence as { toolCalls: Array<{ tool: string; valid: boolean; allowed: boolean }> }).toolCalls;
    expect(calls[1]).toMatchObject({ tool: 'delete_file', valid: false, allowed: false });
  });
});

describe('cancellation-restart', () => {
  it('records an acknowledged cancel, excluded from latency summaries', async () => {
    const r = await record('cancel', 'cancellation-restart', 'cancel-001');
    expect(r.outcome.status).toBe('passed');
    expect(r.outcome.stream).toBe('cancelled');
    expect(r.outcome.cancellationLatencyMs).toBe(42.5);
    expect(r.includeInSummary).toBe(false);
    expect(r.exclusionReason).toBe('deliberate-cancellation');
    expect(r.suiteEvidence).toMatchObject({ cancellationLatencyMs: 42.5, runtimeCrashed: false });
  });

  it('records crash → restart health → follow-up as recovery', async () => {
    const r = await record('crash-restart', 'cancellation-restart', 'cancel-001');
    expect(r.outcome.crash).toBe(true);
    expect(r.outcome.stream).toBe('crashed');
    expect(r.outcome.restartRecovery).toBe(true);
    expect(r.outcome.status).toBe('passed');
    expect(r.suiteEvidence).toMatchObject({ runtimeCrashed: true, restartHealthy: true, followUpPassed: true });
  });
});

describe('record integrity', () => {
  it('every emitted record passes producer validation on re-read', async () => {
    fileCounter += 1;
    const outFile = path.join(outDir, `records-${fileCounter}.jsonl`);
    await withPlumeEnv(async () => {
      await runOne({
        config: fakeConfig('short-chat-pass'),
        fixtureDir: fixtureDir('short-chat', 'fact-001'),
        population: 'cold',
        repetition: 1,
        plannedRepetitions: 3,
        outFile,
      });
    });
    const lines = readFileSync(outFile, 'utf8').trim().split('\n');
    expect(lines).toHaveLength(1);
    const reread = validateRecord(JSON.parse(lines[0] ?? ''), 'producer');
    expect(reread.errors).toEqual([]);
    const parsed = JSON.parse(lines[0] ?? '') as BenchmarkRecord;
    expect(parsed.run.population).toBe('cold');
    expect(parsed.run.coldMethod).toBe('processRestart');
  });

  it('refuses a fixture whose content drifted from its digest', async () => {
    const dir = mkdtempSync(path.join(os.tmpdir(), 'plume-bench-drift-'));
    try {
      const source = fixtureDir('short-chat', 'fact-001');
      const manifest = JSON.parse(readFileSync(path.join(source, 'manifest.json'), 'utf8')) as Record<string, unknown>;
      manifest['files'] = ['padding.txt'];
      writeFileSync(path.join(dir, 'manifest.json'), JSON.stringify(manifest));
      writeFileSync(path.join(dir, 'padding.txt'), 'content the digest does not cover');
      await expect(
        runOne({
          config: fakeConfig('short-chat-pass'),
          fixtureDir: dir,
          population: 'warm',
          repetition: 1,
          plannedRepetitions: 3,
          outFile: path.join(dir, 'out.jsonl'),
        }),
      ).rejects.toThrow(/contentDigest mismatch/);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it('rejects a plumeOrchestration config — not implemented, not faked', () => {
    const config = fakeConfig('short-chat-pass') as unknown as { measurementPath: string };
    config.measurementPath = 'plumeOrchestration';
    const dir = mkdtempSync(path.join(os.tmpdir(), 'plume-bench-config-'));
    try {
      const configFile = path.join(dir, 'config.json');
      writeFileSync(configFile, JSON.stringify(config));
      expect(() => loadHarnessConfig(configFile)).toThrow(/plumeOrchestration/);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it('exposes the case scripts the tests rely on', () => {
    // Guard against silently renamed cases: a missing case file would
    // otherwise surface as a confusing crash-shaped test failure.
    for (const name of ['short-chat-pass', 'cancel', 'crash-restart', 'loop-invalid-call']) {
      expect(() => readFileSync(casePath(name))).not.toThrow();
    }
  });
});
