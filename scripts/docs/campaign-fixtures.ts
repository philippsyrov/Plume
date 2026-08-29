import { readFileSync, readdirSync, realpathSync, statSync, type Dirent } from 'node:fs';
import { isAbsolute, join, relative, resolve } from 'node:path';
import { declaresRunnableTest, stripRustNonCode } from './campaign-evidence.ts';

export const FIXTURE_STATUSES = ['unimplemented', 'implemented'] as const;

export const REQUIRED_SCENARIOS = [
  'grant-revocation',
  'legacy-session-migration',
  'memory-correction-and-forget',
  'reference-folder-write-rejection',
  'repeated-compaction',
  'run-cancellation',
] as const;

export type ScenarioId = (typeof REQUIRED_SCENARIOS)[number];

/**
 * The phase each scenario belongs to. Enforced rather than advisory: a fixture whose
 * phase drifts from the campaign plan would quietly reassign the work it describes.
 */
export const SCENARIO_PHASES: Readonly<Record<ScenarioId, number>> = {
  'grant-revocation': 4,
  'legacy-session-migration': 5,
  'memory-correction-and-forget': 3,
  'reference-folder-write-rejection': 6,
  'repeated-compaction': 2,
  'run-cancellation': 6,
};

export const FIXTURE_REVISION = 'v2';

export const FIXTURE_KEYS = [
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
] as const;

export const EVIDENCE_KEYS = ['path', 'testName'] as const;

export const PROBE_KEYS = ['requiredTypeDeclarations', 'requiredCommandNames'] as const;

export type FixtureCheckResult = {
  errors: string[];
  warnings: string[];
};

export type ProbeObservation = {
  scenarioId: string;
  satisfied: boolean;
  missing: string[];
};

const FIXTURE_DIRECTORY = 'docs/superpowers/fixtures/continuous-chat';

const RUST_SOURCE_DIRECTORY = 'src-tauri/src';

const APP_COMMANDS_FILE = 'src-tauri/src/app_commands.rs';

/**
 * The repository-wide ledger vocabulary from docs/FEATURE_INVENTORY.md. These words are
 * rejected with their own message so this corpus can never read as a competing ledger.
 */
const INVENTORY_STATUSES = [
  'shipped',
  'partial',
  'scaffold',
  'researched',
  'blocked',
  'retired',
] as const;

const REQUIRED_ARRAY_KEYS = ['ownedState', 'steps', 'expectedOutcome', 'mustNotHappen'] as const;

// TypeScript needs the suffix, because vitest only collects files matching it.
// Rust does not: this repo puts tests in `_tests.rs`, in `tests.rs`, and in
// inline `#[cfg(test)] mod tests` blocks inside ordinary source files, and
// excluding those would reject genuinely running tests. The real gate for Rust
// is the test attribute, checked in campaign-evidence.ts.
const TEST_FILE_SUFFIXES = ['.test.ts', '.test.tsx', '.spec.ts', '.spec.tsx', '.rs'] as const;

