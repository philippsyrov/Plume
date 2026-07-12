// D132: benchmark results viewer — data layer. Reads the D129 result
// artifacts (`benchmark-artifacts/**/*.jsonl`) and the D131 catalog
// (`benchmarks/catalog/`) of the CURRENTLY OPEN TRUSTED PROJECT
// through the existing display-read fs verbs. No new IPC surface: the
// trust gate, path-safety resolution, and the 2 MB display-read cap
// all apply unchanged.
//
// Honesty rules carried over from the harness:
//   * Records are READER-validated by the same `readRecords` the CLI
//     summarizer uses — one validator, not a display-grade copy.
//     Invalid lines are excluded and their errors shown, never
//     silently dropped.
//   * The catalog is parsed by the same producer-strict loader the
//     preset CLI uses (`catalog-schema.ts`); a malformed catalog is a
//     visible refusal, not a best-effort render.
//   * Fixture existence for catalog validation is ground truth read
//     through the same trust-gated fs, not assumed.

import { listDir, readFile, type FileEntry } from '../../lib/api/fs';
import { ipcErrorMessage, isIpcError, type IpcError } from '../../lib/api/errors';
import { parseCatalog, type Catalog } from '../../../scripts/benchmark/catalog-schema.ts';
import {
  readRecords,
  summarizeGroups,
  summarizePairs,
  type GroupSummary,
  type PairSummary,
} from '../../../scripts/benchmark/summarize-lib.ts';
import type { BenchmarkRecord } from '../../../scripts/benchmark/types.ts';

export const ARTIFACTS_DIR = 'benchmark-artifacts';
export const CATALOG_DIR = 'benchmarks/catalog';
const FIXTURES_DIR = 'benchmarks/fixtures';
/// Deep enough for benchmark-artifacts/presets/<id>/records.jsonl;
/// a runaway artifact tree stops here instead of hammering fs.list.
const MAX_WALK_DEPTH = 3;
/// Breadth budgets. The walks are also breadth-BOUNDED so a
/// pathological project cannot make opening the panel issue an
/// unbounded number of fs.list/fs.read IPC calls or render an
/// unbounded result set. Exceeding a budget is a VISIBLE refusal of
/// the whole walk, never a silent arbitrary prefix — a partial
/// evidence view would make missing runs look like absent runs.
/// Today's largest real tree is 3 run dirs / 3 record files and 7
/// suites / ~15 fixture cases, so these budgets are an order of
/// magnitude of headroom, not a near-term ceiling.
export const MAX_WALK_DIRS = 64;
export const MAX_RESULT_FILES = 64;
export const MAX_FIXTURE_DIRS = 128;
/// Engine name the scripted fake runtime stamps into its records —
/// the summarizer banners these as harness test data, and so do we.
export const FAKE_ENGINE = 'plume-fake-runtime';

export interface ResultFile {
  /// Repo-relative path, e.g. `benchmark-artifacts/presets/x/records.jsonl`.
  path: string;
  /// The run directory under benchmark-artifacts, e.g. `presets/x`.
  runLabel: string;
  /// null when the file could not be read or its schema is unreadable;
  /// the message says why. The file still renders, as a refusal.
  readError: string | null;
  records: BenchmarkRecord[];
  lineErrors: string[];
  warnings: string[];
  groups: GroupSummary[];
  pairs: PairSummary[];
  hasFakeRuntime: boolean;
}

export type CatalogState =
  | { kind: 'absent' }
  | { kind: 'error'; message: string }
  | { kind: 'loaded'; catalog: Catalog };

export interface BenchmarkEvidence {
  /// absent = the project has no benchmark-artifacts directory.
  /// refused = the tree exceeded a walk budget; nothing is shown
  /// rather than an arbitrary subset, and the message says which
  /// budget broke.
  artifacts:
    | { kind: 'absent' }
    | { kind: 'refused'; message: string }
    | { kind: 'loaded'; files: ResultFile[] };
  catalog: CatalogState;
}

/// Thrown when a walk exceeds its breadth budget; callers turn it
/// into the visible refused/error state for their section.
class WalkBudgetExceeded extends Error {}

