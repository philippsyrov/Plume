import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';

export const FEATURE_STATUSES = [
  'shipped',
  'partial',
  'scaffold',
  'researched',
  'blocked',
  'retired',
] as const;

export type DocsCheckResult = {
  errors: string[];
  warnings: string[];
};

type GitRunner = (args: string[]) => { ok: boolean; stdout: string };

type InventoryRecord = {
  id: string;
  status: string;
  automatedEvidence: string[];
  implementationPaths: string[];
  lastVerifiedCommit: string;
};

const INVENTORY_KEYS = [
  'id',
  'track',
  'status',
  'currentBehavior',
  'missingBehavior',
  'frontendReachability',
  'backendReachability',
  'automatedEvidence',
  'manualOrHardwareEvidence',
  'dependencies',
  'implementationPaths',
  'sourceDocuments',
  'nextCommissionedSlice',
  'lastVerifiedCommit',
  'lastVerifiedDate',
] as const;

const INVENTORY_STRING_KEYS = [
  'id',
  'track',
  'status',
  'currentBehavior',
  'missingBehavior',
  'frontendReachability',
  'backendReachability',
  'manualOrHardwareEvidence',
  'nextCommissionedSlice',
  'lastVerifiedCommit',
  'lastVerifiedDate',
] as const;

const INVENTORY_ARRAY_KEYS = [
  'automatedEvidence',
  'dependencies',
  'implementationPaths',
  'sourceDocuments',
] as const;

const RESEARCH_KEYS = ['family', 'sourceDate', 'hygiene', 'sources', 'refreshTrigger'] as const;

const RESEARCH_HYGIENE = [
  'official-public',
  'local-observation',
  'clean-room-reference',
  'behavior-report-only',
  'do-not-use-source',
] as const;

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function fencedJson(markdown: string, label: string): string[] {
  const pattern = new RegExp('^```' + label + '\\s*\\r?\\n([\\s\\S]*?)^```\\s*$', 'gm');
  return [...markdown.matchAll(pattern)].map((match) => match[1] ?? '');
}

function markdownFiles(root: string, directory: 'research' | 'archive'): string[] {
  const relativeDirectory = `docs/${directory}`;
  return readdirSync(join(root, relativeDirectory), { withFileTypes: true })
    .filter((entry) => entry.isFile() && entry.name.endsWith('.md') && entry.name !== 'README.md')
    .map((entry) => `${relativeDirectory}/${entry.name}`)
    .sort();
}

function recordId(value: Record<string, unknown>, index: number): string {
  return typeof value.id === 'string' && value.id.length > 0 ? value.id : `at index ${index}`;
}

function validateExactKeys(
  value: Record<string, unknown>,
  expected: readonly string[],
  subject: string,
  errors: string[],
): boolean {
  let valid = true;
  const expectedSet = new Set(expected);

  for (const key of expected) {
    if (!(key in value)) {
      errors.push(`${subject} is missing required key ${key}`);
      valid = false;
    }
  }

  for (const key of Object.keys(value)) {
    if (!expectedSet.has(key)) {
      errors.push(`${subject} has unexpected key ${key}`);
      valid = false;
    }
  }

  return valid;
}

function parseInventory(root: string, errors: string[]): InventoryRecord[] {
  const path = 'docs/FEATURE_INVENTORY.md';
  let markdown: string;

  try {
    markdown = readFileSync(join(root, path), 'utf8');
  } catch {
    errors.push(`${path} could not be read`);
    return [];
  }

  const fences = fencedJson(markdown, 'inventory-json');
  if (fences.length !== 1) {
    errors.push(`${path} must contain exactly one inventory-json fence`);
    return [];
  }

  let parsed: unknown;
  try {
    parsed = JSON.parse(fences[0] ?? '');
  } catch {
    errors.push(`${path} inventory-json fence must contain valid JSON`);
    return [];
  }

  if (!Array.isArray(parsed)) {
    errors.push(`${path} inventory-json fence must contain a JSON array`);
    return [];
  }

  const records: InventoryRecord[] = [];

  parsed.forEach((value, index) => {
    if (!isObject(value)) {
      errors.push(`inventory record at index ${index} must be an object`);
      return;
    }

    const id = recordId(value, index);
    const subject = `inventory record ${id}`;
    const exactKeys = validateExactKeys(value, INVENTORY_KEYS, subject, errors);
    let validTypes = true;

    for (const key of INVENTORY_STRING_KEYS) {
      if (key in value && typeof value[key] !== 'string') {
        errors.push(`${subject} key ${key} must be a string`);
        validTypes = false;
      }
    }

    for (const key of INVENTORY_ARRAY_KEYS) {
      if (key in value && (!Array.isArray(value[key]) || !value[key].every((item) => typeof item === 'string'))) {
        errors.push(`${subject} key ${key} must be an array of strings`);
        validTypes = false;
      }
    }

    if (typeof value.status === 'string' && !FEATURE_STATUSES.includes(value.status as (typeof FEATURE_STATUSES)[number])) {
      errors.push(`${subject} has unknown status '${value.status}'`);
    }

    if (value.status === 'shipped' && Array.isArray(value.automatedEvidence) && value.automatedEvidence.length === 0) {
      errors.push(`shipped ${subject} must name automatedEvidence`);
    }

    if (!exactKeys || !validTypes) return;

    records.push({
      id: value.id as string,
      status: value.status as string,
      automatedEvidence: value.automatedEvidence as string[],
      implementationPaths: value.implementationPaths as string[],
      lastVerifiedCommit: value.lastVerifiedCommit as string,
    });
  });

  return records;
}