const MAX_PHASE = 9;

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function isNonEmptyString(value: unknown): value is string {
  return typeof value === 'string' && value.trim() !== '';
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

function validateExactKeys(
  record: Record<string, unknown>,
  expected: readonly string[],
  subject: string,
  errors: string[],
): boolean {
  let valid = true;
  const expectedSet = new Set<string>(expected);

  for (const key of expected) {
    if (key in record) continue;
    errors.push(`${subject} is missing required key ${key}`);
    valid = false;
  }

  for (const key of Object.keys(record)) {
    if (expectedSet.has(key)) continue;
    errors.push(`${subject} has unexpected key ${key}`);
    valid = false;
  }

  return valid;
}

function validateStringArray(
  value: unknown,
  key: string,
  subject: string,
  options: { allowEmpty: boolean },
  errors: string[],
): boolean {
  if (!Array.isArray(value)) {
    errors.push(`${subject} ${key} must be an array of non-empty strings`);
    return false;
  }

  if (!options.allowEmpty && value.length === 0) {
    errors.push(`${subject} ${key} must be a non-empty array of non-empty strings`);
    return false;
  }

  let valid = true;
  value.forEach((entry, index) => {
    if (isNonEmptyString(entry)) return;
    errors.push(`${subject} ${key}[${index}] must be a non-empty string`);
    valid = false;
  });
  return valid;
}

function validateStatus(record: Record<string, unknown>, subject: string, errors: string[]): void {
  const status = record.implementationStatus;

  if (typeof status === 'string' && (INVENTORY_STATUSES as readonly string[]).includes(status)) {
    errors.push(
      `${subject} implementationStatus '${status}' reuses the docs/FEATURE_INVENTORY.md status vocabulary; this corpus must never read as a competing status ledger`,
    );
    return;
  }

  if (typeof status !== 'string' || !(FIXTURE_STATUSES as readonly string[]).includes(status)) {
    const rendered = typeof status === 'string' ? status : JSON.stringify(status);
    errors.push(
      `${subject} implementationStatus '${rendered}' must be one of ${FIXTURE_STATUSES.join(', ')}`,
    );
  }
}

function isOutsideRoot(root: string, candidate: string): boolean {
  const fromRoot = relative(root, candidate);
  const upwards = `..${process.platform === 'win32' ? '\\' : '/'}`;
  return fromRoot === '..' || fromRoot.startsWith(upwards) || isAbsolute(fromRoot);
}

/**
 * Evidence must name a real test file inside the repository. An escaping path, a
 * directory, or an ordinary source file would satisfy a bare existence check and let a
 * scenario claim it is implemented without a test, which is the one thing this corpus
 * exists to prevent. Containment mirrors localPathError in roadmap-docs.ts.
 */
function evidencePathIssue(root: string, value: string): string | null {
  if (isAbsolute(value)) return 'must be repository-relative';

  const resolvedRoot = resolve(root);
  const candidate = resolve(resolvedRoot, value);
  if (isOutsideRoot(resolvedRoot, candidate)) return 'must stay inside the repository';

  try {
    const canonicalRoot = realpathSync(resolvedRoot);
    const canonicalCandidate = realpathSync(candidate);
    if (isOutsideRoot(canonicalRoot, canonicalCandidate)) {
      return 'must stay inside the repository';
    }
    if (!statSync(canonicalCandidate).isFile()) return 'must name an existing regular file';
  } catch {
    return 'must name an existing regular file';
  }

  if (!TEST_FILE_SUFFIXES.some((suffix) => value.endsWith(suffix))) {
    return `must name a test file (${TEST_FILE_SUFFIXES.join(', ')})`;
  }
  return null;
}

/**
 * Evidence claims a named test exists, so the checker reads the file and looks for a
 * declaration carrying that name. Matching the bare name anywhere in the file would let
 * a comment or an unrelated string stand in for a test that was never written.
 */
function fileDeclaresTest(root: string, value: string, testName: string): boolean {
  let source: string;
  try {
    source = readFileSync(resolve(resolve(root), value), 'utf8');
  } catch {
    return false;
  }

  return declaresRunnableTest(source, testName, value);
}

function validateEvidence(
  root: string,
  record: Record<string, unknown>,
  subject: string,
  errors: string[],
): void {
  const evidence = record.automatedEvidence;
  if (!Array.isArray(evidence)) {
    errors.push(`${subject} automatedEvidence must be an array of { path, testName } objects`);
    return;
  }

  const entries: { path: string; testName: string }[] = [];
  let shapeValid = true;

  evidence.forEach((entry, index) => {
    const entrySubject = `${subject} automatedEvidence[${index}]`;
    if (!isObject(entry)) {
      errors.push(`${entrySubject} must be an object with keys path and testName`);
      shapeValid = false;
      return;
    }
    if (!validateExactKeys(entry, EVIDENCE_KEYS, entrySubject, errors)) {
      shapeValid = false;
      return;
    }

    let entryValid = true;
    for (const key of EVIDENCE_KEYS) {
      if (isNonEmptyString(entry[key])) continue;
      errors.push(`${entrySubject} ${key} must be a non-empty string`);
      entryValid = false;
    }
    if (!entryValid) {
      shapeValid = false;
      return;
    }

    entries.push({ path: String(entry.path), testName: String(entry.testName) });
  });

  // A scenario is flipped in the same commit as its test, so evidence attached
  // to an unimplemented scenario means the status and the test disagree.
  if (record.implementationStatus !== 'implemented') {
    if (record.implementationStatus === 'unimplemented' && evidence.length > 0) {
      errors.push(`${subject} is unimplemented so automatedEvidence must be empty`);
    }
    return;
  }

  if (evidence.length === 0) {
    errors.push(`${subject} claims implemented but automatedEvidence is empty`);
    return;
  }

  if (!shapeValid) return;

  for (const entry of entries) {
    const issue = evidencePathIssue(root, entry.path);
    if (issue !== null) {
      errors.push(
        `${subject} claims implemented but automatedEvidence path '${entry.path}' ${issue}`,
      );
      continue;
    }
    if (fileDeclaresTest(root, entry.path, entry.testName)) continue;
    errors.push(
      `${subject} claims implemented but '${entry.path}' does not contain a test named '${entry.testName}'`,
    );
  }
}

type CapabilityProbe = {
  requiredTypeDeclarations: string[];
  requiredCommandNames: string[];
};

function readProbe(value: unknown): CapabilityProbe | null {
  if (!isObject(value)) return null;

  const declarations = value.requiredTypeDeclarations;
  const substrings = value.requiredCommandNames;
  if (!Array.isArray(declarations) || !Array.isArray(substrings)) return null;
  if (!declarations.every(isNonEmptyString) || !substrings.every(isNonEmptyString)) return null;

  return {
    requiredTypeDeclarations: [...declarations],
    requiredCommandNames: [...substrings],
  };
}

function validateProbe(
  record: Record<string, unknown>,
  subject: string,
  errors: string[],
): CapabilityProbe | null {
  const probe = record.capabilityProbe;
  if (!isObject(probe)) {
    errors.push(`${subject} capabilityProbe must be an object with keys ${PROBE_KEYS.join(' and ')}`);
    return null;
  }

  const probeSubject = `${subject} capabilityProbe`;
  if (!validateExactKeys(probe, PROBE_KEYS, probeSubject, errors)) return null;

  let valid = true;
  for (const key of PROBE_KEYS) {
    if (validateStringArray(probe[key], key, probeSubject, { allowEmpty: true }, errors)) continue;
    valid = false;
  }
  if (!valid) return null;

  const parsed = readProbe(probe);
  if (parsed === null) return null;

  if (parsed.requiredTypeDeclarations.length === 0 && parsed.requiredCommandNames.length === 0) {
    errors.push(
      `${probeSubject} must require at least one type declaration or one command substring`,
    );
    return null;
  }
  return parsed;
}

function collectRustSources(directory: string): string[] {
  let entries: Dirent[];
  try {
    entries = readdirSync(directory, { withFileTypes: true });
  } catch {
    return [];
  }

  const sources: string[] = [];
  for (const entry of entries) {
    const full = join(directory, entry.name);
    if (entry.isDirectory()) {
      sources.push(...collectRustSources(full));
      continue;
    }
    if (!entry.isFile() || !entry.name.endsWith('.rs')) continue;
    try {
      sources.push(stripNonProductionRust(readFileSync(full, 'utf8')));
    } catch {
      continue;
    }
  }
  return sources;
}

function readAppCommandStrings(root: string): string[] {
  let source: string;
  try {
    source = readFileSync(join(root, APP_COMMANDS_FILE), 'utf8');
  } catch {
    return [];
  }

  const start = source.indexOf('APP_COMMANDS');
  if (start === -1) return [];
  const open = source.indexOf('[', start);
  if (open === -1) return [];

  const close = source.indexOf('];', open);
  const block = source.slice(open, close === -1 ? source.length : close);
  return [...block.matchAll(/"([^"\\]*)"/g)].map((match) => match[1] ?? '');
}

type ProbeContext = {
  rustSources: string[];
  commandStrings: string[];
};

function loadProbeContext(root: string): ProbeContext {
  return {
    rustSources: collectRustSources(join(root, RUST_SOURCE_DIRECTORY)),
    commandStrings: readAppCommandStrings(root),
  };
}

/**
 * A type counts as present only when a `struct`/`enum` declaration carries exactly that
 * name. A bare substring search would let `ResearchRunLease` answer a probe for
 * `RunLease` and report an unbuilt capability as landed.
 */
function declaresType(context: ProbeContext, name: string): boolean {
  const pattern = new RegExp(`\\b(?:struct|enum)\\s+${escapeRegExp(name)}\\b`);
  return context.rustSources.some((source) => pattern.test(source));
}

/**
 * Test-only and conditionally-compiled code is not the production capability.
 *
 * A probe answers "does this exist yet?", and a type declared inside
 * `#[cfg(test)] mod tests` — or behind any other `cfg` — does not exist for the
 * shipped app. Counting it would let a fixture claim a capability landed
 * because a fixture for it landed.
 */
function stripNonProductionRust(source: string): string {
  // Comments go first, then string bodies: `"struct RunLease"` is data, not a
  // declaration, and a probe that counts it reports a capability that does not
  // exist. Lengths are preserved so line structure survives.
  const withoutText = stripRustNonCode(source).replace(
    /"(?:[^"\\]|\\.)*"/g,
    (match) => `"${' '.repeat(Math.max(match.length - 2, 0))}"`,
  );
  const lines = withoutText.split('\n');
  const kept: string[] = [];
  let skipDepth: number | null = null;
  let depth = 0;

  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index] ?? '';
    const opens = (line.match(/\{/g) ?? []).length;
    const closes = (line.match(/\}/g) ?? []).length;

    if (skipDepth === null && /^\s*#\[cfg\(/.test(line)) {
      // The attribute may sit a line or two above the item it gates.
      const ahead = lines.slice(index, index + 4).join('\n');
      if (/\bmod\s+\w+/.test(ahead) || /\b(struct|enum)\s+\w+/.test(ahead)) {
        skipDepth = depth;
        continue;
      }
    }

    if (skipDepth === null) kept.push(line);
    depth += opens - closes;
    if (skipDepth !== null && depth <= skipDepth && closes > 0) skipDepth = null;
  }

  return kept.join('\n');
}

