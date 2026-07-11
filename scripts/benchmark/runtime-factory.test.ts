// @vitest-environment node
//
// D129A: runtime-factory refusal tests — verified identity or no run.
// Uses a tiny fake "model dir" and a stub interpreter script, so none
// of this needs mlx-lm or a real checkpoint.

import { chmodSync, mkdtempSync, rmSync, statSync, utimesSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterAll, describe, expect, it } from 'vitest';

import { digestModelDir } from './model-identity.ts';
import { resolveRuntime } from './runtime-factory.ts';
import type { HarnessConfig } from './runtime-factory.ts';
import { fakeConfig } from './test-support.ts';

const dir = mkdtempSync(path.join(os.tmpdir(), 'plume-factory-'));
afterAll(() => rmSync(dir, { recursive: true, force: true }));

// A fake checkpoint (two small files) and a stub "python" that
// answers the version probe with a fixed string.
const modelDir = path.join(dir, 'model');
const { mkdirSync } = await import('node:fs');
mkdirSync(modelDir);
writeFileSync(path.join(modelDir, 'config.json'), '{"model_type":"synthetic"}');
writeFileSync(path.join(modelDir, 'model.safetensors'), 'tiny fake weights');
const stubPython = path.join(dir, 'python');
writeFileSync(stubPython, '#!/bin/sh\necho 9.9.9\n');
chmodSync(stubPython, 0o755);
const realDigest = digestModelDir(modelDir);

function mlxConfig(overrides?: {
  declaredSha?: string;
  declaredVersion?: string | null;
  commandModelDir?: string;
  extraServerArgs?: string[];
  engine?: string;
}): HarnessConfig {
  const base = fakeConfig('short-chat-pass');
  return {
    ...base,
    runtime: {
      path: 'plume-mlx-lm',
      name: 'mlx-lm',
      version: overrides?.declaredVersion ?? null,
      engine: overrides?.engine ?? 'mlx-lm',
      backend: 'MLX',
      transport: 'openai-sse',
      server: {
        command: [
          stubPython,
          '-m',
          'mlx_lm',
          'server',
          '--model',
          overrides?.commandModelDir ?? modelDir,
          ...(overrides?.extraServerArgs ?? []),
        ],
        modelDir,
        startupTimeoutMs: 1000,
      },
      configuration: base.runtime.configuration,
    },
    model: {
      ...base.model,
      artifact: { ...base.model.artifact, format: 'mlx', sha256: overrides?.declaredSha ?? realDigest },
    },
  };
}

describe('resolveRuntime (openai-sse) identity verification', () => {
  it('resolves with the probed engine version filling a null declaration', async () => {
    const resolved = await resolveRuntime(mlxConfig());
    expect(resolved.block.version).toBe('9.9.9');
    expect(resolved.block.engine).toBe('mlx-lm');
    expect(resolved.timingMethod).toBe('clientObserved');
  });

  it('refuses a declared artifact digest the model dir does not hash to', async () => {
    const config = mlxConfig({ declaredSha: 'sha256:' + '0'.repeat(64) });
    await expect(resolveRuntime(config)).rejects.toThrow(/model identity mismatch/);
  });

  it('refuses a declared engine version the interpreter does not serve', async () => {
    const config = mlxConfig({ declaredVersion: '1.2.3' });
    await expect(resolveRuntime(config)).rejects.toThrow(/engine version mismatch/);
  });

  it('refuses a server command whose --model is not the digested directory', async () => {
    const config = mlxConfig({ commandModelDir: path.join(dir, 'other-model') });
    await expect(resolveRuntime(config)).rejects.toThrow(/--model with exactly server.modelDir/);
  });

  it('refuses a duplicate --model flag (argparse would let the later one win)', async () => {
    const config = mlxConfig({ extraServerArgs: ['--model', path.join(dir, 'other-model')] });
    await expect(resolveRuntime(config)).rejects.toThrow(/single --model/);
  });

  it('refuses the --model= form (it would bypass the two-token check)', async () => {
    const config = mlxConfig({ extraServerArgs: [`--model=${path.join(dir, 'other-model')}`] });
    await expect(resolveRuntime(config)).rejects.toThrow(/single --model/);
  });

  it('refuses an openai-sse engine it cannot verify', async () => {
    const config = mlxConfig({ engine: 'llama-cpp' });
    await expect(resolveRuntime(config)).rejects.toThrow(/mlx-lm.*only/);
  });

  it('detects a same-size rewrite with a restored mtime (full re-digest, no stat trust)', async () => {
    const resolved = await resolveRuntime(mlxConfig());
    const weights = path.join(modelDir, 'model.safetensors');
    const before = statSync(weights);
    try {
      // Same byte length as 'tiny fake weights', mtime put back —
      // invisible to any size+mtime fingerprint. Only hashing the
      // actual bytes catches it.
      writeFileSync(weights, 'tiny fake weightZ');
      utimesSync(weights, before.atime, before.mtime);
      await expect(resolved.createSession()).rejects.toThrow(/model identity mismatch/);
    } finally {
      writeFileSync(weights, 'tiny fake weights');
    }
  });

  it('re-verifies the artifact at every session launch, not only at resolve', async () => {
    const resolved = await resolveRuntime(mlxConfig());
    try {
      // Rewrite the checkpoint AFTER resolve succeeded — the next
      // launch must refuse instead of running under the stale digest.
      writeFileSync(path.join(modelDir, 'model.safetensors'), 'tampered weights, longer than before');
      await expect(resolved.createSession()).rejects.toThrow(/model identity mismatch/);
      await expect(resolved.crashRestart(1000)).rejects.toThrow(/model identity mismatch/);
    } finally {
      writeFileSync(path.join(modelDir, 'model.safetensors'), 'tiny fake weights');
    }
  });

  it('refuses an unknown transport', async () => {
    const config = fakeConfig('short-chat-pass');
    (config.runtime as { transport: string }).transport = 'carrier-pigeon';
    await expect(resolveRuntime(config)).rejects.toThrow(/unknown runtime transport/);
  });
});
