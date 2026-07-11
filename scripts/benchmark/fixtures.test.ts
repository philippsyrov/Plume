// D129: fixture-pack integrity — every committed fixture loads (its
// contentDigest matches its files), and fixture text stays synthetic
// and local (no URLs, no network hooks in verifiers).

import { readdirSync, readFileSync, statSync } from 'node:fs';
import path from 'node:path';
import { describe, expect, it } from 'vitest';

import { loadFixture } from './fixtures.ts';
import { REPO_ROOT } from './test-support.ts';
import { SUITE_IDS } from './types.ts';

const fixturesRoot = path.join(REPO_ROOT, 'benchmarks', 'fixtures');

function fixtureDirs(): string[] {
  const dirs: string[] = [];
  for (const suite of readdirSync(fixturesRoot)) {
    const suiteDir = path.join(fixturesRoot, suite);
    if (!statSync(suiteDir).isDirectory()) continue;
    for (const caseId of readdirSync(suiteDir)) {
      const caseDir = path.join(suiteDir, caseId);
      if (statSync(caseDir).isDirectory()) dirs.push(caseDir);
    }
  }
  return dirs;
}

describe('fixture pack', () => {
  it('covers every suite the contract defines', () => {
    const covered = new Set(fixtureDirs().map((d) => loadFixture(d).manifest.suiteId));
    for (const suite of SUITE_IDS) {
      expect(covered, `missing fixture for suite ${suite}`).toContain(suite);
    }
  });

  it('every fixture loads with a matching content digest', () => {
    for (const dir of fixtureDirs()) {
      expect(() => loadFixture(dir), dir).not.toThrow();
    }
  });

  it('contains no network references', () => {
    for (const dir of fixtureDirs()) {
      const fixture = loadFixture(dir);
      const texts = [
        JSON.stringify(fixture.manifest),
        ...fixture.manifest.files.map((f) => readFileSync(path.join(dir, f), 'utf8')),
      ];
      for (const text of texts) {
        expect(text, dir).not.toMatch(/https?:\/\/|curl |wget |fetch\(/);
      }
    }
  });
});