function evaluateProbe(context: ProbeContext, probe: CapabilityProbe): string[] {
  const missing: string[] = [];

  for (const name of probe.requiredTypeDeclarations) {
    if (declaresType(context, name)) continue;
    missing.push(`no 'struct ${name}' or 'enum ${name}' declaration under ${RUST_SOURCE_DIRECTORY}`);
  }

  // Exact names, not substrings: a probe for 'import' would be answered by any unrelated
  // command whose name happens to contain it.
  for (const name of probe.requiredCommandNames) {
    if (context.commandStrings.includes(name)) continue;
    missing.push(`no APP_COMMANDS entry named '${name}'`);
  }

  return missing;
}

function readFixtureRecords(root: string): { stem: string; record: Record<string, unknown> }[] {
  const directory = join(root, FIXTURE_DIRECTORY);

  let fileNames: string[];
  try {
    fileNames = readdirSync(directory, { withFileTypes: true })
      .filter((entry) => entry.isFile() && entry.name.endsWith('.json'))
      .map((entry) => entry.name)
      .sort();
  } catch {
    return [];
  }

  const records: { stem: string; record: Record<string, unknown> }[] = [];
  for (const fileName of fileNames) {
    let parsed: unknown;
    try {
      parsed = JSON.parse(readFileSync(join(directory, fileName), 'utf8'));
    } catch {
      continue;
    }
    if (!isObject(parsed)) continue;
    records.push({ stem: fileName.slice(0, -'.json'.length), record: parsed });
  }
  return records;
}

