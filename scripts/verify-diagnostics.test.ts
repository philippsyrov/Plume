// Shell-level regression for verify.sh's failure diagnostics (Codex
// P2 on PR #120): the verifier once printed only `tail -n 30` of the
// frontend-test log and then DELETED it, so a unique root-cause line
// that preceded a long trailer vanished completely. The guarantee
// pinned here: on failure, the concise tail is printed AND the
// complete log survives at a reported path that contains the early
// line.
//
// The real scripts/verify.sh runs end-to-end with a PATH shim: `npm`
// fails with an early unique marker followed by 60 ordinary lines
// (Codex's exact reproduction shape); `cargo` and `npx` are
// instant-success stubs so the nested run stays fast and this test
// exercises only the diagnostics plumbing, not the other checks.

import { spawnSync } from 'node:child_process';
import { chmodSync, existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { describe, expect, it } from 'vitest';

const REPO_ROOT = path.resolve(__dirname, '..');
const EARLY_MARKER = 'UNIQUE-EARLY-ROOT-CAUSE-marker-9d41';

describe('verify.sh frontend-test failure diagnostics', () => {
  it('prints the tail AND preserves the complete log with the early root-cause line', () => {
    const shimDir = mkdtempSync(path.join(os.tmpdir(), 'plume-verify-shim-'));
    const preservedLog = path.join(REPO_ROOT, 'verify-frontend-tests.log');
    try {
      writeFileSync(
        path.join(shimDir, 'npm'),
        `#!/bin/sh
echo "${EARLY_MARKER}"
i=1
while [ $i -le 60 ]; do
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

      const run = spawnSync('bash', [path.join(REPO_ROOT, 'scripts', 'verify.sh')], {
        cwd: REPO_ROOT,
        env: { ...process.env, PATH: `${shimDir}:${process.env['PATH'] ?? ''}`, PLUME_FULL_VERIFY: '' },
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
      // Match to end of line — the repo path may contain spaces.
      const reported = output.match(/full log preserved at: (.+)/)?.[1]?.trim();
      expect(reported).toBe(preservedLog);
      const fullLog = readFileSync(preservedLog, 'utf8');
      expect(fullLog).toContain(EARLY_MARKER);
      expect(fullLog).toContain('trailing diagnostic line 1');
    } finally {
      rmSync(shimDir, { recursive: true, force: true });
      if (existsSync(preservedLog)) rmSync(preservedLog);
    }
  });
});