function isNotFound(err: unknown): err is IpcError {
  return isIpcError(err) && err.kind === 'NotFound';
}

function describeError(err: unknown): string {
  if (isIpcError(err)) return ipcErrorMessage(err);
  if (err instanceof Error) return err.message;
  return 'Unknown error.';
}

/// Walk benchmark-artifacts collecting *.jsonl repo-relative paths.
/// Depth- and breadth-bounded; symlinks are skipped (fs.read would
/// refuse anything escaping the root anyway, and a result tree has no
/// reason to link). `budget` counts directories listed and files
/// collected across the WHOLE walk; exceeding either refuses the walk.
async function collectJsonlFiles(
  dir: string,
  depth: number,
  budget: { dirs: number; files: number },
): Promise<string[]> {
  budget.dirs += 1;
  if (budget.dirs > MAX_WALK_DIRS) {
    throw new WalkBudgetExceeded(
      `benchmark-artifacts holds more than ${MAX_WALK_DIRS} directories — ` +
        'refusing to render an arbitrary subset of the evidence',
    );
  }
  const entries: FileEntry[] = await listDir(dir);
  const files: string[] = [];
  for (const entry of entries) {
    const childPath = `${dir}/${entry.name}`;
    if (entry.kind === 'file' && entry.name.endsWith('.jsonl')) {
      budget.files += 1;
      if (budget.files > MAX_RESULT_FILES) {
        throw new WalkBudgetExceeded(
          `benchmark-artifacts holds more than ${MAX_RESULT_FILES} .jsonl record files — ` +
            'refusing to render an arbitrary subset of the evidence',
        );
      }
      files.push(childPath);
    } else if (entry.kind === 'dir' && depth < MAX_WALK_DEPTH) {
      files.push(...(await collectJsonlFiles(childPath, depth + 1, budget)));
    }
  }
  return files;
}

async function loadResultFile(path: string): Promise<ResultFile> {
  const runLabel = path
    .replace(`${ARTIFACTS_DIR}/`, '')
    .split('/')
    .slice(0, -1)
    .join('/');
  const base: ResultFile = {
    path,
    runLabel: runLabel.length > 0 ? runLabel : '(top level)',
    readError: null,
    records: [],
    lineErrors: [],
    warnings: [],
    groups: [],
    pairs: [],
    hasFakeRuntime: false,
  };
  let text: string;
  try {
    const content = await readFile(path);
    if (content.encoding !== 'utf-8') {
      return { ...base, readError: 'Not a UTF-8 text file.' };
    }
    text = content.content;
  } catch (err) {
    return { ...base, readError: describeError(err) };
  }
  try {
    const { records, lineErrors, warnings } = readRecords(text);
    return {
      ...base,
      records,
      lineErrors,
      warnings,
      groups: summarizeGroups(records),
      pairs: summarizePairs(records),
      hasFakeRuntime: records.some((r) => r.runtime.engine === FAKE_ENGINE),
    };
  } catch (err) {
    // readRecords throws on a newer schema version: the whole file is
    // refused because we cannot trust our reading of ANY of its lines.
    return { ...base, readError: describeError(err) };
  }
}

/// Ground-truth fixture set for catalog validation: every
/// `<suite>/<case>` under benchmarks/fixtures that has a
/// manifest.json. Missing tree → empty set → the catalog's fixture
/// claims refuse, which is the honest outcome.
async function collectFixtures(): Promise<Set<string>> {
  const fixtures = new Set<string>();
  let suites: FileEntry[];
  try {
    suites = await listDir(FIXTURES_DIR);
  } catch (err) {
    if (isNotFound(err)) return fixtures;
    throw err;
  }
  // Breadth budget across suite AND case directories: past it we can
  // no longer establish fixture ground truth with a bounded number of
  // IPC calls, so catalog validation refuses (visible error state)
  // instead of walking an arbitrary amount of the tree.
  let dirsListed = 1;
  const spend = (): void => {
    dirsListed += 1;
    if (dirsListed > MAX_FIXTURE_DIRS) {
      throw new WalkBudgetExceeded(
        `benchmarks/fixtures holds more than ${MAX_FIXTURE_DIRS} directories — ` +
          'refusing to verify catalog fixture claims against an arbitrary subset',
      );
    }
  };
  for (const suite of suites) {
    if (suite.kind !== 'dir') continue;
    spend();
    const cases = await listDir(`${FIXTURES_DIR}/${suite.name}`);
    for (const caseEntry of cases) {
      if (caseEntry.kind !== 'dir') continue;
      spend();
      const caseFiles = await listDir(`${FIXTURES_DIR}/${suite.name}/${caseEntry.name}`);
      if (caseFiles.some((f) => f.kind === 'file' && f.name === 'manifest.json')) {
        fixtures.add(`${suite.name}/${caseEntry.name}`);
      }
    }
  }
  return fixtures;
}