/** Evaluates every scenario's capability probe against the real source tree. */
export function probeScenarios(options: { root: string }): ProbeObservation[] {
  const context = loadProbeContext(options.root);
  const observations: ProbeObservation[] = [];

  for (const { stem, record } of readFixtureRecords(options.root)) {
    const probe = readProbe(record.capabilityProbe);
    if (probe === null) continue;

    const scenarioId = isNonEmptyString(record.scenarioId) ? record.scenarioId : stem;
    const missing = evaluateProbe(context, probe);
    observations.push({ scenarioId, satisfied: missing.length === 0, missing });
  }

  return observations.sort((left, right) => left.scenarioId.localeCompare(right.scenarioId));
}

/** Renders a probe report as plain text lines for a CLI to print. */
export function renderProbeReport(observations: readonly ProbeObservation[]): string[] {
  const lines: string[] = [];
  for (const observation of observations) {
    lines.push(`${observation.scenarioId}: ${observation.satisfied ? 'satisfied' : 'unsatisfied'}`);
    for (const requirement of observation.missing) {
      lines.push(`  - ${requirement}`);
    }
  }
  return lines;
}

/**
 * The probe is one-directional on purpose. Prerequisites missing is proof the scenario
 * cannot pass, so an `implemented` claim over a missing prerequisite is a hard error. The
 * converse does not hold: a `FolderGrant` type can exist for many commits before revocation
 * actually works, and failing the build the moment it appears would forbid ordinary TDD.
 * Only a passing test flips a status, so the present-prerequisite case is a warning.
 */
function validateProbeAgainstTree(
  context: ProbeContext,
  probe: CapabilityProbe,
  status: unknown,
  subject: string,
  errors: string[],
  warnings: string[],
): void {
  const missing = evaluateProbe(context, probe);

  if (status === 'unimplemented' && missing.length === 0) {
    warnings.push(
      `${subject} is unimplemented and its capability prerequisites are now present; check whether a passing test can flip it`,
    );
    return;
  }

  if (status === 'implemented' && missing.length > 0) {
    errors.push(
      `${subject} claims implemented but its capability probe is unsatisfied: ${missing.join('; ')}`,
    );
  }
}

