// @vitest-environment node

import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

import {
  checkCampaignFixtures,
  FIXTURE_KEYS,
  FIXTURE_STATUSES,
  REQUIRED_SCENARIOS,
  type FixtureCheckResult,
} from './campaign-fixtures.ts';

const FIXTURE_DIRECTORY = 'docs/superpowers/fixtures/continuous-chat';

type FixtureRecord = {
  scenarioId: string;
  fixtureRevision: string;
  phase: number;
  intent: string;
  ownedState: string[];
  steps: string[];
  expectedOutcome: string[];
  mustNotHappen: string[];
  implementationStatus: string;
  automatedEvidence: string[];
};

function validRecord(overrides: Partial<FixtureRecord> = {}): FixtureRecord {
  return {
    scenarioId: 'repeated-compaction',
    fixtureRevision: 'v1',
    phase: 2,
    intent: 'A long conversation is compacted and keeps its standing constraint.',
    ownedState: ['CompactionCheckpoint'],
    steps: ['State a goal.', 'Let a checkpoint form.'],
    expectedOutcome: ['The goal survives the checkpoint.'],
    mustNotHappen: ['Compaction prose never confers authority.'],
    implementationStatus: 'unimplemented',
    automatedEvidence: [],
    ...overrides,
  };
}

/** Every required scenario, valid, keyed by filename stem. */
function validCorpus(): Record<string, unknown> {
  const corpus: Record<string, unknown> = {};
  for (const scenarioId of REQUIRED_SCENARIOS) {
    corpus[scenarioId] = validRecord({ scenarioId });
  }
  return corpus;
}

/** Writes a temp repository root holding `records` as JSON and `raw` verbatim. */
function writeCorpus(records: Record<string, unknown>, raw: Record<string, string> = {}): string {
  const root = mkdtempSync(join(tmpdir(), 'plume-campaign-fixtures-'));
  mkdirSync(join(root, FIXTURE_DIRECTORY), { recursive: true });

  for (const [stem, value] of Object.entries(records)) {
    writeFileSync(join(root, FIXTURE_DIRECTORY, `${stem}.json`), `${JSON.stringify(value, null, 2)}\n`);
  }
  for (const [stem, value] of Object.entries(raw)) {
    writeFileSync(join(root, FIXTURE_DIRECTORY, `${stem}.json`), value);
  }
  return root;
}

function check(root: string): FixtureCheckResult {
  return checkCampaignFixtures({ root });
}

function subject(stem: string): string {
  return `${FIXTURE_DIRECTORY}/${stem}.json`;
}

