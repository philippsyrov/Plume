// D129A: verified model and engine identity for real-runtime records.
//
// The evidence contract requires every run to record the runtime
// engine identity and the exact artifact digest. For a real MLX
// checkpoint that means: hash the model directory's actual bytes and
// probe the actual installed mlx-lm version — then REFUSE to run when
// the sanitized config declares something different. Nothing here is
// inferred from folder names.

import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import path from 'node:path';

/// sha256 over every regular file in the model directory, in sorted
/// relative-path order, each contribution being `<relpath>\n<bytes>`.
/// Symlinks are refused: a link could smuggle content from outside the
/// directory into the identity.
export function digestModelDir(dir: string): string {
  const files: string[] = [];
  const walk = (rel: string): void => {
    const abs = path.join(dir, rel);
    for (const entry of readdirSync(abs, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
      const childRel = rel === '' ? entry.name : `${rel}/${entry.name}`;
      if (entry.isSymbolicLink()) {
        throw new Error(`model dir contains a symlink (${childRel}) — refusing to digest linked content`);
      }
      if (entry.isDirectory()) walk(childRel);
      else if (entry.isFile()) files.push(childRel);
    }
  };
  const stats = statSync(dir);
  if (!stats.isDirectory()) throw new Error(`${dir}: not a directory`);
  walk('');
  const hash = createHash('sha256');
  for (const rel of files) {
    hash.update(`${rel}\n`);
    hash.update(readFileSync(path.join(dir, rel)));
  }
  return `sha256:${hash.digest('hex')}`;
}

/// Probe the mlx-lm version the given python interpreter would
/// actually serve with. Returns null when the import fails (the
/// caller decides whether that is fatal).
export function probeMlxLmVersion(pythonBin: string): string | null {
  try {
    const out = execFileSync(pythonBin, ['-c', 'import mlx_lm; print(mlx_lm.__version__)'], {
      encoding: 'utf8',
      timeout: 30_000,
    });
    const version = out.trim();
    return version.length > 0 ? version : null;
  } catch {
    return null;
  }
}

const digestCache = new Map<string, string>();

/// Digesting a multi-GB checkpoint is expensive; one verification per
/// process per directory is enough (the bytes cannot change under us
/// without a new process noticing on its own first pass).
export function digestModelDirCached(dir: string): string {
  const resolved = path.resolve(dir);
  const cached = digestCache.get(resolved);
  if (cached !== undefined) return cached;
  const digest = digestModelDir(resolved);
  digestCache.set(resolved, digest);
  return digest;
}
