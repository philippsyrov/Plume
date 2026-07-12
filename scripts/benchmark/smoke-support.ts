// D129C: shared discovery helpers for the smoke CLIs (mlx-smoke-cli,
// plume-smoke-cli). Interpreter and checkpoint resolution mirror
// Plume's shell smoke scripts; nothing is downloaded or installed —
// missing prerequisites refuse with a diagnostic.

import { existsSync, readdirSync, readFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';

import { probeMlxLmVersion } from './model-identity.ts';

export const REPO_ROOT = path.resolve(__dirname, '..', '..');

export function fail(prefix: string, message: string): never {
  process.stderr.write(`${prefix}: ${message}\n`);
  process.exit(1);
}

/// Interpreter resolution, mirroring scripts/smoke-qwen-mlx.sh:
/// PLUME_MLX_PYTHON → ~/.venvs/mlx-env/bin/python → python3.
export function resolvePython(prefix: string): string {
  const candidates = [
    process.env['PLUME_MLX_PYTHON'],
    path.join(os.homedir(), '.venvs', 'mlx-env', 'bin', 'python'),
    'python3',
  ].filter((c): c is string => c !== undefined && c.length > 0);
  for (const candidate of candidates) {
    if (probeMlxLmVersion(candidate) !== null) return candidate;
  }
  fail(
    prefix,
    'no interpreter with an importable mlx_lm found ' +
      '(tried PLUME_MLX_PYTHON, ~/.venvs/mlx-env/bin/python, python3). Nothing was installed.',
  );
}

/// Model discovery, mirroring the smoke scripts: PLUME_MODEL_DIR →
/// <repo>/plume-models → primary-checkout/plume-models →
/// ~/plume-models, preferring the documented Qwen2.5-Coder 3B 4-bit
/// target, else any folder with a config.json and a .safetensors file.
export function resolveModelDir(prefix: string): string {
  const worktreeMarker = `${path.sep}.claude${path.sep}worktrees${path.sep}`;
  const markerIndex = REPO_ROOT.indexOf(worktreeMarker);
  const primaryCheckout = markerIndex === -1 ? null : REPO_ROOT.slice(0, markerIndex);
  const roots = [
    process.env['PLUME_MODEL_DIR'],
    path.join(REPO_ROOT, 'plume-models'),
    primaryCheckout === null ? undefined : path.join(primaryCheckout, 'plume-models'),
    path.join(os.homedir(), 'plume-models'),
  ].filter((r): r is string => r !== undefined && r.length > 0);
  const preferred = 'Qwen2.5-Coder-3B-Instruct-4bit';
  for (const root of roots) {
    const candidate = path.join(root, preferred);
    if (existsSync(path.join(candidate, 'config.json'))) return candidate;
  }
  for (const root of roots) {
    if (!existsSync(root)) continue;
    for (const entry of readdirSync(root)) {
      const candidate = path.join(root, entry);
      if (existsSync(path.join(candidate, 'config.json')) && existsSync(path.join(candidate, 'model.safetensors'))) {
        return candidate;
      }
    }
  }
  fail(prefix, 'no MLX checkpoint found (tried PLUME_MODEL_DIR, <repo>/plume-models, ~/plume-models). Nothing was downloaded.');
}

export interface CheckpointQuantization {
  method: string | null;
  bits: number | null;
  groupSize: number | null;
}

/// Quantization identity read from the checkpoint's own config.json —
/// recorded only when the file actually states it.
export function readQuantization(modelDir: string): CheckpointQuantization {
  try {
    const config = JSON.parse(readFileSync(path.join(modelDir, 'config.json'), 'utf8')) as {
      quantization?: { bits?: number; group_size?: number };
    };
    const q = config.quantization;
    if (q === undefined) return { method: null, bits: null, groupSize: null };
    return {
      method: 'affine',
      bits: typeof q.bits === 'number' ? q.bits : null,
      groupSize: typeof q.group_size === 'number' ? q.group_size : null,
    };
  } catch {
    return { method: null, bits: null, groupSize: null };
  }
}
