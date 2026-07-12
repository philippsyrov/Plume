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

/// Sorted relative paths of every regular file under the model
/// directory. Symlinks are refused: a link could smuggle content from
/// outside the directory into the identity.
function listModelFiles(dir: string): string[] {
  const stats = statSync(dir);
  if (!stats.isDirectory()) throw new Error(`${dir}: not a directory`);
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
  walk('');
  return files;
}

/// sha256 over every regular file in the model directory, in sorted
/// relative-path order, each contribution being `<relpath>\n<bytes>`.
export function digestModelDir(dir: string): string {
  const hash = createHash('sha256');
  for (const rel of listModelFiles(dir)) {
    hash.update(`${rel}\n`);
    hash.update(readFileSync(path.join(dir, rel)));
  }
  return `sha256:${hash.digest('hex')}`;
}

// There is deliberately NO digest cache. A stat-level fingerprint
// (size + mtime) cannot see a same-size rewrite with a restored
// mtime, so a cached digest can vouch for bytes that are no longer
// there. Identity verification is a full re-digest of the actual
// bytes, every time — the cost (seconds per launch, always outside
// the timed windows) is the price of the "verified identity" claim.

/// The Plume identity every record carries (run-model stamps it on
/// records; the sidecar verification below compares against it).
/// Env overrides exist for deterministic tests.
export interface PlumeIdentity {
  gitSha: string;
  dirty: boolean;
}

export function plumeIdentity(): PlumeIdentity {
  const envSha = process.env['PLUME_BENCH_GIT_SHA'];
  const envDirty = process.env['PLUME_BENCH_DIRTY'];
  if (envSha !== undefined && envDirty !== undefined) {
    return { gitSha: envSha, dirty: envDirty === 'true' };
  }
  const gitValue = (args: string[]): string => execFileSync('git', args, { encoding: 'utf8' }).trim();
  return {
    gitSha: gitValue(['rev-parse', 'HEAD']),
    dirty: gitValue(['status', '--porcelain']).length > 0,
  };
}

export interface SidecarIdentity {
  gitSha: string | null;
  dirty: boolean | null;
  maxOutputTokens: number;
}

/// Ask a plume_bench binary for its BUILD identity (embedded by
/// src-tauri/build.rs at compile time) and the product output cap.
/// Throws on any malformed reply — an unidentifiable binary is never
/// probed further.
export function probeSidecarIdentity(binary: string): SidecarIdentity {
  let out: string;
  try {
    out = execFileSync(binary, ['identity'], { encoding: 'utf8', timeout: 15_000 });
  } catch (err) {
    throw new Error(`plume_bench identity probe failed: ${err instanceof Error ? err.message : String(err)}`);
  }
  let parsed: { ok?: unknown; gitSha?: unknown; dirty?: unknown; maxOutputTokens?: unknown };
  try {
    parsed = JSON.parse(out.trim()) as typeof parsed;
  } catch {
    throw new Error(`plume_bench identity reply is not JSON: ${out.trim()}`);
  }
  if (
    parsed.ok !== true ||
    typeof parsed.maxOutputTokens !== 'number' ||
    (typeof parsed.gitSha !== 'string' && parsed.gitSha !== null) ||
    (typeof parsed.dirty !== 'boolean' && parsed.dirty !== null)
  ) {
    throw new Error(`plume_bench identity reply malformed: ${out.trim()}`);
  }
  return { gitSha: parsed.gitSha, dirty: parsed.dirty, maxOutputTokens: parsed.maxOutputTokens };
}

/// Verified sidecar provenance, or no run: the binary's embedded
/// build identity must be EXACTLY the given expected identity (the
/// snapshot the caller pinned for this attempt/launch — callers pass
/// it in rather than letting this function recompute, so a commit or
/// rebuild between capture and use is a refusal, never a mixed
/// record). A stale target/debug build, a foreign binary, or a
/// git-less build (null identity) refuses. Returns the probed reply
/// so callers can verify the output cap from the same handshake.
export function verifySidecarIdentity(binary: string, expected: PlumeIdentity): SidecarIdentity {
  const sidecar = probeSidecarIdentity(binary);
  if (sidecar.gitSha === null || sidecar.dirty === null) {
    throw new Error(
      `plume_bench at ${binary} carries no verifiable build identity (built without git?) — ` +
        'refusing to label its measurements as Plume',
    );
  }
  if (sidecar.gitSha !== expected.gitSha || sidecar.dirty !== expected.dirty) {
    throw new Error(
      `plume_bench identity mismatch: the sidecar was built from ${sidecar.gitSha}` +
        `${sidecar.dirty ? ' (dirty)' : ''} but records would carry ${expected.gitSha}` +
        `${expected.dirty ? ' (dirty)' : ''} — stale or foreign binary; rebuild it ` +
        '(scripts/benchmark-plume-smoke.sh rebuilds automatically)',
    );
  }
  return sidecar;
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
