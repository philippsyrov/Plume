import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';

export const FIXTURE_STATUSES = ['unimplemented', 'implemented'] as const;

export const REQUIRED_SCENARIOS = [
  'grant-revocation',
  'legacy-session-migration',
  'memory-correction-and-forget',
  'reference-folder-write-rejection',
  'repeated-compaction',
  'run-cancellation',
] as const;

export const FIXTURE_KEYS = [
  'scenarioId',
  'fixtureRevision',
  'phase',
  'intent',
  'ownedState',
  'steps',
  'expectedOutcome',
  'mustNotHappen',
  'implementationStatus',
  'automatedEvidence',
] as const;

export type FixtureCheckResult = {
  errors: string[];
  warnings: string[];
};

const FIXTURE_DIRECTORY = 'docs/superpowers/fixtures/continuous-chat';

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

const MAX_PHASE = 9;

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function isNonEmptyString(value: unknown): value is string {
  return typeof value === 'string' && value.trim() !== '';
}

function validateExactKeys(
  record: Record<string, unknown>,
  subject: string,
  errors: string[],
): boolean {
  let valid = true;
  const expected = new Set<string>(FIXTURE_KEYS);

  for (const key of FIXTURE_KEYS) {
    if (key in record) continue;
    errors.push(`${subject} is missing required key ${key}`);
    valid = false;
  }

  for (const key of Object.keys(record)) {
    if (expected.has(key)) continue;
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
): void {
  if (!Array.isArray(value)) {
    errors.push(`${subject} ${key} must be an array of non-empty strings`);
    return;
  }

  if (!options.allowEmpty && value.length === 0) {
    errors.push(`${subject} ${key} must be a non-empty array of non-empty strings`);
    return;
  }

  value.forEach((entry, index) => {
    if (isNonEmptyString(entry)) return;
    errors.push(`${subject} ${key}[${index}] must be a non-empty string`);
  });
}

function validateStatus(
  record: Record<string, unknown>,
  subject: string,
  errors: string[],
): void {
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

function validateEvidence(
  root: string,
  record: Record<string, unknown>,
  subject: string,
  errors: string[],
): void {
  if (record.implementationStatus !== 'implemented') return;

  const evidence = record.automatedEvidence;
  if (!Array.isArray(evidence)) return;

  if (evidence.length === 0) {
    errors.push(`${subject} claims implemented but automatedEvidence is empty`);
    return;
  }

  for (const entry of evidence) {
    if (!isNonEmptyString(entry)) continue;
    if (existsSync(resolve(root, entry))) continue;
    errors.push(
      `${subject} claims implemented but automatedEvidence path '${entry}' does not exist on disk`,
    );
  }
}

function validateRecord(
  root: string,
  record: Record<string, unknown>,
  stem: string,
  subject: string,
  seen: Set<string>,
  errors: string[],
): void {
  if (!validateExactKeys(record, subject, errors)) return;

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

  if (!(REQUIRED_SCENARIOS as readonly string[]).includes(stem)) {
    errors.push(`${subject} declares unknown scenarioId '${stem}'`);
  }

  if (!isNonEmptyString(record.fixtureRevision)) {
    errors.push(`${subject} fixtureRevision must be a non-empty string`);
  }

  if (!isNonEmptyString(record.intent)) {
    errors.push(`${subject} intent must be a non-empty string`);
  }

  const phase = record.phase;
  if (typeof phase !== 'number' || !Number.isInteger(phase) || phase < 0 || phase > MAX_PHASE) {
    errors.push(`${subject} phase must be an integer between 0 and ${MAX_PHASE}`);
  }

  for (const key of REQUIRED_ARRAY_KEYS) {
    validateStringArray(record[key], key, subject, { allowEmpty: false }, errors);
  }
  validateStringArray(record.automatedEvidence, 'automatedEvidence', subject, { allowEmpty: true }, errors);

  validateStatus(record, subject, errors);
  validateEvidence(root, record, subject, errors);
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

    validateRecord(options.root, parsed, stem, subject, seen, errors);
  }

  for (const scenarioId of REQUIRED_SCENARIOS) {
    if (presentStems.has(scenarioId)) continue;
    errors.push(`${FIXTURE_DIRECTORY} is missing required scenario '${scenarioId}'`);
  }

  return { errors, warnings };
}