function checkResearch(root: string, errors: string[]): void {
  let paths: string[];

  try {
    paths = markdownFiles(root, 'research');
  } catch {
    errors.push('docs/research could not be read');
    return;
  }

  for (const path of paths) {
    const fences = fencedJson(readFileSync(join(root, path), 'utf8'), 'research-metadata');
    if (fences.length !== 1) {
      errors.push(`${path} must contain exactly one research-metadata fence`);
      continue;
    }

    let metadata: unknown;
    try {
      metadata = JSON.parse(fences[0] ?? '');
    } catch {
      errors.push(`${path} research-metadata fence must contain valid JSON`);
      continue;
    }

    if (!isObject(metadata)) {
      errors.push(`${path} research-metadata fence must contain a JSON object`);
      continue;
    }

    validateExactKeys(metadata, RESEARCH_KEYS, path, errors);

    if (typeof metadata.hygiene === 'string' && !RESEARCH_HYGIENE.includes(metadata.hygiene as (typeof RESEARCH_HYGIENE)[number])) {
      errors.push(`${path} has unknown research hygiene '${metadata.hygiene}'`);
    }

    for (const key of ['family', 'sourceDate', 'hygiene', 'refreshTrigger'] as const) {
      if (key in metadata && typeof metadata[key] !== 'string') errors.push(`${path} key ${key} must be a string`);
    }

    if ('sources' in metadata && (!Array.isArray(metadata.sources) || !metadata.sources.every((source) => typeof source === 'string'))) {
      errors.push(`${path} key sources must be an array of strings`);
    }
  }
}

function checkArchive(root: string, errors: string[]): void {
  let paths: string[];

  try {
    paths = markdownFiles(root, 'archive');
  } catch {
    errors.push('docs/archive could not be read');
    return;
  }

  for (const path of paths) {
    const markdown = readFileSync(join(root, path), 'utf8');
    if (!/^Replacement:/m.test(markdown)) {
      errors.push(`${path} must contain a line beginning Replacement:`);
    }
  }
}

function checkFreshness(records: InventoryRecord[], git: GitRunner, warnings: string[]): void {
  for (const record of records) {
    if (!['shipped', 'partial', 'scaffold'].includes(record.status)) continue;

    const ancestor = git(['merge-base', '--is-ancestor', record.lastVerifiedCommit, 'HEAD']);
    if (!ancestor.ok) {
      warnings.push(
        `inventory record ${record.id} cannot verify ${record.lastVerifiedCommit}: commit is missing or is not an ancestor of HEAD`,
      );
      continue;
    }

    const changed = git([
      'diff',
      '--name-only',
      `${record.lastVerifiedCommit}..HEAD`,
      '--',
      ...record.implementationPaths,
    ]);
    if (!changed.ok) {
      warnings.push(`inventory record ${record.id} could not compare owned paths since ${record.lastVerifiedCommit}`);
      continue;
    }

    const ownedChanges = changed.stdout.split(/\r?\n/).filter((path) => path.length > 0);

    for (const path of ownedChanges) {
      warnings.push(
        `inventory record ${record.id} may be stale: owned path changed since ${record.lastVerifiedCommit}: ${path}`,
      );
    }
  }
}

export function checkRoadmapDocs(options: { root: string; git: GitRunner }): DocsCheckResult {
  const errors: string[] = [];
  const warnings: string[] = [];
  const records = parseInventory(options.root, errors);

  checkResearch(options.root, errors);
  checkArchive(options.root, errors);
  checkFreshness(records, options.git, warnings);

  return { errors, warnings };
}
