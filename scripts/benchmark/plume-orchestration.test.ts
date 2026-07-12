// @vitest-environment node
//
// D129C: deterministic tests for the plumeOrchestration measurement
// path and the Plume patch-validator bridge — ALL against fakes, per
// the commission: a protocol-faithful fake plume_bench sidecar, a
// stub interpreter/server, and a scripted fake patch-check. Nothing
// here needs cargo, a model, or mlx-lm. The REAL sidecar is covered
// by its cargo unit tests and the live paired smoke.

import { chmodSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';

import { digestModelDir } from './model-identity.ts';
import { NULL_READINGS } from './resource-probes.ts';
import { exerciseDiff } from './oracles.ts';
import { loadHarnessConfig, runOne } from './run-model.ts';
import type { HarnessConfig } from './run-model.ts';
import { resolveRuntime } from './runtime-factory.ts';
import { fakeConfig, fixtureDir, withPlumeEnv } from './test-support.ts';
import { loadFixture } from './fixtures.ts';

const dir = mkdtempSync(path.join(os.tmpdir(), 'plume-orch-'));
const outDir = path.join(dir, 'records');
afterAll(() => rmSync(dir, { recursive: true, force: true }));

// ---- fakes ------------------------------------------------------------

// A fake checkpoint the identity checks digest.
const modelDir = path.join(dir, 'model');
const { mkdirSync } = await import('node:fs');
mkdirSync(modelDir);
mkdirSync(outDir);
writeFileSync(path.join(modelDir, 'config.json'), '{"model_type":"synthetic"}');
writeFileSync(path.join(modelDir, 'model.safetensors'), 'tiny fake weights');
const realDigest = digestModelDir(modelDir);

// Stub interpreter/server in one: answers the version probe (`-c`)
// with 9.9.9, otherwise serves HTTP 200 on the appended --port (the
// managed-server health poll).
const stubServer = path.join(dir, 'python-stub.sh');
writeFileSync(
  stubServer,
  `#!/bin/sh
case "$*" in
  *" -c "*|"-c "*) echo "9.9.9"; exit 0;;
esac
for arg in "$@"; do port="$arg"; done
exec "${process.execPath}" -e "require('http').createServer((q, s) => s.end('ok')).listen(process.argv[1], '127.0.0.1')" "$port"
`,
);
chmodSync(stubServer, 0o755);

// Protocol-faithful fake plume_bench: health handshake with a cap of
// 4096, orchestrate session loop replying "pong" with a fixed report.
const fakeSidecar = path.join(dir, 'fake-plume-bench.mjs');
writeFileSync(
  fakeSidecar,
  `#!/usr/bin/env node
import readline from 'node:readline';
const args = process.argv.slice(2);
if (args[0] === 'identity' || (args[0] === 'orchestrate' && args.includes('--health'))) {
  // The fake DECLARES its fixture identity explicitly (test env);
  // with none declared it reports null and the harness refuses —
  // an arbitrary binary never qualifies as Plume.
  const { readFileSync } = await import('node:fs');
  const shaFile = process.env.PLUME_FAKE_SIDECAR_SHA_FILE;
  const fileSha = shaFile ? readFileSync(shaFile, 'utf8').trim() : null;
  const sha = process.env.PLUME_FAKE_SIDECAR_SHA ?? fileSha ?? process.env.PLUME_BENCH_GIT_SHA ?? null;
  const dirtyRaw = process.env.PLUME_FAKE_SIDECAR_DIRTY ?? process.env.PLUME_BENCH_DIRTY ?? null;
  console.log(JSON.stringify({
    ok: true,
    gitSha: sha,
    dirty: dirtyRaw === null ? null : dirtyRaw === 'true',
    maxOutputTokens: 4096,
  }));
  process.exit(0);
}
if (args[0] === 'orchestrate') {
  let requestIndex = 0;
  const rl = readline.createInterface({ input: process.stdin });
  rl.on('line', (line) => {
    let frame;
    try { frame = JSON.parse(line); } catch { return; }
    if (frame.type !== 'generate') return;
    console.log(JSON.stringify({ type: 'token', text: 'po' }));
    console.log(JSON.stringify({ type: 'token', text: 'ng' }));
    console.log(JSON.stringify({ type: 'done', report: {
      promptTokens: 24, outputTokens: 3, ttftMs: 5.5,
      generationDurationMs: 4.5, endToEndMs: 12.0, requestIndex,
    } }));
    requestIndex += 1;
  });
} else {
  process.exit(2);
}
`,
);
chmodSync(fakeSidecar, 0o755);

interface PlumeConfigOverrides {
  binary?: string;
  temperature?: number | null;
  maxOutputTokens?: number;
}

function plumeConfig(overrides?: PlumeConfigOverrides): HarnessConfig {
  const base = fakeConfig('short-chat-pass');
  return {
    ...base,
    measurementPath: 'plumeOrchestration',
    plumeBench: { binary: overrides?.binary ?? fakeSidecar },
    runtime: {
      path: 'plume-mlx-lm',
      name: 'mlx-lm',
      version: null,
      engine: 'mlx-lm',
      backend: 'MLX',
      transport: 'openai-sse',
      server: {
        command: [stubServer, '-m', 'mlx_lm', 'server', '--model', modelDir],
        modelDir,
        startupTimeoutMs: 10_000,
      },
      configuration: base.runtime.configuration,
    },
    model: {
      ...base.model,
      artifact: { ...base.model.artifact, format: 'mlx', sha256: realDigest },
      context: { pointTokens: 8192, configuredTokens: 8192, acceptedTokens: null, maxOutputTokens: 4096 },
      sampling: {
        // Plume's real posture: no client sampling controls, the
        // product's own explicit output cap.
        temperature: overrides?.temperature !== undefined ? overrides.temperature : null,
        topP: null,
        topK: null,
        minP: null,
        repeatPenalty: null,
        seed: null,
        maxOutputTokens: overrides?.maxOutputTokens ?? 4096,
        stopSequences: [],
      },
    },
  };
}

// ---- config loader ------------------------------------------------------

describe('loadHarnessConfig (plumeOrchestration)', () => {
  let counter = 0;
  function writeConfig(config: unknown): string {
    counter += 1;
    const file = path.join(dir, `config-${counter}.json`);
    writeFileSync(file, JSON.stringify(config));
    return file;
  }

  it('accepts a plumeOrchestration config with a sidecar and openai-sse transport', () => {
    const loaded = loadHarnessConfig(writeConfig(plumeConfig()));
    expect(loaded.measurementPath).toBe('plumeOrchestration');
  });

  it('refuses plumeOrchestration without the sidecar binary', () => {
    const config = plumeConfig() as unknown as Record<string, unknown>;
    delete config['plumeBench'];
    expect(() => loadHarnessConfig(writeConfig(config))).toThrow(/plumeBench\.binary/);
  });

  it('refuses plumeOrchestration over a non-managed transport', () => {
    const config = plumeConfig();
    config.runtime = { ...fakeConfig('short-chat-pass').runtime };
    expect(() => loadHarnessConfig(writeConfig(config))).toThrow(/openai-sse/);
  });

  it('refuses an unknown measurement path', () => {
    const config = { ...plumeConfig(), measurementPath: 'both-at-once' };
    expect(() => loadHarnessConfig(writeConfig(config))).toThrow(/measurementPath/);
  });
});

// ---- factory posture verification ---------------------------------------

describe('resolveRuntime (plumeOrchestration posture + provenance)', () => {
  it('refuses a declared sampling control Plume does not send', async () => {
    await expect(withPlumeEnv(() => resolveRuntime(plumeConfig({ temperature: 0.0 })))).rejects.toThrow(
      /cannot honor sampling\.temperature/,
    );
  });

  it('refuses an output cap that is not what Plume actually sends', async () => {
    await expect(withPlumeEnv(() => resolveRuntime(plumeConfig({ maxOutputTokens: 64 })))).rejects.toThrow(
      /output cap mismatch/,
    );
  });

  it('refuses a missing sidecar binary at resolve time', async () => {
    const config = plumeConfig();
    delete (config as { plumeBench?: unknown }).plumeBench;
    await expect(resolveRuntime(config)).rejects.toThrow(/plumeBench\.binary/);
  });

  it('resolves with runtimeReported timing and probe support', async () => {
    const resolved = await withPlumeEnv(() => resolveRuntime(plumeConfig()));
    expect(resolved.timingMethod).toBe('runtimeReported');
    expect(resolved.supportsResourceProbes).toBe(true);
    expect(resolved.block.version).toBe('9.9.9');
    expect(resolved.block.transport).toBe('openai-sse');
  });

  it('refuses a sidecar built from a different commit than the records will carry', async () => {
    process.env['PLUME_FAKE_SIDECAR_SHA'] = 'f'.repeat(40);
    try {
      await expect(withPlumeEnv(() => resolveRuntime(plumeConfig()))).rejects.toThrow(
        /plume_bench identity mismatch/,
      );
    } finally {
      delete process.env['PLUME_FAKE_SIDECAR_SHA'];
    }
  });

  it('refuses a dirty-built sidecar when records would claim a clean checkout', async () => {
    process.env['PLUME_FAKE_SIDECAR_DIRTY'] = 'true';
    try {
      await expect(withPlumeEnv(() => resolveRuntime(plumeConfig()))).rejects.toThrow(
        /plume_bench identity mismatch/,
      );
    } finally {
      delete process.env['PLUME_FAKE_SIDECAR_DIRTY'];
    }
  });

  it('refuses an attempt whose identity drifted from the session launch identity', async () => {
    // The session's sidecar is verified under the fixture identity;
    // then a "commit" happens (the identity changes) before the next
    // attempt. Mixing the two in one suite must refuse.
    const session = await withPlumeEnv(async () => (await resolveRuntime(plumeConfig())).createSession());
    process.env['PLUME_BENCH_GIT_SHA'] = 'a'.repeat(40);
    process.env['PLUME_BENCH_DIRTY'] = 'false';
    try {
      await expect(
        runOne({
          config: plumeConfig(),
          fixtureDir: fixtureDir('short-chat', 'pong-001'),
          population: 'warm',
          repetition: 1,
          plannedRepetitions: 3,
          outFile: path.join(outDir, 'drift.jsonl'),
          timestampUtc: '2026-07-12T12:00:00Z',
          session,
        }),
      ).rejects.toThrow(/identity drifted mid-suite/);
    } finally {
      delete process.env['PLUME_BENCH_GIT_SHA'];
      delete process.env['PLUME_BENCH_DIRTY'];
      await session.close();
    }
  });

  it('refuses a rebuild landing during the model load (verify-before-spawn ordering)', async () => {
    // The fake sidecar's identity comes from a file; a "rebuilding"
    // server stub overwrites that file DURING startup — the window
    // where a real model load takes minutes. Verification must
    // happen after the server is up, immediately before the sidecar
    // spawn, so this rebuild refuses instead of slipping in.
    const shaFile = path.join(dir, 'sidecar-sha.txt');
    writeFileSync(shaFile, '0123456789abcdef0123456789abcdef01234567');
    const rebuildingServer = path.join(dir, 'rebuilding-server.sh');
    writeFileSync(
      rebuildingServer,
      `#!/bin/sh
case "$*" in
  *" -c "*|"-c "*) echo "9.9.9"; exit 0;;
esac
printf '${'f'.repeat(40)}' > "${shaFile}"
for arg in "$@"; do port="$arg"; done
exec "${process.execPath}" -e "require('http').createServer((q, s) => s.end('ok')).listen(process.argv[1], '127.0.0.1')" "$port"
`,
    );
    chmodSync(rebuildingServer, 0o755);
    const config = plumeConfig();
    if (config.runtime.server === undefined) throw new Error('test config must carry a server block');
    config.runtime.server.command = [rebuildingServer, '-m', 'mlx_lm', 'server', '--model', modelDir];
    process.env['PLUME_FAKE_SIDECAR_SHA_FILE'] = shaFile;
    try {
      await expect(
        withPlumeEnv(async () => (await resolveRuntime(config)).createSession()),
      ).rejects.toThrow(/plume_bench identity mismatch/);
    } finally {
      delete process.env['PLUME_FAKE_SIDECAR_SHA_FILE'];
    }
  });

  it('refuses a sidecar with no verifiable build identity at all', async () => {
    // No fixture identity declared: the fake reports null, exactly
    // like a git-less build. The harness must not label it as Plume.
    await expect(resolveRuntime(plumeConfig())).rejects.toThrow(/no verifiable build identity/);
  });
});

// ---- runOne: a full deterministic plumeOrchestration record -------------

describe('runOne (plumeOrchestration, fake sidecar)', () => {
  it('produces a valid path-separated record with sidecar-reported timing', async () => {
    const record = await withPlumeEnv(() =>
      runOne({
        config: plumeConfig(),
        fixtureDir: fixtureDir('short-chat', 'pong-001'),
        population: 'warm',
        repetition: 1,
        plannedRepetitions: 3,
        outFile: path.join(outDir, 'plume-warm.jsonl'),
        timestampUtc: '2026-07-12T12:00:00Z',
        pairId: 'pair_test_001',
      }),
    );
    expect(record.run.measurementPath).toBe('plumeOrchestration');
    expect(record.run.pairId).toBe('pair_test_001');
    // The record carries the attempt-pinned identity snapshot.
    expect(record.plume).toEqual({ gitSha: '0123456789abcdef0123456789abcdef01234567', dirty: false });
    expect(record.outcome.status).toBe('passed'); // fake sidecar answers pong
    expect(record.timing.method).toBe('runtimeReported');
    expect(record.timing.timeToFirstTokenMs).toBe(5.5);
    expect(record.timing.endToEndMs).toBe(12.0);
    expect(record.timing.generationTokensPerSecond).toBe(3 / (4.5 / 1000));
    expect(record.timing.promptEvaluationMs).toBeNull();
    expect(record.timing.promptTokensPerSecond).toBeNull();
    expect(record.tokens.finalAssembledPromptTokens).toBe(24);
    expect(record.tokens.outputTokens).toBe(3);
    expect(record.runtime.version).toBe('9.9.9');
    // Plume's posture on the record: no client sampling controls.
    expect(record.model.sampling.temperature).toBeNull();
    expect(record.model.sampling.maxOutputTokens).toBe(4096);
  });
});

// ---- runOne: patch-check provenance (rawRuntime + plumeBench) ------------

describe('runOne verifies the patch-check sidecar identity', () => {
  function bugFixConfig(): HarnessConfig {
    return { ...fakeConfig('bug-fix-pass'), plumeBench: { binary: fakeSidecar } };
  }
  const runBugFix = (): Promise<Awaited<ReturnType<typeof runOne>>> =>
    runOne({
      config: bugFixConfig(),
      fixtureDir: fixtureDir('single-file-bug-fix', 'bug-001'),
      population: 'warm',
      repetition: 1,
      plannedRepetitions: 3,
      outFile: path.join(outDir, 'bugfix-provenance.jsonl'),
      timestampUtc: '2026-07-12T12:00:00Z',
    });

  it('refuses a mismatched sidecar at the moment of use', async () => {
    // Verification happens immediately before the patch-check spawn
    // (judge time), against the identity pinned at attempt start.
    process.env['PLUME_FAKE_SIDECAR_SHA'] = 'f'.repeat(40);
    try {
      await expect(withPlumeEnv(runBugFix)).rejects.toThrow(/plume_bench identity mismatch/);
    } finally {
      delete process.env['PLUME_FAKE_SIDECAR_SHA'];
    }
  });

  it('produces a record through the bridge when the identity matches', async () => {
    // The fake sidecar has no patch-check mode, so the bridge fails
    // AFTER identity verification passes: the record exists with
    // null diff mechanics — provenance gates the run, a broken
    // bridge only nulls the evidence.
    const record = await withPlumeEnv(runBugFix);
    expect(record.run.measurementPath).toBe('rawRuntime');
    expect(record.outcome.validDiff).toBeNull();
  });
});

// ---- record identity is the attempt snapshot ------------------------------

describe('runOne pins one identity per attempt', () => {
  it('keeps the attempt snapshot even when the identity changes mid-run', async () => {
    // A "commit lands" while the request is in flight (simulated via
    // the sampler seam, which runs between attempt start and record
    // assembly). The record must carry the identity pinned at attempt
    // start — never a later recompute.
    const record = await withPlumeEnv(() =>
      runOne({
        config: fakeConfig('short-chat-pass'),
        fixtureDir: fixtureDir('short-chat', 'fact-001'),
        population: 'warm',
        repetition: 1,
        plannedRepetitions: 3,
        outFile: path.join(outDir, 'snapshot.jsonl'),
        timestampUtc: '2026-07-12T12:00:00Z',
        samplerFactory: async () => {
          process.env['PLUME_BENCH_GIT_SHA'] = 'b'.repeat(40);
          return { stop: async () => NULL_READINGS };
        },
      }),
    );
    expect(record.plume.gitSha).toBe('0123456789abcdef0123456789abcdef01234567');
  });
});

// ---- oracle: Plume patch-check bridge ------------------------------------

const fakePatchCheck = path.join(dir, 'fake-patch-check.mjs');
writeFileSync(
  fakePatchCheck,
  `#!/usr/bin/env node
if (process.argv[2] === 'identity') {
  const sha = process.env.PLUME_FAKE_SIDECAR_SHA ?? process.env.PLUME_BENCH_GIT_SHA ?? null;
  const dirtyRaw = process.env.PLUME_FAKE_SIDECAR_DIRTY ?? process.env.PLUME_BENCH_DIRTY ?? null;
  console.log(JSON.stringify({ ok: true, gitSha: sha, dirty: dirtyRaw === null ? null : dirtyRaw === 'true', maxOutputTokens: 4096 }));
  process.exit(0);
}
let input = '';
process.stdin.on('data', (c) => { input += c; });
process.stdin.on('end', () => {
  const request = JSON.parse(input);
  if (request.diff.includes('BRIDGE_BROKEN')) process.exit(3);
  if (request.diff.includes('INVALID')) {
    console.log(JSON.stringify({ ok: true, valid: false, applied: null }));
  } else if (request.diff.includes('NOAPPLY')) {
    console.log(JSON.stringify({ ok: true, valid: true, applied: false }));
  } else {
    console.log(JSON.stringify({ ok: true, valid: true, applied: true }));
  }
});
`,
);
chmodSync(fakePatchCheck, 0o755);

describe('exerciseDiff with the Plume patch-check bridge', () => {
  const bugFixDir = fixtureDir('single-file-bug-fix', 'bug-001');
  const manifest = loadFixture(bugFixDir).manifest;
  const FIXTURE_IDENTITY = { gitSha: '0123456789abcdef0123456789abcdef01234567', dirty: false };
  const mechanics = { patchCheck: [fakePatchCheck, 'patch-check'], expectedIdentity: FIXTURE_IDENTITY };
  beforeAll(() => {
    process.env['PLUME_BENCH_GIT_SHA'] = FIXTURE_IDENTITY.gitSha;
    process.env['PLUME_BENCH_DIRTY'] = 'false';
  });
  afterAll(() => {
    delete process.env['PLUME_BENCH_GIT_SHA'];
    delete process.env['PLUME_BENCH_DIRTY'];
  });
  // A diff shaped well enough for target-path extraction; the FAKE
  // bridge decides verdicts by marker, so content is irrelevant.
  const diff = (marker: string): string =>
    `--- a/src/thing.py\n+++ b/src/thing.py\n@@ -1,1 +1,1 @@\n-x ${marker}\n+y\n`;

  it("records Plume's verdicts, not git's", () => {
    const result = exerciseDiff(bugFixDir, manifest, diff('VALID_APPLIES'), mechanics);
    expect(result.diffValid).toBe(true);
    expect(result.applySucceeded).toBe(true);
  });

  it('keeps apply null when Plume declares the diff invalid', () => {
    const result = exerciseDiff(bugFixDir, manifest, diff('INVALID'), mechanics);
    expect(result.diffValid).toBe(false);
    expect(result.applySucceeded).toBeNull();
  });

  it('records a Plume pre-image refusal as apply failure', () => {
    const result = exerciseDiff(bugFixDir, manifest, diff('NOAPPLY'), mechanics);
    expect(result.diffValid).toBe(true);
    expect(result.applySucceeded).toBe(false);
  });

  it('records null mechanics (not false) when the bridge itself breaks', () => {
    const result = exerciseDiff(bugFixDir, manifest, diff('BRIDGE_BROKEN'), mechanics);
    expect(result.diffValid).toBeNull();
    expect(result.applySucceeded).toBeNull();
  });

  it('refuses when the bridge identity mismatches at the moment of use', () => {
    process.env['PLUME_FAKE_SIDECAR_SHA'] = 'f'.repeat(40);
    try {
      expect(() => exerciseDiff(bugFixDir, manifest, diff('VALID_APPLIES'), mechanics)).toThrow(
        /plume_bench identity mismatch/,
      );
    } finally {
      delete process.env['PLUME_FAKE_SIDECAR_SHA'];
    }
  });

  it('refuses a patchCheck command without a pinned identity', () => {
    expect(() =>
      exerciseDiff(bugFixDir, manifest, diff('VALID_APPLIES'), { patchCheck: [fakePatchCheck, 'patch-check'] }),
    ).toThrow(/requires expectedIdentity/);
  });

  it('uses ONLY Plume’s verdict — the lexical screen is bypassed', () => {
    // A backslash path fails the git-path lexical screen but the
    // bridge says valid: parity means Plume’s verdict wins.
    const weird = '--- a/src\\thing.py\n+++ b/src\\thing.py\n@@ -1,1 +1,1 @@\n-x\n+y\n';
    const viaBridge = exerciseDiff(bugFixDir, manifest, weird, mechanics);
    expect(viaBridge.diffValid).toBe(true);
    const viaGit = exerciseDiff(bugFixDir, manifest, weird);
    expect(viaGit.diffValid).toBe(false);
  });
});