function validateRecord(
  root: string,
  context: ProbeContext,
  record: Record<string, unknown>,
  stem: string,
  subject: string,
  seen: Set<string>,
  errors: string[],
  warnings: string[],
): void {
  if (!validateExactKeys(record, FIXTURE_KEYS, subject, errors)) return;

  const scenarioId = record.scenarioId;
  if (!isNonEmptyString(scenarioId)) {
    errors.push(`${subject} scenarioId must be a non-empty string`);
  } else {
    if (scenarioId !== stem) {
      errors.push(`${subject} scenarioId '${scenarioId}' must equal the filename stem '${stem}'`);
    }
    if (seen.has(scenarioId)) {
      errors.push(
        `${subject} repeats scenarioId '${scenarioId}' already declared by another fixture`,
      );
    } else {
      seen.add(scenarioId);
    }
  }

  const known = (REQUIRED_SCENARIOS as readonly string[]).includes(stem);
  if (!known) {
    errors.push(`${subject} declares unknown scenarioId '${stem}'`);
  }

  if (record.fixtureRevision !== FIXTURE_REVISION) {
    const revision = record.fixtureRevision;
    const rendered = typeof revision === 'string' ? revision : JSON.stringify(revision);
    errors.push(`${subject} fixtureRevision '${rendered}' must be '${FIXTURE_REVISION}'`);
  }

  if (!isNonEmptyString(record.intent)) {
    errors.push(`${subject} intent must be a non-empty string`);
  }

  const phase = record.phase;
  if (typeof phase !== 'number' || !Number.isInteger(phase) || phase < 0 || phase > MAX_PHASE) {
    errors.push(`${subject} phase must be an integer between 0 and ${MAX_PHASE}`);
  } else if (known) {
    const expected = SCENARIO_PHASES[stem as ScenarioId];
    if (phase !== expected) {
      errors.push(
        `${subject} phase ${phase} must equal the canonical phase ${expected} for scenario '${stem}'`,
      );
    }
  }

  for (const key of REQUIRED_ARRAY_KEYS) {
    validateStringArray(record[key], key, subject, { allowEmpty: false }, errors);
  }

  validateStatus(record, subject, errors);
  validateEvidence(root, record, subject, errors);

  const probe = validateProbe(record, subject, errors);
  if (probe === null) return;
  validateProbeAgainstTree(context, probe, record.implementationStatus, subject, errors, warnings);
}

export function checkCampaignFixtures(options: { root: string }): FixtureCheckResult {
  const errors: string[] = [];
  const warnings: string[] = [];
  const directory = join(options.root, FIXTURE_DIRECTORY);

  let fileNames: string[];
  try {
    fileNames = readdirSync(directory, { withFileTypes: true })
      .filter((entry) => entry.isFile() && entry.name.endsWith('.json'))
      .map((entry) => entry.name)
      .sort();
  } catch {
    errors.push(`${FIXTURE_DIRECTORY} could not be read`);
    return { errors, warnings };
  }

  const context = loadProbeContext(options.root);
  const seen = new Set<string>();
  // Presence is a filesystem fact; validity is a record fact. Keeping them
  // apart stops a malformed-but-present fixture from also being reported as
  // missing, which would send a reviewer looking for the wrong problem.
  const presentStems = new Set<string>();

  for (const fileName of fileNames) {
    const stem = fileName.slice(0, -'.json'.length);
    presentStems.add(stem);
    const subject = `${FIXTURE_DIRECTORY}/${fileName}`;

    let raw: string;
    try {
      raw = readFileSync(join(directory, fileName), 'utf8');
    } catch {
      errors.push(`${subject} could not be read`);
      continue;
    }

    let parsed: unknown;
    try {
      parsed = JSON.parse(raw);
    } catch {
      errors.push(`${subject} must contain valid JSON`);
      continue;
    }

    if (!isObject(parsed)) {
      errors.push(`${subject} must contain a JSON object`);
      continue;
    }

    validateRecord(options.root, context, parsed, stem, subject, seen, errors, warnings);
  }

  for (const scenarioId of REQUIRED_SCENARIOS) {
    if (presentStems.has(scenarioId)) continue;
    errors.push(`${FIXTURE_DIRECTORY} is missing required scenario '${scenarioId}'`);
  }

  return { errors, warnings };
}
