// Shell-level regression for verify.sh's failure diagnostics (Codex
// P2 + P1 on PR #120):
//
//   P2 — the verifier once printed only `tail -n 30` of the
//   frontend-test log and then DELETED it, so a unique root-cause line
//   that preceded a long trailer vanished completely. Guarantee pinned:
//   on failure the concise tail is printed AND the complete log
//   survives at a reported path that contains the early line.
//
//   P1 — the preserved log lived at a single shared filename. The
//   frontend suite ITSELF runs this test, which spawns a nested
//   verify.sh; the nested run overwrote and then deleted the outer
//   run's log mid-suite, so a real outer failure lost its evidence.
//   Guarantee pinned: each verifier process writes a distinct
//   (PID-namespaced) log, so concurrent/nested runs never clobber one
//   another.
//
// The real scripts/verify.sh runs end-to-end with a PATH shim: `npm`
// fails with an early unique marker followed by ordinary lines;
// `cargo` and `npx` are instant-success stubs so the nested run stays
// fast and this test exercises only the diagnostics plumbing.

import { spawn, spawnSync } from 'node:child_process';
import { chmodSync, existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { describe, expect, it } from 'vitest';

const REPO_ROOT = path.resolve(__dirname, '..');
const VERIFY = path.join(REPO_ROOT, 'scripts', 'verify.sh');
const EARLY_MARKER = 'UNIQUE-EARLY-ROOT-CAUSE-marker-9d41';

// A PATH shim whose `npm` echoes $PLUME_TEST_MARKER, then `trailing`
// ordinary lines, then exits 1 (failing the frontend check). `cargo`
// and `npx` are instant-success stubs so only the diagnostics path is
// exercised. Returns the shim dir (caller removes it).
function makeShim(trailing: number): string {
  const shimDir = mkdtempSync(path.join(os.tmpdir(), 'plume-verify-shim-'));
  writeFileSync(
    path.join(shimDir, 'npm'),
    `#!/bin/sh
echo "$PLUME_TEST_MARKER"
i=1
while [ $i -le ${trailing} ]; do
  echo "trailing diagnostic line $i"
  i=$((i + 1))
done
exit 1
`,
  );
  for (const tool of ['cargo', 'npx']) {
    writeFileSync(path.join(shimDir, tool), '#!/bin/sh\nexit 0\n');
  }
  for (const tool of ['npm', 'cargo', 'npx']) {
    chmodSync(path.join(shimDir, tool), 0o755);
  }
  return shimDir;
}

// Extract the reported "full log preserved at:" path. Matched to
// end-of-line because the repo path may contain spaces.
function reportedLogPath(output: string): string | undefined {
  return output.match(/full log preserved at: (.+)/)?.[1]?.trim();
}

describe('verify.sh frontend-test failure diagnostics', () => {
  it('prints the tail AND preserves the complete log with the early root-cause line', () => {
    const shimDir = makeShim(60);
    let preservedLog: string | undefined;
    try {
      const run = spawnSync('bash', [VERIFY], {
        cwd: REPO_ROOT,
        env: {
          ...process.env,
          PATH: `${shimDir}:${process.env['PATH'] ?? ''}`,
          PLUME_FULL_VERIFY: '',
          PLUME_TEST_MARKER: EARLY_MARKER,
        },
        encoding: 'utf8',
        timeout: 120_000,
      });

      const output = `${run.stdout}\n${run.stderr}`;
      // The frontend check failed, so the whole verifier must fail.
      expect(run.status).toBe(1);
      expect(output).toContain('Frontend tests failed');
      // Concise tail printed inline…
      expect(output).toContain('trailing diagnostic line 60');
      // …and the COMPLETE log preserved at a reported path holding the
      // early root-cause line the tail cannot show.
      expect(output).toContain('full log preserved at:');
      preservedLog = reportedLogPath(output);
      expect(preservedLog).toBeDefined();
      // PID-namespaced, not the bare shared filename (P1 fix).
      expect(preservedLog).toMatch(/[/\\]verify-frontend-tests\.\d+\.log$/);
      const fullLog = readFileSync(preservedLog as string, 'utf8');
      expect(fullLog).toContain(EARLY_MARKER);
      expect(fullLog).toContain('trailing diagnostic line 1');
    } finally {
      rmSync(shimDir, { recursive: true, force: true });
      if (preservedLog && existsSync(preservedLog)) rmSync(preservedLog);
    }
  });

  // P1 re-entrancy: the frontend suite runs THIS test, which spawns a
  // nested verify.sh. With a shared log filename the nested run
  // clobbered and deleted the outer run's evidence. Two concurrent
  // verifiers must therefore each preserve their OWN log, at distinct
  // paths, containing only their own marker.
  it('gives each concurrent verifier a distinct log — no cross-run clobber', async () => {
    const shimDir = makeShim(5);
    const markerA = 'CONCURRENT-VERIFIER-A-marker-a1b2';
    const markerB = 'CONCURRENT-VERIFIER-B-marker-c3d4';

    const runVerify = (marker: string): Promise<string> =>
      new Promise((resolve, reject) => {
        const child = spawn('bash', [VERIFY], {
          cwd: REPO_ROOT,
          env: {
            ...process.env,
            PATH: `${shimDir}:${process.env['PATH'] ?? ''}`,
            PLUME_FULL_VERIFY: '',
            PLUME_TEST_MARKER: marker,
          },
        });
        let out = '';
        child.stdout.on('data', (c: Buffer) => (out += c.toString('utf8')));
        child.stderr.on('data', (c: Buffer) => (out += c.toString('utf8')));
        child.on('error', reject);
        child.on('close', () => resolve(out));
      });

    let logA: string | undefined;
    let logB: string | undefined;
    try {
      // Genuinely concurrent — both processes race for the log file.
      const [outA, outB] = await Promise.all([runVerify(markerA), runVerify(markerB)]);

      logA = reportedLogPath(outA);
      logB = reportedLogPath(outB);
      expect(logA).toBeDefined();
      expect(logB).toBeDefined();
      // Distinct files (PID-namespaced) — the crux of the P1 fix.
      expect(logA).not.toBe(logB);

      // Each preserved log holds ONLY its own marker: no clobber.
      const contentA = readFileSync(logA as string, 'utf8');
      const contentB = readFileSync(logB as string, 'utf8');
      expect(contentA).toContain(markerA);
      expect(contentA).not.toContain(markerB);
      expect(contentB).toContain(markerB);
      expect(contentB).not.toContain(markerA);
    } finally {
      rmSync(shimDir, { recursive: true, force: true });
      if (logA && existsSync(logA)) rmSync(logA);
      if (logB && existsSync(logB)) rmSync(logB);
    }
  });
});
