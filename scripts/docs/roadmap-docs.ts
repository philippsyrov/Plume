import { readFileSync, readdirSync, realpathSync, statSync } from 'node:fs';
import { dirname, isAbsolute, join, relative, resolve } from 'node:path';

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

type PathKind = 'file' | 'fileOrDirectory';

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

const REQUIRED_NAVIGATION_FILES = [
  'src/features/README.md',
  'src-tauri/src/README.md',
  'docs/history/slice-ledger.md',
] as const;

const DOMAIN_MAP_FILES = ['src/features/README.md', 'src-tauri/src/README.md'] as const;

const AGENTS_LINE_HARD_CAP = 400;

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function isOutsideRoot(root: string, candidate: string): boolean {
  const fromRoot = relative(root, candidate);
  return fromRoot === '..' || fromRoot.startsWith(`..${process.platform === 'win32' ? '\\' : '/'}`) || isAbsolute(fromRoot);
}

function isHttpUrl(value: string): boolean {
  try {
    const protocol = new URL(value).protocol;
    return protocol === 'http:' || protocol === 'https:';
  } catch {
    return false;
  }
}

function localPathError(
  root: string,
  baseDirectory: string,
  value: string,
  kind: PathKind,
): string | null {
  if (value.trim() === '') return 'must be non-empty';
  if (isAbsolute(value)) return 'must be repository-relative';

  const resolvedRoot = resolve(root);
  const candidate = resolve(baseDirectory, value);
  if (isOutsideRoot(resolvedRoot, candidate)) return 'must stay inside the repository';

  try {
    const canonicalRoot = realpathSync(resolvedRoot);
    const canonicalCandidate = realpathSync(candidate);
    if (isOutsideRoot(canonicalRoot, canonicalCandidate)) return 'must stay inside the repository';

    const stats = statSync(canonicalCandidate);
    if (kind === 'file' && !stats.isFile()) return 'must name an existing regular file';
    if (kind === 'fileOrDirectory' && !stats.isFile() && !stats.isDirectory()) {
      return 'must name an existing file or directory';
    }
  } catch {
    return kind === 'file' ? 'must name an existing regular file' : 'must name an existing file or directory';
  }
  return null;
}