/// Read one catalog file, distinguishing "missing" from every other
/// failure so the caller can tell a wholly absent catalog from a
/// half-present one.
async function readCatalogFile(
  path: string,
): Promise<{ kind: 'missing' } | { kind: 'error'; message: string } | { kind: 'ok'; text: string }> {
  try {
    const content = await readFile(path);
    if (content.encoding !== 'utf-8') {
      return { kind: 'error', message: `${path} is not UTF-8 text.` };
    }
    return { kind: 'ok', text: content.content };
  } catch (err) {
    if (isNotFound(err)) return { kind: 'missing' };
    return { kind: 'error', message: describeError(err) };
  }
}

async function loadCatalogState(): Promise<CatalogState> {
  const models = await readCatalogFile(`${CATALOG_DIR}/models.json`);
  const presets = await readCatalogFile(`${CATALOG_DIR}/presets.json`);
  if (models.kind === 'error') return { kind: 'error', message: models.message };
  if (presets.kind === 'error') return { kind: 'error', message: presets.message };
  // Only BOTH files missing is an absent catalog. One present and one
  // missing is a malformed catalog and refuses visibly — labelling it
  // "absent" would hide a real defect in the project's evidence.
  if (models.kind === 'missing' && presets.kind === 'missing') return { kind: 'absent' };
  if (models.kind === 'missing' || presets.kind === 'missing') {
    const present = models.kind === 'missing' ? 'presets.json' : 'models.json';
    const missing = models.kind === 'missing' ? 'models.json' : 'presets.json';
    return {
      kind: 'error',
      message:
        `Catalog is incomplete: ${CATALOG_DIR}/${present} exists but ` +
        `${CATALOG_DIR}/${missing} is missing — a catalog needs both files.`,
    };
  }
  const modelsText = models.text;
  const presetsText = presets.text;
  try {
    const fixtures = await collectFixtures();
    const catalog = parseCatalog(
      modelsText,
      presetsText,
      `${CATALOG_DIR}/models.json`,
      `${CATALOG_DIR}/presets.json`,
      (fixture) => fixtures.has(fixture),
    );
    return { kind: 'loaded', catalog };
  } catch (err) {
    return { kind: 'error', message: describeError(err) };
  }
}

/// Load everything the viewer shows. Only an unexpected artifacts
/// walk failure rejects (surfaced by the hook); a missing artifacts
/// tree, a missing catalog, and per-file read/validation failures
/// are all REPRESENTED, not thrown — each renders as its own state.
export async function loadBenchmarkEvidence(): Promise<BenchmarkEvidence> {
  const catalog = await loadCatalogState();
  let jsonlPaths: string[];
  try {
    jsonlPaths = await collectJsonlFiles(ARTIFACTS_DIR, 1, { dirs: 0, files: 0 });
  } catch (err) {
    if (isNotFound(err)) {
      return { artifacts: { kind: 'absent' }, catalog };
    }
    if (err instanceof WalkBudgetExceeded) {
      return { artifacts: { kind: 'refused', message: err.message }, catalog };
    }
    throw err;
  }
  const files: ResultFile[] = [];
  for (const path of jsonlPaths.sort()) {
    files.push(await loadResultFile(path));
  }
  return { artifacts: { kind: 'loaded', files }, catalog };
}