describe('checkCampaignFixtures', () => {
  it('accepts a corpus where every required scenario is well formed', () => {
    const result = check(writeCorpus(validCorpus()));

    expect(result.errors).toEqual([]);
    expect(result.warnings).toEqual([]);
  });

  it('exposes the two-word status vocabulary and the ten record keys', () => {
    expect([...FIXTURE_STATUSES]).toEqual(['unimplemented', 'implemented']);
    expect([...FIXTURE_KEYS]).toHaveLength(10);
    expect([...REQUIRED_SCENARIOS]).toEqual([...REQUIRED_SCENARIOS].sort());
  });

  it.each([...FIXTURE_KEYS])('rejects a record missing required key %s', (key) => {
    const corpus = validCorpus();
    const record = validRecord({ scenarioId: 'run-cancellation' }) as Record<string, unknown>;
    delete record[key];
    corpus['run-cancellation'] = record;

    expect(check(writeCorpus(corpus)).errors).toContain(
      `${subject('run-cancellation')} is missing required key ${key}`,
    );
  });

  it('rejects a record carrying an unexpected extra key', () => {
    const corpus = validCorpus();
    corpus['run-cancellation'] = { ...validRecord({ scenarioId: 'run-cancellation' }), owner: 'nobody' };

    expect(check(writeCorpus(corpus)).errors).toContain(
      `${subject('run-cancellation')} has unexpected key owner`,
    );
  });

  it('rejects a scenarioId that does not equal the filename stem', () => {
    const corpus = validCorpus();
    corpus['run-cancellation'] = validRecord({ scenarioId: 'grant-revocation' });

    expect(check(writeCorpus(corpus)).errors).toContain(
      `${subject('run-cancellation')} scenarioId 'grant-revocation' must equal the filename stem 'run-cancellation'`,
    );
  });

  it('rejects a duplicate scenarioId', () => {
    const corpus = validCorpus();
    corpus['run-cancellation'] = validRecord({ scenarioId: 'grant-revocation' });

    expect(check(writeCorpus(corpus)).errors).toContain(
      `${subject('run-cancellation')} repeats scenarioId 'grant-revocation' already declared by another fixture`,
    );
  });

  it('rejects an unimplemented scenario that carries evidence', () => {
    // The corpus README promises a scenario is flipped in the same commit as
    // its test. Evidence attached to an unimplemented scenario means one of
    // the two happened without the other.
    const corpus = validCorpus();
    corpus['repeated-compaction'] = validRecord({
      implementationStatus: 'unimplemented',
      automatedEvidence: ['scripts/docs/campaign-fixtures.test.ts'],
    });

    expect(check(writeCorpus(corpus)).errors).toContain(
      `${subject('repeated-compaction')} is unimplemented so automatedEvidence must be empty`,
    );
  });

  it('rejects implemented evidence that escapes the repository', () => {
    const corpus = validCorpus();
    corpus['repeated-compaction'] = validRecord({
      implementationStatus: 'implemented',
      automatedEvidence: ['../../../../../../etc/passwd'],
    });

    expect(check(writeCorpus(corpus)).errors).toContain(
      `${subject('repeated-compaction')} claims implemented but automatedEvidence path '../../../../../../etc/passwd' must stay inside the repository`,
    );
  });

  it('rejects an absolute implemented evidence path', () => {
    const corpus = validCorpus();
    corpus['repeated-compaction'] = validRecord({
      implementationStatus: 'implemented',
      automatedEvidence: ['/etc/passwd'],
    });

    expect(check(writeCorpus(corpus)).errors).toContain(
      `${subject('repeated-compaction')} claims implemented but automatedEvidence path '/etc/passwd' must be repository-relative`,
    );
  });

  it('rejects implemented evidence naming a directory instead of a test file', () => {
    const corpus = validCorpus();
    corpus['repeated-compaction'] = validRecord({
      implementationStatus: 'implemented',
      automatedEvidence: ['docs'],
    });

    expect(check(writeCorpus(corpus)).errors).toContain(
      `${subject('repeated-compaction')} claims implemented but automatedEvidence path 'docs' must name an existing regular file`,
    );
  });

  it('accepts implemented evidence naming a real file inside the repository', () => {
    const corpus = validCorpus();
    corpus['repeated-compaction'] = validRecord({
      implementationStatus: 'implemented',
      automatedEvidence: ['scripts/docs/proof.test.ts'],
    });
    const root = writeCorpus(corpus);
    mkdirSync(join(root, 'scripts/docs'), { recursive: true });
    writeFileSync(join(root, 'scripts/docs/proof.test.ts'), 'export {};\n');

    expect(check(root).errors).toEqual([]);
  });

  it('does not report a present-but-malformed scenario as missing', () => {
    // A malformed record still fails, but the message must name its actual
    // defect. Reporting a file that exists as "missing" sends a reviewer
    // looking for the wrong problem.
    const corpus = validCorpus();
    corpus['grant-revocation'] = {
      ...validRecord({ scenarioId: 'grant-revocation', phase: 4 }),
      sneaky: true,
    };

    const errors = check(writeCorpus(corpus)).errors;

    expect(errors).toContain(
      `${FIXTURE_DIRECTORY}/grant-revocation.json has unexpected key sneaky`,
    );
    expect(errors).not.toContain(
      `${FIXTURE_DIRECTORY} is missing required scenario 'grant-revocation'`,
    );
  });

  it('rejects a corpus missing a required scenario', () => {
    const corpus = validCorpus();
    delete corpus['grant-revocation'];

    expect(check(writeCorpus(corpus)).errors).toContain(
      `${FIXTURE_DIRECTORY} is missing required scenario 'grant-revocation'`,
    );
  });

  it('rejects a fixture file whose stem is not a known scenario', () => {
    const corpus = validCorpus();
    corpus['invented-scenario'] = validRecord({ scenarioId: 'invented-scenario' });

    expect(check(writeCorpus(corpus)).errors).toContain(
      `${subject('invented-scenario')} declares unknown scenarioId 'invented-scenario'`,
    );
  });

  it('rejects an implementationStatus outside the two-word vocabulary', () => {
    const corpus = validCorpus();
    corpus['run-cancellation'] = validRecord({
      scenarioId: 'run-cancellation',
      implementationStatus: 'in-progress',
    });

    expect(check(writeCorpus(corpus)).errors).toContain(
      `${subject('run-cancellation')} implementationStatus 'in-progress' must be one of unimplemented, implemented`,
    );
  });

  it.each(['shipped', 'partial', 'scaffold', 'researched', 'blocked', 'retired'])(
    'rejects the inventory status word %s with its own explicit message',
    (word) => {
      const corpus = validCorpus();
      corpus['run-cancellation'] = validRecord({
        scenarioId: 'run-cancellation',
        implementationStatus: word,
      });

      expect(check(writeCorpus(corpus)).errors).toContain(
        `${subject('run-cancellation')} implementationStatus '${word}' reuses the docs/FEATURE_INVENTORY.md status vocabulary; this corpus must never read as a competing status ledger`,
      );
    },
  );

  it('rejects implemented with empty automatedEvidence', () => {
    const corpus = validCorpus();
    corpus['run-cancellation'] = validRecord({
      scenarioId: 'run-cancellation',
      implementationStatus: 'implemented',
      automatedEvidence: [],
    });

    expect(check(writeCorpus(corpus)).errors).toContain(
      `${subject('run-cancellation')} claims implemented but automatedEvidence is empty`,
    );
  });

  it('rejects implemented naming an evidence path that does not exist on disk', () => {
    const corpus = validCorpus();
    corpus['run-cancellation'] = validRecord({
      scenarioId: 'run-cancellation',
      implementationStatus: 'implemented',
      automatedEvidence: ['src/features/runs/absent.test.ts'],
    });

    expect(check(writeCorpus(corpus)).errors).toContain(
      `${subject('run-cancellation')} claims implemented but automatedEvidence path 'src/features/runs/absent.test.ts' must name an existing regular file`,
    );
  });

  it('accepts implemented when every evidence path exists on disk', () => {
    const corpus = validCorpus();
    corpus['run-cancellation'] = validRecord({
      scenarioId: 'run-cancellation',
      implementationStatus: 'implemented',
      automatedEvidence: ['src/runs.test.ts'],
    });
    const root = writeCorpus(corpus);
    mkdirSync(join(root, 'src'), { recursive: true });
    writeFileSync(join(root, 'src/runs.test.ts'), 'evidence\n');

    expect(check(root).errors).toEqual([]);
  });

  it.each([['' as unknown], [42 as unknown], [null as unknown]])(
    'rejects a non-string or empty intent: %s',
    (intent) => {
      const corpus = validCorpus();
      corpus['run-cancellation'] = { ...validRecord({ scenarioId: 'run-cancellation' }), intent };

      expect(check(writeCorpus(corpus)).errors).toContain(
        `${subject('run-cancellation')} intent must be a non-empty string`,
      );
    },
  );

  it.each(['ownedState', 'steps', 'expectedOutcome', 'mustNotHappen'])(
    'rejects an empty %s array',
    (key) => {
      const corpus = validCorpus();
      corpus['run-cancellation'] = { ...validRecord({ scenarioId: 'run-cancellation' }), [key]: [] };

      expect(check(writeCorpus(corpus)).errors).toContain(
        `${subject('run-cancellation')} ${key} must be a non-empty array of non-empty strings`,
      );
    },
  );

  it.each(['ownedState', 'steps', 'expectedOutcome', 'mustNotHappen', 'automatedEvidence'])(
    'rejects a non-array %s',
    (key) => {
      const corpus = validCorpus();
      corpus['run-cancellation'] = { ...validRecord({ scenarioId: 'run-cancellation' }), [key]: 'one' };

      expect(check(writeCorpus(corpus)).errors).toContain(
        `${subject('run-cancellation')} ${key} must be an array of non-empty strings`,
      );
    },
  );

  it.each(['ownedState', 'steps', 'expectedOutcome', 'mustNotHappen', 'automatedEvidence'])(
    'rejects a non-string entry inside %s',
    (key) => {
      const corpus = validCorpus();
      corpus['run-cancellation'] = { ...validRecord({ scenarioId: 'run-cancellation' }), [key]: [7] };

      expect(check(writeCorpus(corpus)).errors).toContain(
        `${subject('run-cancellation')} ${key}[0] must be a non-empty string`,
      );
    },
  );

  it.each(['ownedState', 'steps', 'expectedOutcome', 'mustNotHappen', 'automatedEvidence'])(
    'rejects an empty string inside %s',
    (key) => {
      const corpus = validCorpus();
      corpus['run-cancellation'] = { ...validRecord({ scenarioId: 'run-cancellation' }), [key]: ['  '] };

      expect(check(writeCorpus(corpus)).errors).toContain(
        `${subject('run-cancellation')} ${key}[0] must be a non-empty string`,
      );
    },
  );

  it('accepts automatedEvidence as an empty array while unimplemented', () => {
    const corpus = validCorpus();
    corpus['run-cancellation'] = validRecord({ scenarioId: 'run-cancellation', automatedEvidence: [] });

    expect(check(writeCorpus(corpus)).errors).toEqual([]);
  });

  it.each([[-1], [10], [2.5], ['2' as unknown]])('rejects an out-of-range phase: %s', (phase) => {
    const corpus = validCorpus();
    corpus['run-cancellation'] = { ...validRecord({ scenarioId: 'run-cancellation' }), phase };

    expect(check(writeCorpus(corpus)).errors).toContain(
      `${subject('run-cancellation')} phase must be an integer between 0 and 9`,
    );
  });

  it('rejects malformed JSON', () => {
    const corpus = validCorpus();
    delete corpus['run-cancellation'];
    const root = writeCorpus(corpus, { 'run-cancellation': '{ "scenarioId": ' });

    expect(check(root).errors).toContain(`${subject('run-cancellation')} must contain valid JSON`);
  });

  it('rejects a JSON value that is not an object', () => {
    const corpus = validCorpus();
    corpus['run-cancellation'] = [validRecord({ scenarioId: 'run-cancellation' })];

    expect(check(writeCorpus(corpus)).errors).toContain(
      `${subject('run-cancellation')} must contain a JSON object`,
    );
  });

  it('rejects a missing or unreadable fixtures directory', () => {
    const root = writeCorpus(validCorpus());
    rmSync(join(root, FIXTURE_DIRECTORY), { recursive: true, force: true });

    expect(check(root).errors).toContain(`${FIXTURE_DIRECTORY} could not be read`);
  });

  it('accepts the real repository corpus', () => {
    const result = checkCampaignFixtures({ root: process.cwd() });

    expect(result.errors).toEqual([]);
  });
});