function isRealCalendarDate(value: string): boolean {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value);
  if (match === null) return false;

  const year = Number(match[1]);
  const month = Number(match[2]);
  const day = Number(match[3]);
  const leapYear = year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0);
  const daysByMonth = [31, leapYear ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
  return month >= 1 && month <= 12 && day >= 1 && day <= (daysByMonth[month - 1] ?? 0);
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

    let validPaths = true;
    if (value.status === 'shipped' && Array.isArray(value.automatedEvidence) && value.automatedEvidence.length === 0) {
      errors.push(`shipped ${subject} must name automatedEvidence`);
      validPaths = false;
    }

    if (
      typeof value.status === 'string' &&
      ['shipped', 'partial', 'scaffold'].includes(value.status) &&
      Array.isArray(value.implementationPaths) &&
      value.implementationPaths.length === 0
    ) {
      errors.push(`${subject} must name implementationPaths`);
      validPaths = false;
    }

    if (value.status === 'shipped' && Array.isArray(value.automatedEvidence)) {
      for (const pathValue of value.automatedEvidence) {
        if (typeof pathValue !== 'string') continue;
        const reason = localPathError(root, root, pathValue, 'file');
        if (reason !== null) {
          errors.push(`${subject} automatedEvidence path '${pathValue}' ${reason}`);
          validPaths = false;
        }
      }
    }

    if (
      typeof value.status === 'string' &&
      ['shipped', 'partial', 'scaffold'].includes(value.status) &&
      Array.isArray(value.implementationPaths)
    ) {
      for (const pathValue of value.implementationPaths) {
        if (typeof pathValue !== 'string') continue;
        const reason = localPathError(root, root, pathValue, 'fileOrDirectory');
        if (reason !== null) {
          errors.push(`${subject} implementationPaths path '${pathValue}' ${reason}`);
          validPaths = false;
        }
      }
    }

    if (Array.isArray(value.sourceDocuments)) {
      for (const pathValue of value.sourceDocuments) {
        if (typeof pathValue !== 'string') continue;
        if (pathValue.trim() !== '' && isHttpUrl(pathValue)) continue;
        const reason = localPathError(root, root, pathValue, 'file');
        if (reason !== null) {
          errors.push(`${subject} sourceDocuments path '${pathValue}' ${reason}`);
          validPaths = false;
        }
      }
    }

    if (!exactKeys || !validTypes || !validPaths) return;

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

    if (
      typeof metadata.hygiene === 'string' &&
      metadata.hygiene.trim() !== '' &&
      !RESEARCH_HYGIENE.includes(metadata.hygiene as (typeof RESEARCH_HYGIENE)[number])
    ) {
      errors.push(`${path} has unknown research hygiene '${metadata.hygiene}'`);
    }

    for (const key of ['family', 'sourceDate', 'hygiene', 'refreshTrigger'] as const) {
      if (!(key in metadata)) continue;
      if (typeof metadata[key] !== 'string') {
        errors.push(`${path} key ${key} must be a string`);
      } else if (metadata[key].trim() === '') {
        errors.push(`${path} key ${key} must be non-empty`);
      }
    }

    if (typeof metadata.sourceDate === 'string' && !isRealCalendarDate(metadata.sourceDate)) {
      errors.push(`${path} key sourceDate must be a real YYYY-MM-DD calendar date`);
    }

    if ('sources' in metadata) {
      if (!Array.isArray(metadata.sources) || !metadata.sources.every((source) => typeof source === 'string')) {
        errors.push(`${path} key sources must be an array of strings`);
      } else if (metadata.sources.length === 0) {
        errors.push(`${path} key sources must be a non-empty array of strings`);
      } else {
        const noteDirectory = dirname(join(root, path));
        for (const source of metadata.sources) {
          if (source.trim() !== '' && isHttpUrl(source)) continue;
          const reason = localPathError(root, noteDirectory, source, 'file');
          if (reason !== null) errors.push(`${path} sources path '${source}' ${reason}`);
        }
      }
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

function checkNavigation(root: string, errors: string[]): void {
  for (const path of REQUIRED_NAVIGATION_FILES) {
    try {
      const stats = statSync(join(root, path));
      if (!stats.isFile() || stats.size === 0) {
        errors.push(`${path} must be a non-empty regular file`);
      }
    } catch {
      errors.push(`${path} must be a non-empty regular file`);
    }
  }

  for (const path of DOMAIN_MAP_FILES) {
    let markdown: string;
    try {
      markdown = readFileSync(join(root, path), 'utf8');
    } catch {
      continue;
    }

    const mappedPaths = [...markdown.matchAll(/`(src(?:-tauri)?\/[^`\s]+)`/g)]
      .map((match) => match[1])
      .filter((value): value is string => value !== undefined);
    if (mappedPaths.length === 0) {
      errors.push(`${path} must contain repository-root src/ or src-tauri/ path literals`);
      continue;
    }

    for (const mappedPath of new Set(mappedPaths)) {
      const reason = localPathError(root, root, mappedPath, 'fileOrDirectory');
      if (reason !== null) errors.push(`${path} mapped path '${mappedPath}' ${reason}`);
    }
  }

  let agents: string;
  try {
    agents = readFileSync(join(root, 'AGENTS.md'), 'utf8');
  } catch {
    errors.push('AGENTS.md could not be read');
    return;
  }

  const lineCount = agents.trimEnd().split(/\r?\n/).length;
  if (lineCount > AGENTS_LINE_HARD_CAP) {
    errors.push(`AGENTS.md exceeds the ${AGENTS_LINE_HARD_CAP}-line hard cap (${lineCount} lines)`);
  }

  const hasSliceChronology = /\bSlices?\s+(?:[A-Z]\b|D\d)/i.test(agents);
  const hasDatedChronology = /^#{1,6}\s+\d{4}-\d{2}-\d{2}\b/m.test(agents);
  if (hasSliceChronology || hasDatedChronology) {
    errors.push(
      'AGENTS.md contains chronological slice history; move it to docs/history/slice-ledger.md',
    );
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
  checkNavigation(options.root, errors);
  checkFreshness(records, options.git, warnings);

  return { errors, warnings };
}
