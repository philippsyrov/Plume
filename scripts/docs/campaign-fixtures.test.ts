// @vitest-environment node

import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { describe, expect, it } from 'vitest';

import {
  checkCampaignFixtures,
  EVIDENCE_KEYS,
  FIXTURE_KEYS,
  FIXTURE_REVISION,
  FIXTURE_STATUSES,
  PROBE_KEYS,
  probeScenarios,
  renderProbeReport,
  REQUIRED_SCENARIOS,
  SCENARIO_PHASES,
  type FixtureCheckResult,
  type ScenarioId,
} from './campaign-fixtures.ts';

const FIXTURE_DIRECTORY = 'docs/superpowers/fixtures/continuous-chat';

type EvidenceEntry = { path: string; testName: string };

type CapabilityProbe = {
  requiredTypeDeclarations: string[];
  requiredCommandNames: string[];
};

type FixtureRecord = {
  scenarioId: string;
  fixtureRevision: string;
  phase: number;
  intent: string;
  ownedState: string[];
  steps: string[];
  expectedOutcome: string[];
  mustNotHappen: string[];
  capabilityProbe: CapabilityProbe;
  implementationStatus: string;
  automatedEvidence: EvidenceEntry[];
};

function validRecord(overrides: Partial<FixtureRecord> = {}): FixtureRecord {
  const scenarioId = overrides.scenarioId ?? 'repeated-compaction';
  return {
    scenarioId,
    fixtureRevision: FIXTURE_REVISION,
    phase: SCENARIO_PHASES[scenarioId as ScenarioId] ?? 2,
    intent: 'A long conversation is compacted and keeps its standing constraint.',
    ownedState: ['CompactionCheckpoint'],
    steps: ['State a goal.', 'Let a checkpoint form.'],
    expectedOutcome: ['The goal survives the checkpoint.'],
    mustNotHappen: ['Compaction prose never confers authority.'],
    capabilityProbe: {
      requiredTypeDeclarations: ['CompactionCheckpoint'],
      requiredCommandNames: [],
    },
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

function writeFile(root: string, relativePath: string, contents: string): void {
  const target = join(root, relativePath);
  mkdirSync(dirname(target), { recursive: true });
  writeFileSync(target, contents);
}

/** Declares `name` in the temp tree so a probe requiring it reads as satisfied. */
function declareType(root: string, name: string): void {
  writeFile(root, `src-tauri/src/${name.toLowerCase()}.rs`, `pub struct ${name} {\n    id: String,\n}\n`);
}

function writeAppCommands(root: string, commands: readonly string[], trailer = ''): void {
  const body = commands.map((command) => `    "${command}",`).join('\n');
  writeFile(
    root,
    'src-tauri/src/app_commands.rs',
    `pub const APP_COMMANDS: &[&str] = &[\n${body}\n];\n${trailer}`,
  );
}

function check(root: string): FixtureCheckResult {
  return checkCampaignFixtures({ root });
}

function subject(stem: string): string {
  return `${FIXTURE_DIRECTORY}/${stem}.json`;
}

const VITEST_IMPORT = "import { describe, it, test } from 'vitest';\n";

describe('checkCampaignFixtures', () => {
  it('accepts a corpus where every required scenario is well formed', () => {
    const result = check(writeCorpus(validCorpus()));

    expect(result.errors).toEqual([]);
    expect(result.warnings).toEqual([]);
  });

  it('exposes the two-word status vocabulary and the eleven record keys', () => {
    expect([...FIXTURE_STATUSES]).toEqual(['unimplemented', 'implemented']);
    expect([...FIXTURE_KEYS]).toEqual([
      'scenarioId',
      'fixtureRevision',
      'phase',
      'intent',
      'ownedState',
      'steps',
      'expectedOutcome',
      'mustNotHappen',
      'capabilityProbe',
      'implementationStatus',
      'automatedEvidence',
    ]);
    expect([...EVIDENCE_KEYS]).toEqual(['path', 'testName']);
    expect([...PROBE_KEYS]).toEqual(['requiredTypeDeclarations', 'requiredCommandNames']);
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

  it('rejects a fixtureRevision other than v2', () => {
    const corpus = validCorpus();
    corpus['run-cancellation'] = validRecord({
      scenarioId: 'run-cancellation',
      fixtureRevision: 'v1',
    });

    expect(check(writeCorpus(corpus)).errors).toContain(
      `${subject('run-cancellation')} fixtureRevision 'v1' must be 'v2'`,
    );
  });

  it('rejects a non-string fixtureRevision', () => {
    const corpus = validCorpus();
    corpus['run-cancellation'] = {
      ...validRecord({ scenarioId: 'run-cancellation' }),
      fixtureRevision: 2,
    };

    expect(check(writeCorpus(corpus)).errors).toContain(
      `${subject('run-cancellation')} fixtureRevision '2' must be 'v2'`,
    );
  });

  it('pins the canonical scenario to phase map', () => {
    expect(SCENARIO_PHASES).toEqual({
      'grant-revocation': 4,
      'legacy-session-migration': 5,
      'memory-correction-and-forget': 3,
      'reference-folder-write-rejection': 6,
      'repeated-compaction': 2,
      'run-cancellation': 6,
    });
  });

  it.each([...REQUIRED_SCENARIOS])(
    'rejects %s carrying a phase the map does not assign',
    (scenarioId) => {
      const expected = SCENARIO_PHASES[scenarioId];
      const wrong = expected === 9 ? 8 : expected + 1;
      const corpus = validCorpus();
      corpus[scenarioId] = validRecord({ scenarioId, phase: wrong });

      expect(check(writeCorpus(corpus)).errors).toContain(
        `${subject(scenarioId)} phase ${wrong} must equal the canonical phase ${expected} for scenario '${scenarioId}'`,
      );
    },
  );

  it('rejects an unimplemented scenario that carries evidence', () => {
    // The corpus README promises a scenario is flipped in the same commit as
    // its test. Evidence attached to an unimplemented scenario means one of
    // the two happened without the other.
    const corpus = validCorpus();
    corpus['repeated-compaction'] = validRecord({
      implementationStatus: 'unimplemented',
      automatedEvidence: [
        {
          path: 'scripts/docs/campaign-fixtures.test.ts',
          testName: 'accepts the real repository corpus',
        },
      ],
    });

    expect(check(writeCorpus(corpus)).errors).toContain(
      `${subject('repeated-compaction')} is unimplemented so automatedEvidence must be empty`,
    );
  });

  it('rejects implemented evidence that escapes the repository', () => {
    const corpus = validCorpus();
    corpus['repeated-compaction'] = validRecord({
      implementationStatus: 'implemented',
      automatedEvidence: [{ path: '../../../../../../etc/passwd', testName: 'anything' }],
    });

    expect(check(writeCorpus(corpus)).errors).toContain(
      `${subject('repeated-compaction')} claims implemented but automatedEvidence path '../../../../../../etc/passwd' must stay inside the repository`,
    );
  });

  it('rejects an absolute implemented evidence path', () => {
    const corpus = validCorpus();
    corpus['repeated-compaction'] = validRecord({
      implementationStatus: 'implemented',
      automatedEvidence: [{ path: '/etc/passwd', testName: 'anything' }],
    });

    expect(check(writeCorpus(corpus)).errors).toContain(
      `${subject('repeated-compaction')} claims implemented but automatedEvidence path '/etc/passwd' must be repository-relative`,
    );
  });

  it('rejects implemented evidence naming a directory instead of a test file', () => {
    const corpus = validCorpus();
    corpus['repeated-compaction'] = validRecord({
      implementationStatus: 'implemented',
      automatedEvidence: [{ path: 'docs', testName: 'anything' }],
    });

    expect(check(writeCorpus(corpus)).errors).toContain(
      `${subject('repeated-compaction')} claims implemented but automatedEvidence path 'docs' must name an existing regular file`,
    );
  });

  it('rejects implemented evidence naming a real file that is not a test file', () => {
    const corpus = validCorpus();
    corpus['repeated-compaction'] = validRecord({
      implementationStatus: 'implemented',
      automatedEvidence: [{ path: 'README.md', testName: 'anything' }],
    });
    const root = writeCorpus(corpus);
    writeFile(root, 'README.md', "it('anything', () => {});\n");

    expect(check(root).errors).toContain(
      `${subject('repeated-compaction')} claims implemented but automatedEvidence path 'README.md' must name a test file (.test.ts, .test.tsx, .spec.ts, .spec.tsx, _tests.rs)`,
    );
  });

  it('rejects evidence naming a test that the file does not contain', () => {
    const corpus = validCorpus();
    corpus['run-cancellation'] = validRecord({
      scenarioId: 'run-cancellation',
      implementationStatus: 'implemented',
      automatedEvidence: [{ path: 'src/runs.test.ts', testName: 'settles a live run' }],
    });
    const root = writeCorpus(corpus);
    writeFile(root, 'src/runs.test.ts', "it('does something else', () => {});\n");

    expect(check(root).errors).toContain(
      `${subject('run-cancellation')} claims implemented but 'src/runs.test.ts' does not contain a test named 'settles a live run'`,
    );
  });

  it.each([
    ['it and single quotes', `${VITEST_IMPORT}it('settles a live run', () => {});\n`],
    ['it and double quotes', `${VITEST_IMPORT}it("settles a live run", () => {});\n`],
    ['a test declaration', `${VITEST_IMPORT}test('settles a live run', () => {});\n`],
  ])('accepts evidence whose test is declared with %s', (_label, source) => {
    const corpus = validCorpus();
    corpus['run-cancellation'] = validRecord({
      scenarioId: 'run-cancellation',
      implementationStatus: 'implemented',
      automatedEvidence: [{ path: 'src/runs.test.ts', testName: 'settles a live run' }],
      capabilityProbe: { requiredTypeDeclarations: ['RunLease'], requiredCommandNames: [] },
    });
    const root = writeCorpus(corpus);
    writeFile(root, 'src/runs.test.ts', source);
    declareType(root, 'RunLease');

    expect(check(root).errors).toEqual([]);
  });

  it.each([
    ['a describe block with no test inside', 'describe("settles a live run", () => {});\n'],
    ['a skipped test', "it.skip('settles a live run', () => {});\n"],
  ])('rejects evidence whose name is only %s', (_label, source) => {
    const corpus = validCorpus();
    corpus['run-cancellation'] = validRecord({
      scenarioId: 'run-cancellation',
      implementationStatus: 'implemented',
      automatedEvidence: [{ path: 'src/runs.test.ts', testName: 'settles a live run' }],
      capabilityProbe: { requiredTypeDeclarations: ['RunLease'], requiredCommandNames: [] },
    });
    const root = writeCorpus(corpus);
    writeFile(root, 'src/runs.test.ts', source);
    declareType(root, 'RunLease');

    expect(check(root).errors).toContain(
      `${subject('run-cancellation')} claims implemented but 'src/runs.test.ts' does not contain a test named 'settles a live run'`,
    );
  });

  it.each([
    ['a line comment', "// it('settles a live run', () => {});\n"],
    ['a block comment', "/* it('settles a live run', () => {}); */\n"],
    ['a string literal', "const source = \"it('settles a live run', () => {})\";\n"],
    ['a method call on another object', "helper.test('settles a live run', () => {});\n"],
    ['a call with no body argument', "it('settles a live run');\n"],
  ])('rejects a TypeScript test name that appears only as %s', (_label, source) => {
    const corpus = validCorpus();
    corpus['run-cancellation'] = validRecord({
      scenarioId: 'run-cancellation',
      implementationStatus: 'implemented',
      automatedEvidence: [{ path: 'src/runs.test.ts', testName: 'settles a live run' }],
      capabilityProbe: { requiredTypeDeclarations: ['RunLease'], requiredCommandNames: [] },
    });
    const root = writeCorpus(corpus);
    writeFile(root, 'src/runs.test.ts', source);
    declareType(root, 'RunLease');

    expect(check(root).errors).toContain(
      `${subject('run-cancellation')} claims implemented but 'src/runs.test.ts' does not contain a test named 'settles a live run'`,
    );
  });

  it.each([
    ['#[cfg(test)]', '#[cfg(test)]\nfn stop_settles_the_run() {}\n'],
    ['a doc comment naming test', '/// a test helper\nfn stop_settles_the_run() {}\n'],
    ['#[cfg(test)] above #[ignore]', '#[cfg(test)]\n#[ignore]\nfn stop_settles_the_run() {}\n'],
  ])('rejects a Rust function whose only nearby attribute is %s', (_label, source) => {
    const corpus = validCorpus();
    corpus['run-cancellation'] = validRecord({
      scenarioId: 'run-cancellation',
      implementationStatus: 'implemented',
      automatedEvidence: [{ path: 'src-tauri/src/runs_tests.rs', testName: 'stop_settles_the_run' }],
      capabilityProbe: { requiredTypeDeclarations: ['RunLease'], requiredCommandNames: [] },
    });
    const root = writeCorpus(corpus);
    writeFile(root, 'src-tauri/src/runs_tests.rs', source);
    declareType(root, 'RunLease');

    expect(check(root).errors).toContain(
      `${subject('run-cancellation')} claims implemented but 'src-tauri/src/runs_tests.rs' does not contain a test named 'stop_settles_the_run'`,
    );
  });

  it.each([
    [
      'a skipped suite',
      "describe.skip('runs', () => {\n  it('settles a live run', () => {});\n});\n",
    ],
    [
      'a function nobody calls',
      "function helper() {\n  it('settles a live run', () => {});\n}\n",
    ],
    [
      'a conditional branch',
      "if (flag) {\n  it('settles a live run', () => {});\n}\n",
    ],
  ])('rejects a TypeScript test that never runs because it sits inside %s', (_label, source) => {
    const corpus = validCorpus();
    corpus['run-cancellation'] = validRecord({
      scenarioId: 'run-cancellation',
      implementationStatus: 'implemented',
      automatedEvidence: [{ path: 'src/runs.test.ts', testName: 'settles a live run' }],
      capabilityProbe: { requiredTypeDeclarations: ['RunLease'], requiredCommandNames: [] },
    });
    const root = writeCorpus(corpus);
    writeFile(root, 'src/runs.test.ts', source);
    declareType(root, 'RunLease');

    expect(check(root).errors).toContain(
      `${subject('run-cancellation')} claims implemented but 'src/runs.test.ts' does not contain a test named 'settles a live run'`,
    );
  });

  it('accepts a TypeScript test nested inside a describe block that runs', () => {
    const corpus = validCorpus();
    corpus['run-cancellation'] = validRecord({
      scenarioId: 'run-cancellation',
      implementationStatus: 'implemented',
      automatedEvidence: [{ path: 'src/runs.test.ts', testName: 'settles a live run' }],
      capabilityProbe: { requiredTypeDeclarations: ['RunLease'], requiredCommandNames: [] },
    });
    const root = writeCorpus(corpus);
    writeFile(
      root,
      'src/runs.test.ts',
      `${VITEST_IMPORT}describe('runs', () => {\n  describe('cancellation', () => {\n    it('settles a live run', () => {});\n  });\n});\n`,
    );
    declareType(root, 'RunLease');

    expect(check(root).errors).toEqual([]);
  });

  it('rejects a Rust test marked #[ignore], which cargo does not run by default', () => {
    const corpus = validCorpus();
    corpus['run-cancellation'] = validRecord({
      scenarioId: 'run-cancellation',
      implementationStatus: 'implemented',
      automatedEvidence: [{ path: 'src-tauri/src/runs_tests.rs', testName: 'stop_settles_the_run' }],
      capabilityProbe: { requiredTypeDeclarations: ['RunLease'], requiredCommandNames: [] },
    });
    const root = writeCorpus(corpus);
    writeFile(root, 'src-tauri/src/runs_tests.rs', '#[test]\n#[ignore]\nfn stop_settles_the_run() {}\n');
    declareType(root, 'RunLease');

    expect(check(root).errors).toContain(
      `${subject('run-cancellation')} claims implemented but 'src-tauri/src/runs_tests.rs' does not contain a test named 'stop_settles_the_run'`,
    );
  });

  it('rejects a Rust test left inside a nested block comment', () => {
    const corpus = validCorpus();
    corpus['run-cancellation'] = validRecord({
      scenarioId: 'run-cancellation',
      implementationStatus: 'implemented',
      automatedEvidence: [{ path: 'src-tauri/src/runs_tests.rs', testName: 'stop_settles_the_run' }],
      capabilityProbe: { requiredTypeDeclarations: ['RunLease'], requiredCommandNames: [] },
    });
    const root = writeCorpus(corpus);
    writeFile(
      root,
      'src-tauri/src/runs_tests.rs',
      '/* parked /* inner */\n#[test]\nfn stop_settles_the_run() {}\n*/\n',
    );
    declareType(root, 'RunLease');

    expect(check(root).errors).toContain(
      `${subject('run-cancellation')} claims implemented but 'src-tauri/src/runs_tests.rs' does not contain a test named 'stop_settles_the_run'`,
    );
  });

  it('keeps a test whose name appears in a string literal elsewhere in the file', () => {
    const corpus = validCorpus();
    corpus['run-cancellation'] = validRecord({
      scenarioId: 'run-cancellation',
      implementationStatus: 'implemented',
      automatedEvidence: [{ path: 'src-tauri/src/runs_tests.rs', testName: 'stop_settles_the_run' }],
      capabilityProbe: { requiredTypeDeclarations: ['RunLease'], requiredCommandNames: [] },
    });
    const root = writeCorpus(corpus);
    writeFile(
      root,
      'src-tauri/src/runs_tests.rs',
      'const NOTE: &str = "/* not a comment */";\n#[test]\nfn stop_settles_the_run() {}\n',
    );
    declareType(root, 'RunLease');

    expect(check(root).errors).toEqual([]);
  });

  it.each([
    [
      'a local no-op that shadows the runner',
      "const test = (_name: string, _fn: () => void) => {};\ntest('settles a live run', () => {});\n",
    ],
    [
      'a runner imported from somewhere other than vitest',
      "import { it } from './fake-runner.ts';\nit('settles a live run', () => {});\n",
    ],
    [
      'no runner import at all',
      "it('settles a live run', () => {});\n",
    ],
  ])('rejects a TypeScript test whose runner is %s', (_label, source) => {
    const corpus = validCorpus();
    corpus['run-cancellation'] = validRecord({
      scenarioId: 'run-cancellation',
      implementationStatus: 'implemented',
      automatedEvidence: [{ path: 'src/runs.test.ts', testName: 'settles a live run' }],
      capabilityProbe: { requiredTypeDeclarations: ['RunLease'], requiredCommandNames: [] },
    });
    const root = writeCorpus(corpus);
    writeFile(root, 'src/runs.test.ts', source);
    declareType(root, 'RunLease');

    expect(check(root).errors).toContain(
      `${subject('run-cancellation')} claims implemented but 'src/runs.test.ts' does not contain a test named 'settles a live run'`,
    );
  });

  it.each([
    ['#[cfg_attr(test, ignore)]', '#[cfg_attr(test, ignore)]\n#[test]\nfn stop_settles_the_run() {}\n'],
    ['#[cfg_attr(unix, ignore)]', '#[test]\n#[cfg_attr(unix, ignore)]\nfn stop_settles_the_run() {}\n'],
    ['#[ignore = "slow"]', '#[test]\n#[ignore = "slow"]\nfn stop_settles_the_run() {}\n'],
    ['#[cfg(feature = "x")]', '#[cfg(feature = "x")]\n#[test]\nfn stop_settles_the_run() {}\n'],
    // Platform gates are refused too. Deciding them means resolving the whole cfg graph,
    // so campaign evidence must name an unconditional test — a cheap constraint on code
    // that is not written yet.
    ['#[cfg(unix)]', '#[cfg(unix)]\n#[test]\nfn stop_settles_the_run() {}\n'],
  ])('rejects a Rust test that cargo may skip because of %s', (_label, source) => {
    const corpus = validCorpus();
    corpus['run-cancellation'] = validRecord({
      scenarioId: 'run-cancellation',
      implementationStatus: 'implemented',
      automatedEvidence: [{ path: 'src-tauri/src/runs_tests.rs', testName: 'stop_settles_the_run' }],
      capabilityProbe: { requiredTypeDeclarations: ['RunLease'], requiredCommandNames: [] },
    });
    const root = writeCorpus(corpus);
    writeFile(root, 'src-tauri/src/runs_tests.rs', source);
    declareType(root, 'RunLease');

    expect(check(root).errors).toContain(
      `${subject('run-cancellation')} claims implemented but 'src-tauri/src/runs_tests.rs' does not contain a test named 'stop_settles_the_run'`,
    );
  });

  it('rejects a Rust function in a test file that carries no test attribute', () => {
    const corpus = validCorpus();
    corpus['run-cancellation'] = validRecord({
      scenarioId: 'run-cancellation',
      implementationStatus: 'implemented',
      automatedEvidence: [{ path: 'src-tauri/src/runs_tests.rs', testName: 'stop_settles_the_run' }],
      capabilityProbe: { requiredTypeDeclarations: ['RunLease'], requiredCommandNames: [] },
    });
    const root = writeCorpus(corpus);
    writeFile(root, 'src-tauri/src/runs_tests.rs', 'fn stop_settles_the_run() {}\n');
    declareType(root, 'RunLease');

    expect(check(root).errors).toContain(
      `${subject('run-cancellation')} claims implemented but 'src-tauri/src/runs_tests.rs' does not contain a test named 'stop_settles_the_run'`,
    );
  });

  it.each([
    ['#[test]', '#[test]\nfn stop_settles_the_run() {}\n'],
    ['#[tokio::test]', '#[tokio::test]\nasync fn stop_settles_the_run() {}\n'],
    [
      'a test attribute under other attributes',
      '#[test]\n#[should_panic]\nfn stop_settles_the_run() {}\n',
    ],
  ])('accepts Rust evidence declared with %s', (_label, source) => {
    const corpus = validCorpus();
    corpus['run-cancellation'] = validRecord({
      scenarioId: 'run-cancellation',
      implementationStatus: 'implemented',
      automatedEvidence: [{ path: 'src-tauri/src/runs_tests.rs', testName: 'stop_settles_the_run' }],
      capabilityProbe: { requiredTypeDeclarations: ['RunLease'], requiredCommandNames: [] },
    });
    const root = writeCorpus(corpus);
    writeFile(root, 'src-tauri/src/runs_tests.rs', source);
    declareType(root, 'RunLease');

    expect(check(root).errors).toEqual([]);
  });

  it('rejects an evidence entry that is not an object', () => {
    const corpus = validCorpus();
    corpus['run-cancellation'] = {
      ...validRecord({ scenarioId: 'run-cancellation' }),
      implementationStatus: 'implemented',
      automatedEvidence: ['scripts/docs/campaign-fixtures.test.ts'],
    };

    expect(check(writeCorpus(corpus)).errors).toContain(
      `${subject('run-cancellation')} automatedEvidence[0] must be an object with keys path and testName`,
    );
  });

  it.each([...EVIDENCE_KEYS])('rejects an evidence entry missing key %s', (key) => {
    const entry: Record<string, unknown> = { path: 'src/runs.test.ts', testName: 'a test' };
    delete entry[key];
    const corpus = validCorpus();
    corpus['run-cancellation'] = {
      ...validRecord({ scenarioId: 'run-cancellation' }),
      implementationStatus: 'implemented',
      automatedEvidence: [entry],
    };

    expect(check(writeCorpus(corpus)).errors).toContain(
      `${subject('run-cancellation')} automatedEvidence[0] is missing required key ${key}`,
    );
  });

  it('rejects an evidence entry carrying an extra key', () => {
    const corpus = validCorpus();
    corpus['run-cancellation'] = {
      ...validRecord({ scenarioId: 'run-cancellation' }),
      implementationStatus: 'implemented',
      automatedEvidence: [{ path: 'src/runs.test.ts', testName: 'a test', note: 'extra' }],
    };

    expect(check(writeCorpus(corpus)).errors).toContain(
      `${subject('run-cancellation')} automatedEvidence[0] has unexpected key note`,
    );
  });

  it.each([...EVIDENCE_KEYS])('rejects an empty evidence %s', (key) => {
    const corpus = validCorpus();
    corpus['run-cancellation'] = {
      ...validRecord({ scenarioId: 'run-cancellation' }),
      implementationStatus: 'implemented',
      automatedEvidence: [{ path: 'src/runs.test.ts', testName: 'a test', [key]: '  ' }],
    };

    expect(check(writeCorpus(corpus)).errors).toContain(
      `${subject('run-cancellation')} automatedEvidence[0] ${key} must be a non-empty string`,
    );
  });

  it('rejects a non-array automatedEvidence', () => {
    const corpus = validCorpus();
    corpus['run-cancellation'] = {
      ...validRecord({ scenarioId: 'run-cancellation' }),
      automatedEvidence: 'scripts/docs/campaign-fixtures.test.ts',
    };

    expect(check(writeCorpus(corpus)).errors).toContain(
      `${subject('run-cancellation')} automatedEvidence must be an array of { path, testName } objects`,
    );
  });

  it('does not report a present-but-malformed scenario as missing', () => {
    // A malformed record still fails, but the message must name its actual
    // defect. Reporting a file that exists as "missing" sends a reviewer
    // looking for the wrong problem.
    const corpus = validCorpus();
    corpus['grant-revocation'] = {
      ...validRecord({ scenarioId: 'grant-revocation' }),
      sneaky: true,
    };

    const errors = check(writeCorpus(corpus)).errors;

    expect(errors).toContain(`${subject('grant-revocation')} has unexpected key sneaky`);
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
      automatedEvidence: [{ path: 'src/features/runs/absent.test.ts', testName: 'a test' }],
    });

    expect(check(writeCorpus(corpus)).errors).toContain(
      `${subject('run-cancellation')} claims implemented but automatedEvidence path 'src/features/runs/absent.test.ts' must name an existing regular file`,
    );
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

  it.each(['ownedState', 'steps', 'expectedOutcome', 'mustNotHappen'])(
    'rejects a non-array %s',
    (key) => {
      const corpus = validCorpus();
      corpus['run-cancellation'] = { ...validRecord({ scenarioId: 'run-cancellation' }), [key]: 'one' };

      expect(check(writeCorpus(corpus)).errors).toContain(
        `${subject('run-cancellation')} ${key} must be an array of non-empty strings`,
      );
    },
  );

  it.each(['ownedState', 'steps', 'expectedOutcome', 'mustNotHappen'])(
    'rejects a non-string entry inside %s',
    (key) => {
      const corpus = validCorpus();
      corpus['run-cancellation'] = { ...validRecord({ scenarioId: 'run-cancellation' }), [key]: [7] };

      expect(check(writeCorpus(corpus)).errors).toContain(
        `${subject('run-cancellation')} ${key}[0] must be a non-empty string`,
      );
    },
  );

  it.each(['ownedState', 'steps', 'expectedOutcome', 'mustNotHappen'])(
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

describe('capabilityProbe validation', () => {
  it.each([['a string' as unknown], [42 as unknown], [null as unknown], [[] as unknown]])(
    'rejects a capabilityProbe that is not an object: %s',
    (capabilityProbe) => {
      const corpus = validCorpus();
      corpus['run-cancellation'] = {
        ...validRecord({ scenarioId: 'run-cancellation' }),
        capabilityProbe,
      };

      expect(check(writeCorpus(corpus)).errors).toContain(
        `${subject('run-cancellation')} capabilityProbe must be an object with keys requiredTypeDeclarations and requiredCommandNames`,
      );
    },
  );

  it.each([...PROBE_KEYS])('rejects a capabilityProbe missing key %s', (key) => {
    const probe: Record<string, unknown> = {
      requiredTypeDeclarations: ['RunLease'],
      requiredCommandNames: [],
    };
    delete probe[key];
    const corpus = validCorpus();
    corpus['run-cancellation'] = {
      ...validRecord({ scenarioId: 'run-cancellation' }),
      capabilityProbe: probe,
    };

    expect(check(writeCorpus(corpus)).errors).toContain(
      `${subject('run-cancellation')} capabilityProbe is missing required key ${key}`,
    );
  });

  it('rejects a capabilityProbe carrying an extra key', () => {
    const corpus = validCorpus();
    corpus['run-cancellation'] = {
      ...validRecord({ scenarioId: 'run-cancellation' }),
      capabilityProbe: {
        requiredTypeDeclarations: ['RunLease'],
        requiredCommandNames: [],
        requiredFiles: [],
      },
    };

    expect(check(writeCorpus(corpus)).errors).toContain(
      `${subject('run-cancellation')} capabilityProbe has unexpected key requiredFiles`,
    );
  });

  it.each([...PROBE_KEYS])('rejects a non-array capabilityProbe %s', (key) => {
    const corpus = validCorpus();
    corpus['run-cancellation'] = {
      ...validRecord({ scenarioId: 'run-cancellation' }),
      capabilityProbe: {
        requiredTypeDeclarations: [],
        requiredCommandNames: [],
        [key]: 'RunLease',
      },
    };

    expect(check(writeCorpus(corpus)).errors).toContain(
      `${subject('run-cancellation')} capabilityProbe ${key} must be an array of non-empty strings`,
    );
  });

  it.each([...PROBE_KEYS])('rejects an empty string inside capabilityProbe %s', (key) => {
    const corpus = validCorpus();
    corpus['run-cancellation'] = {
      ...validRecord({ scenarioId: 'run-cancellation' }),
      capabilityProbe: {
        requiredTypeDeclarations: [],
        requiredCommandNames: [],
        [key]: ['  '],
      },
    };

    expect(check(writeCorpus(corpus)).errors).toContain(
      `${subject('run-cancellation')} capabilityProbe ${key}[0] must be a non-empty string`,
    );
  });

  it('rejects a capabilityProbe that requires nothing at all', () => {
    const corpus = validCorpus();
    corpus['run-cancellation'] = validRecord({
      scenarioId: 'run-cancellation',
      capabilityProbe: { requiredTypeDeclarations: [], requiredCommandNames: [] },
    });

    expect(check(writeCorpus(corpus)).errors).toContain(
      `${subject('run-cancellation')} capabilityProbe must require at least one type declaration or one command substring`,
    );
  });

  it('accepts a capabilityProbe that requires only a command substring', () => {
    const corpus = validCorpus();
    corpus['legacy-session-migration'] = validRecord({
      scenarioId: 'legacy-session-migration',
      capabilityProbe: { requiredTypeDeclarations: [], requiredCommandNames: ['sessions_import'] },
    });

    expect(check(writeCorpus(corpus)).errors).toEqual([]);
  });
});

describe('probeScenarios', () => {
  it('does not treat ResearchRunLease as a declaration of RunLease', () => {
    // The reason the probe anchors on a declaration: a bare substring search
    // would read an unrelated existing type as proof the capability landed.
    const corpus = validCorpus();
    corpus['run-cancellation'] = validRecord({
      scenarioId: 'run-cancellation',
      capabilityProbe: { requiredTypeDeclarations: ['RunLease'], requiredCommandNames: [] },
    });
    const root = writeCorpus(corpus);
    writeFile(
      root,
      'src-tauri/src/research/run_registry.rs',
      'pub(crate) struct ResearchRunLease {\n    run_id: String,\n}\n',
    );

    const observation = probeScenarios({ root }).find(
      (entry) => entry.scenarioId === 'run-cancellation',
    );

    expect(observation?.satisfied).toBe(false);
    expect(observation?.missing).toEqual([
      "no 'struct RunLease' or 'enum RunLease' declaration under src-tauri/src",
    ]);
  });

  it.each([
    ['struct', 'pub struct RunLease {\n    id: String,\n}\n'],
    ['enum', 'pub enum RunLease {\n    Active,\n}\n'],
  ])('reports a probe satisfied once the type is declared as a %s', (_kind, source) => {
    const corpus = validCorpus();
    corpus['run-cancellation'] = validRecord({
      scenarioId: 'run-cancellation',
      capabilityProbe: { requiredTypeDeclarations: ['RunLease'], requiredCommandNames: [] },
    });
    const root = writeCorpus(corpus);
    writeFile(root, 'src-tauri/src/runs/lease.rs', source);

    const observation = probeScenarios({ root }).find(
      (entry) => entry.scenarioId === 'run-cancellation',
    );

    expect(observation).toEqual({ scenarioId: 'run-cancellation', satisfied: true, missing: [] });
  });

  it('reports a missing command name', () => {
    const corpus = validCorpus();
    corpus['legacy-session-migration'] = validRecord({
      scenarioId: 'legacy-session-migration',
      capabilityProbe: { requiredTypeDeclarations: [], requiredCommandNames: ['sessions_import'] },
    });
    const root = writeCorpus(corpus);
    writeAppCommands(root, ['sessions_list', 'sessions_load']);

    const observation = probeScenarios({ root }).find(
      (entry) => entry.scenarioId === 'legacy-session-migration',
    );

    expect(observation?.satisfied).toBe(false);
    expect(observation?.missing).toEqual(["no APP_COMMANDS entry named 'sessions_import'"]);
  });

  it('counts a command name only inside the APP_COMMANDS list', () => {
    const corpus = validCorpus();
    corpus['legacy-session-migration'] = validRecord({
      scenarioId: 'legacy-session-migration',
      capabilityProbe: { requiredTypeDeclarations: [], requiredCommandNames: ['sessions_import'] },
    });
    const root = writeCorpus(corpus);
    writeAppCommands(root, ['sessions_list'], 'const NOTE: &str = "sessions_import";\n');

    const outside = probeScenarios({ root }).find(
      (entry) => entry.scenarioId === 'legacy-session-migration',
    );
    expect(outside?.satisfied).toBe(false);

    writeAppCommands(root, ['sessions_list', 'sessions_import_all']);
    const partial = probeScenarios({ root }).find(
      (entry) => entry.scenarioId === 'legacy-session-migration',
    );
    expect(partial?.satisfied).toBe(false);

    writeAppCommands(root, ['sessions_list', 'sessions_import']);
    const inside = probeScenarios({ root }).find(
      (entry) => entry.scenarioId === 'legacy-session-migration',
    );
    expect(inside?.satisfied).toBe(true);
  });

  it('reports every unmet requirement of a multi-requirement probe', () => {
    const corpus = validCorpus();
    corpus['reference-folder-write-rejection'] = validRecord({
      scenarioId: 'reference-folder-write-rejection',
      capabilityProbe: {
        requiredTypeDeclarations: ['RunLease', 'FolderGrant'],
        requiredCommandNames: ['folder_reference'],
      },
    });
    const root = writeCorpus(corpus);
    declareType(root, 'RunLease');

    const observation = probeScenarios({ root }).find(
      (entry) => entry.scenarioId === 'reference-folder-write-rejection',
    );

    expect(observation?.missing).toEqual([
      "no 'struct FolderGrant' or 'enum FolderGrant' declaration under src-tauri/src",
      "no APP_COMMANDS entry named 'folder_reference'",
    ]);
  });

  it('observes every scenario of the real corpus and reports each as unsatisfied today', () => {
    const observations = probeScenarios({ root: process.cwd() });

    expect(observations.map((entry) => entry.scenarioId)).toEqual([...REQUIRED_SCENARIOS]);
    expect(observations.every((entry) => !entry.satisfied)).toBe(true);
  });

  it('renders a report a CLI can print', () => {
    const lines = renderProbeReport([
      {
        scenarioId: 'run-cancellation',
        satisfied: false,
        missing: ["no 'struct RunLease' declaration"],
      },
      { scenarioId: 'repeated-compaction', satisfied: true, missing: [] },
    ]);

    expect(lines).toEqual([
      'run-cancellation: unsatisfied',
      "  - no 'struct RunLease' declaration",
      'repeated-compaction: satisfied',
    ]);
  });
});

describe('probe and status agreement', () => {
  it('warns without failing when an unimplemented scenario has its prerequisites in the tree', () => {
    const corpus = validCorpus();
    corpus['run-cancellation'] = validRecord({
      scenarioId: 'run-cancellation',
      implementationStatus: 'unimplemented',
      capabilityProbe: { requiredTypeDeclarations: ['RunLease'], requiredCommandNames: [] },
    });
    const root = writeCorpus(corpus);
    declareType(root, 'RunLease');

    const result = check(root);
    expect(result.errors).toEqual([]);
    expect(result.warnings).toContain(
      `${subject('run-cancellation')} is unimplemented and its capability prerequisites are now present; check whether a passing test can flip it`,
    );
  });

  it('rejects an implemented scenario whose capability probe is unsatisfied, naming the gaps', () => {
    const corpus = validCorpus();
    corpus['run-cancellation'] = validRecord({
      scenarioId: 'run-cancellation',
      implementationStatus: 'implemented',
      automatedEvidence: [{ path: 'src/runs.test.ts', testName: 'settles a live run' }],
      capabilityProbe: {
        requiredTypeDeclarations: ['RunLease'],
        requiredCommandNames: ['run_cancel'],
      },
    });
    const root = writeCorpus(corpus);
    writeFile(root, 'src/runs.test.ts', `${VITEST_IMPORT}it('settles a live run', () => {});\n`);

    expect(check(root).errors).toContain(
      `${subject('run-cancellation')} claims implemented but its capability probe is unsatisfied: no 'struct RunLease' or 'enum RunLease' declaration under src-tauri/src; no APP_COMMANDS entry named 'run_cancel'`,
    );
  });

  it('accepts an implemented scenario whose probe and evidence both hold', () => {
    const corpus = validCorpus();
    corpus['run-cancellation'] = validRecord({
      scenarioId: 'run-cancellation',
      implementationStatus: 'implemented',
      automatedEvidence: [{ path: 'src/runs.test.ts', testName: 'settles a live run' }],
      capabilityProbe: {
        requiredTypeDeclarations: ['RunLease'],
        requiredCommandNames: ['run_cancel'],
      },
    });
    const root = writeCorpus(corpus);
    writeFile(root, 'src/runs.test.ts', `${VITEST_IMPORT}it('settles a live run', () => {});\n`);
    declareType(root, 'RunLease');
    writeAppCommands(root, ['run_cancel']);

    expect(check(root).errors).toEqual([]);
  });
});
