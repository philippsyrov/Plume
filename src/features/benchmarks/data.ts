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
  artifacts: { kind: 'absent' } | { kind: 'loaded'; files: ResultFile[] };
  catalog: CatalogState;
}

function isNotFound(err: unknown): err is IpcError {
  return isIpcError(err) && err.kind === 'NotFound';
}

function describeError(err: unknown): string {
  if (isIpcError(err)) return ipcErrorMessage(err);
  if (err instanceof Error) return err.message;
  return 'Unknown error.';
}

/// Walk benchmark-artifacts collecting *.jsonl repo-relative paths.
/// Depth-bounded; symlinks are skipped (fs.read would refuse anything
/// escaping the root anyway, and a result tree has no reason to link).
async function collectJsonlFiles(dir: string, depth: number): Promise<string[]> {
  const entries: FileEntry[] = await listDir(dir);
  const files: string[] = [];
  for (const entry of entries) {
    const childPath = `${dir}/${entry.name}`;
    if (entry.kind === 'file' && entry.name.endsWith('.jsonl')) {
      files.push(childPath);
    } else if (entry.kind === 'dir' && depth < MAX_WALK_DEPTH) {
      files.push(...(await collectJsonlFiles(childPath, depth + 1)));
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
  for (const suite of suites) {
    if (suite.kind !== 'dir') continue;
    const cases = await listDir(`${FIXTURES_DIR}/${suite.name}`);
    for (const caseEntry of cases) {
      if (caseEntry.kind !== 'dir') continue;
      const caseFiles = await listDir(`${FIXTURES_DIR}/${suite.name}/${caseEntry.name}`);
      if (caseFiles.some((f) => f.kind === 'file' && f.name === 'manifest.json')) {
        fixtures.add(`${suite.name}/${caseEntry.name}`);
      }
    }
  }
  return fixtures;
}

async function loadCatalogState(): Promise<CatalogState> {
  let modelsText: string;
  let presetsText: string;
  try {
    const models = await readFile(`${CATALOG_DIR}/models.json`);
    const presets = await readFile(`${CATALOG_DIR}/presets.json`);
    if (models.encoding !== 'utf-8' || presets.encoding !== 'utf-8') {
      return { kind: 'error', message: 'Catalog files are not UTF-8 text.' };
    }
    modelsText = models.content;
    presetsText = presets.content;
  } catch (err) {
    if (isNotFound(err)) return { kind: 'absent' };
    return { kind: 'error', message: describeError(err) };
  }
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
    jsonlPaths = await collectJsonlFiles(ARTIFACTS_DIR, 1);
  } catch (err) {
    if (isNotFound(err)) {
      return { artifacts: { kind: 'absent' }, catalog };
    }
    throw err;
  }
  const files: ResultFile[] = [];
  for (const path of jsonlPaths.sort()) {
    files.push(await loadResultFile(path));
  }
  return { artifacts: { kind: 'loaded', files }, catalog };
}
