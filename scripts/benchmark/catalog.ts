// D131: model catalog + benchmark presets — select-and-run instead of
// handwritten JSON. `benchmarks/catalog/models.json` names reusable
// model configurations (folder under the standard model roots, PINNED
// artifact identity, quantization, engine); `benchmarks/catalog/
// presets.json` names runnable matrices over them (measurement paths,
// generation posture, suites × populations × repetitions).
//
// Two-phase honesty:
//   * LOAD validates structure with producer strictness — unknown
//     fields, duplicate ids, dangling model references, out-of-range
//     repetitions, or a plumeOrchestration preset with client-side
//     sampling all refuse. Loading needs no model on disk.
//   * EXPANSION binds a preset to the actual machine and re-verifies
//     the catalog's claims against reality: the checkpoint's live
//     digest must equal the catalog pin, the checkpoint's own
//     config.json quantization must match, and plume-path presets
//     take the output cap from the verified sidecar handshake. Any
//     drift refuses — a catalog entry never overrides what is
//     actually on disk.
//
// Everything downstream (identity verification per launch, provenance
// pinning per attempt, pairing validity) is the unchanged D129
// machinery; expansion only PRODUCES the same HarnessConfig shape the
// smoke CLIs build by hand.

import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';

import { digestModelDir, plumeIdentity, verifySidecarIdentity } from './model-identity.ts';
import type { HarnessConfig } from './runtime-factory.ts';
import type { SamplingBlock } from './types.ts';
import { readQuantization, REPO_ROOT } from './smoke-support.ts';

export const CATALOG_DIR = path.join(REPO_ROOT, 'benchmarks', 'catalog');
const FIXTURES_DIR = path.join(REPO_ROOT, 'benchmarks', 'fixtures');

const MEASUREMENT_PATHS = ['rawRuntime', 'plumeOrchestration'] as const;
type MeasurementPath = (typeof MEASUREMENT_PATHS)[number];

export interface CatalogModel {
  id: string;
  displayName: string;
  folder: string;
  engine: 'mlx-lm';
  maxContextTokens: number;
  artifact: {
    format: string;
    sha256: string;
    quantizationMethod: string | null;
    quantizationBits: number | null;
    quantizationGroupSize: number | null;
  };
}

export interface PresetSuite {
  fixture: string;
  populations: Array<'warm' | 'cold'>;
  repetitions: number;
  contextTokens?: number;
  measurementPaths?: MeasurementPath[];
}

export interface Preset {
  id: string;
  description: string;
  model: string;
  measurementPaths: MeasurementPath[];
  generation: 'plumePosture' | SamplingBlock;
  contextTokens: number;
  suites: PresetSuite[];
}

export interface Catalog {
  models: Map<string, CatalogModel>;
  presets: Map<string, Preset>;
}

function refuse(file: string, message: string): never {
  throw new Error(`${file}: ${message}`);
}

function requireExactFields(file: string, label: string, value: Record<string, unknown>, fields: string[]): void {
  for (const key of Object.keys(value)) {
    if (!fields.includes(key)) refuse(file, `${label}: unknown field "${key}" — the catalog schema is closed`);
  }
  for (const key of fields) {
    if (!(key in value)) refuse(file, `${label}: missing required field "${key}"`);
  }
}

const ID_RE = /^[a-z0-9][a-z0-9.-]{0,63}$/;

/// Suites whose oracles need capabilities NO current measurement path
/// has: the raw adapter returns no tool calls and sends only the
/// fixture prompt (no file bytes), and plume_bench has no tool loop.
/// Scheduling them would record missing harness capability as model
/// failure, so the loader refuses them outright. Remove a suite from
/// this set only when a measurement path can genuinely exercise it.
const TOOL_REQUIRING_SUITES = new Set(['single-file-bug-fix', 'multi-file-navigation', 'tool-calling-agent-loop']);

function loadModels(file: string): Map<string, CatalogModel> {
  const parsed = JSON.parse(readFileSync(file, 'utf8')) as Record<string, unknown>;
  requireExactFields(file, 'catalog', parsed, ['schemaVersion', 'models']);
  if (parsed['schemaVersion'] !== 1) refuse(file, `unsupported schemaVersion ${JSON.stringify(parsed['schemaVersion'])}`);
  if (!Array.isArray(parsed['models']) || parsed['models'].length === 0) refuse(file, 'models must be a non-empty array');
  const models = new Map<string, CatalogModel>();
  for (const entry of parsed['models'] as Array<Record<string, unknown>>) {
    requireExactFields(file, 'model entry', entry, [
      'id', 'displayName', 'folder', 'engine', 'maxContextTokens', 'artifact',
    ]);
    const id = entry['id'];
    if (typeof id !== 'string' || !ID_RE.test(id)) refuse(file, `model id ${JSON.stringify(id)} must match ${ID_RE}`);
    if (models.has(id)) refuse(file, `duplicate model id "${id}"`);
    if (typeof entry['displayName'] !== 'string' || entry['displayName'].length === 0) refuse(file, `${id}: displayName required`);
    if (typeof entry['folder'] !== 'string' || entry['folder'].length === 0 || entry['folder'].includes('/') || entry['folder'].includes('..')) {
      refuse(file, `${id}: folder must be a bare directory name under a model root`);
    }
    if (entry['engine'] !== 'mlx-lm') refuse(file, `${id}: engine must be "mlx-lm" (the only adapter with verified identity)`);
    if (!Number.isInteger(entry['maxContextTokens']) || (entry['maxContextTokens'] as number) <= 0) {
      refuse(file, `${id}: maxContextTokens must be a positive integer`);
    }
    const artifact = entry['artifact'];
    if (typeof artifact !== 'object' || artifact === null) refuse(file, `${id}: artifact must be an object`);
    const art = artifact as Record<string, unknown>;
    requireExactFields(file, `${id}.artifact`, art, [
      'format', 'sha256', 'quantizationMethod', 'quantizationBits', 'quantizationGroupSize',
    ]);
    if (typeof art['sha256'] !== 'string' || !/^sha256:[0-9a-f]{64}$/.test(art['sha256'])) {
      refuse(file, `${id}: artifact.sha256 must be a pinned "sha256:<64 hex>" digest`);
    }
    if (typeof art['format'] !== 'string' || art['format'].length === 0) {
      refuse(file, `${id}: artifact.format must be a non-empty string`);
    }
    if (art['quantizationMethod'] !== null && (typeof art['quantizationMethod'] !== 'string' || art['quantizationMethod'].length === 0)) {
      refuse(file, `${id}: artifact.quantizationMethod must be a non-empty string or null`);
    }
    for (const field of ['quantizationBits', 'quantizationGroupSize'] as const) {
      const value = art[field];
      if (value !== null && (!Number.isInteger(value) || (value as number) <= 0)) {
        refuse(file, `${id}: artifact.${field} must be a positive integer or null`);
      }
    }
    models.set(id, entry as unknown as CatalogModel);
  }
  return models;
}

function validateSampling(file: string, presetId: string, sampling: Record<string, unknown>): SamplingBlock {
  requireExactFields(file, `${presetId}.generation`, sampling, [
    'temperature', 'topP', 'topK', 'minP', 'repeatPenalty', 'seed', 'maxOutputTokens', 'stopSequences',
  ]);
  for (const field of ['temperature', 'topP', 'minP', 'repeatPenalty'] as const) {
    const value = sampling[field];
    if (value !== null && (typeof value !== 'number' || !Number.isFinite(value) || value < 0)) {
      refuse(file, `${presetId}: generation.${field} must be a finite non-negative number or null`);
    }
  }
  for (const field of ['topK', 'seed'] as const) {
    const value = sampling[field];
    if (value !== null && (!Number.isInteger(value) || (value as number) < 0)) {
      refuse(file, `${presetId}: generation.${field} must be a non-negative integer or null`);
    }
  }
  if (!Number.isInteger(sampling['maxOutputTokens']) || (sampling['maxOutputTokens'] as number) <= 0) {
    refuse(file, `${presetId}: generation.maxOutputTokens must be a positive integer`);
  }
  const stops = sampling['stopSequences'];
  if (
    !Array.isArray(stops) ||
    stops.length > 16 ||
    !stops.every((s) => typeof s === 'string' && s.length > 0 && s.length <= 256)
  ) {
    refuse(file, `${presetId}: generation.stopSequences must be at most 16 non-empty strings of at most 256 chars`);
  }
  return sampling as unknown as SamplingBlock;
}

function loadPresets(file: string, models: Map<string, CatalogModel>): Map<string, Preset> {
  const parsed = JSON.parse(readFileSync(file, 'utf8')) as Record<string, unknown>;
  requireExactFields(file, 'presets', parsed, ['schemaVersion', 'presets']);
  if (parsed['schemaVersion'] !== 1) refuse(file, `unsupported schemaVersion ${JSON.stringify(parsed['schemaVersion'])}`);
  if (!Array.isArray(parsed['presets']) || parsed['presets'].length === 0) refuse(file, 'presets must be a non-empty array');
  const presets = new Map<string, Preset>();
  for (const entry of parsed['presets'] as Array<Record<string, unknown>>) {
    requireExactFields(file, 'preset entry', entry, [
      'id', 'description', 'model', 'measurementPaths', 'generation', 'contextTokens', 'suites',
    ]);
    const id = entry['id'];
    if (typeof id !== 'string' || !ID_RE.test(id)) refuse(file, `preset id ${JSON.stringify(id)} must match ${ID_RE}`);
    if (presets.has(id)) refuse(file, `duplicate preset id "${id}"`);
    if (typeof entry['description'] !== 'string' || entry['description'].length === 0) refuse(file, `${id}: description required`);
    const model = entry['model'];
    if (typeof model !== 'string' || !models.has(model)) {
      refuse(file, `${id}: model ${JSON.stringify(model)} is not in the catalog`);
    }
    const paths = entry['measurementPaths'];
    if (
      !Array.isArray(paths) ||
      paths.length === 0 ||
      !paths.every((p) => (MEASUREMENT_PATHS as readonly string[]).includes(p as string)) ||
      new Set(paths).size !== paths.length
    ) {
      refuse(file, `${id}: measurementPaths must be a non-empty duplicate-free subset of ${JSON.stringify(MEASUREMENT_PATHS)}`);
    }
    const includesPlume = (paths as string[]).includes('plumeOrchestration');
    const generation = entry['generation'];
    if (generation !== 'plumePosture') {
      if (includesPlume) {
        refuse(
          file,
          `${id}: a preset measuring plumeOrchestration must use generation "plumePosture" — ` +
            'Plume sends no client sampling controls, so any explicit sampling would be a lie on the wire',
        );
      }
      if (typeof generation !== 'object' || generation === null) {
        refuse(file, `${id}: generation must be "plumePosture" or an explicit sampling object`);
      }
      validateSampling(file, id, generation as Record<string, unknown>);
    }
    if (!Number.isInteger(entry['contextTokens']) || (entry['contextTokens'] as number) <= 0) {
      refuse(file, `${id}: contextTokens must be a positive integer`);
    }
    const suites = entry['suites'];
    if (!Array.isArray(suites) || suites.length === 0) refuse(file, `${id}: suites must be a non-empty array`);
    for (const suite of suites as Array<Record<string, unknown>>) {
      const fields = ['fixture', 'populations', 'repetitions'];
      if ('contextTokens' in suite) fields.push('contextTokens');
      if ('measurementPaths' in suite) fields.push('measurementPaths');
      requireExactFields(file, `${id} suite`, suite, fields);
      const fixture = suite['fixture'];
      if (typeof fixture !== 'string' || !existsSync(path.join(FIXTURES_DIR, fixture, 'manifest.json'))) {
        refuse(file, `${id}: fixture ${JSON.stringify(fixture)} does not exist under benchmarks/fixtures`);
      }
      const suiteId = fixture.split('/')[0] ?? '';
      if (TOOL_REQUIRING_SUITES.has(suiteId)) {
        refuse(
          file,
          `${id}: suite "${suiteId}" cannot be honestly measured by any current path — the raw adapter ` +
            'has no file/tool executor and plume_bench has no tool loop; scheduling it would record ' +
            'missing harness capability as model failure',
        );
      }
      const populations = suite['populations'];
      if (
        !Array.isArray(populations) ||
        populations.length === 0 ||
        !populations.every((p) => p === 'warm' || p === 'cold') ||
        new Set(populations).size !== populations.length
      ) {
        refuse(file, `${id}/${String(fixture)}: populations must be a non-empty duplicate-free subset of ["warm","cold"]`);
      }
      if ('contextTokens' in suite && (!Number.isInteger(suite['contextTokens']) || (suite['contextTokens'] as number) <= 0)) {
        refuse(file, `${id}/${String(fixture)}: contextTokens must be a positive integer`);
      }
      if (!Number.isInteger(suite['repetitions']) || (suite['repetitions'] as number) < 3 || (suite['repetitions'] as number) > 30) {
        refuse(file, `${id}/${String(fixture)}: repetitions must be 3..30 (incomplete evidence below three)`);
      }
      const suitePaths = suite['measurementPaths'];
      if (suitePaths !== undefined) {
        if (
          !Array.isArray(suitePaths) ||
          suitePaths.length === 0 ||
          !suitePaths.every((p) => (paths as string[]).includes(p as string)) ||
          new Set(suitePaths).size !== suitePaths.length
        ) {
          refuse(file, `${id}/${String(fixture)}: suite measurementPaths must be a non-empty duplicate-free subset of the preset's`);
        }
      }
    }
    presets.set(id, entry as unknown as Preset);
  }
  return presets;
}

/// Load and structurally validate the whole catalog. Needs no model
/// on disk and no sidecar — expansion does the machine binding.
export function loadCatalog(dir: string = CATALOG_DIR): Catalog {
  const models = loadModels(path.join(dir, 'models.json'));
  const presets = loadPresets(path.join(dir, 'presets.json'), models);
  return { models, presets };
}

// ---- expansion ----------------------------------------------------------

export interface ExpansionDeps {
  /// Interpreter with an importable mlx_lm (resolved by the caller).
  python: string;
  /// Resolves a catalog folder name to an absolute checkpoint dir.
  resolveFolder: (folder: string) => string;
  /// Built plume_bench, required when the preset touches
  /// plumeOrchestration; its verified handshake supplies the cap.
  sidecar?: string;
}

export interface MatrixRun {
  label: string;
  config: HarnessConfig;
  groupId: string;
  fixtureDir: string;
  population: 'warm' | 'cold';
  repetitions: number;
  /// Shared across paths for the same (suite, population, repetition)
  /// when the suite runs on both paths; null when unpaired.
  pairIdFor: (repetition: number) => string | null;
}

/// Bind a preset to this machine: verify the catalog's pinned claims
/// against the live checkpoint (digest, quantization) and the sidecar
/// handshake, then emit the concrete run matrix. Any drift refuses.
/// The distinct configs a matrix actually runs, with the groups each
/// one serves — persisted as evidence so no per-suite override (e.g.
/// a long-context window) is silently collapsed into a neighbor's
/// config file.
export interface PersistedConfig {
  measurementPath: string;
  contextTokens: number;
  groupIds: string[];
  config: HarnessConfig;
}

export function distinctConfigs(runs: MatrixRun[]): PersistedConfig[] {
  const byShape = new Map<string, PersistedConfig>();
  for (const run of runs) {
    const key = JSON.stringify(run.config);
    const existing = byShape.get(key);
    if (existing !== undefined) {
      existing.groupIds.push(run.groupId);
      continue;
    }
    byShape.set(key, {
      measurementPath: run.config.measurementPath,
      contextTokens: run.config.model.context.configuredTokens,
      groupIds: [run.groupId],
      config: run.config,
    });
  }
  return [...byShape.values()];
}

export function expandPreset(catalog: Catalog, presetId: string, deps: ExpansionDeps): MatrixRun[] {
  const preset = catalog.presets.get(presetId);
  if (preset === undefined) {
    const known = [...catalog.presets.keys()].join(', ');
    throw new Error(`unknown preset "${presetId}" (catalog has: ${known})`);
  }
  const model = catalog.models.get(preset.model);
  if (model === undefined) throw new Error(`preset ${presetId}: model ${preset.model} vanished from the catalog`);

  const modelDir = deps.resolveFolder(model.folder);
  // Catalog pin vs disk, verified at expansion (and again per launch
  // by the factory): the folder must hold EXACTLY the artifact the
  // catalog names.
  const liveDigest = digestModelDir(modelDir);
  if (liveDigest !== model.artifact.sha256) {
    throw new Error(
      `catalog pin mismatch for ${model.id}: catalog pins ${model.artifact.sha256} but ${modelDir} ` +
        `hashes to ${liveDigest} — the checkpoint on disk is not the cataloged artifact`,
    );
  }
  const quant = readQuantization(modelDir);
  if (
    quant.method !== model.artifact.quantizationMethod ||
    quant.bits !== model.artifact.quantizationBits ||
    quant.groupSize !== model.artifact.quantizationGroupSize
  ) {
    throw new Error(
      `catalog quantization mismatch for ${model.id}: catalog says method=${model.artifact.quantizationMethod} ` +
        `bits=${model.artifact.quantizationBits} group=${model.artifact.quantizationGroupSize} but the ` +
        `checkpoint's config.json says method=${quant.method} bits=${quant.bits} group=${quant.groupSize}`,
    );
  }

  const needsPlume = preset.suites.some((s) => (s.measurementPaths ?? preset.measurementPaths).includes('plumeOrchestration'));
  let sampling: SamplingBlock;
  if (preset.generation === 'plumePosture') {
    const sidecar = deps.sidecar;
    if (sidecar === undefined) {
      throw new Error(
        `preset ${presetId} uses generation "plumePosture" — the plume_bench sidecar is required to read ` +
          'the product output cap (build it: ./scripts/dev-env.sh cargo build --manifest-path src-tauri/Cargo.toml --bin plume_bench)',
      );
    }
    const cap = verifySidecarIdentity(sidecar, plumeIdentity()).maxOutputTokens;
    sampling = {
      temperature: null,
      topP: null,
      topK: null,
      minP: null,
      repeatPenalty: null,
      seed: null,
      maxOutputTokens: cap,
      stopSequences: [],
    };
  } else {
    sampling = preset.generation;
  }
  if (needsPlume && deps.sidecar === undefined) {
    throw new Error(`preset ${presetId} measures plumeOrchestration and needs the plume_bench sidecar`);
  }

  const buildConfig = (measurementPath: MeasurementPath, contextTokens: number): HarnessConfig => {
    if (contextTokens > model.maxContextTokens) {
      throw new Error(
        `preset ${presetId}: contextTokens ${contextTokens} exceeds ${model.id}'s maxContextTokens ${model.maxContextTokens}`,
      );
    }
    if (sampling.maxOutputTokens >= contextTokens) {
      throw new Error(
        `preset ${presetId}: contextTokens ${contextTokens} cannot fit the output reserve ${sampling.maxOutputTokens}`,
      );
    }
    return {
      measurementPath,
      // The sidecar rides on every config when the preset touches the
      // plume path, so agent-suite diff mechanics go through Plume's
      // real Rust patch validator on BOTH paths.
      ...(deps.sidecar !== undefined && needsPlume ? { plumeBench: { binary: deps.sidecar } } : {}),
      runtime: {
        path: 'plume-mlx-lm',
        name: 'mlx-lm',
        version: null,
        engine: 'mlx-lm',
        backend: 'MLX',
        transport: 'openai-sse',
        server: {
          command: [deps.python, '-m', 'mlx_lm', 'server', '--model', modelDir],
          modelDir,
          startupTimeoutMs: 120_000,
        },
        configuration: {
          digest: null,
          mtp: null,
          speculativeDecoding: null,
          promptCache: null,
          kvCacheQuantization: null,
          contextTokens: null,
          batchSize: null,
          threads: null,
          gpuLayers: null,
        },
      },
      model: {
        sourceId: `local/${model.folder}`,
        sourceRevision: model.artifact.sha256.slice(0, 71),
        artifact: {
          format: model.artifact.format,
          sha256: model.artifact.sha256,
          quantizationMethod: model.artifact.quantizationMethod,
          quantizationBits: model.artifact.quantizationBits,
          quantizationGroupSize: model.artifact.quantizationGroupSize,
          conversionProvenance: null,
          conversionConfigDigest: null,
        },
        comparisonParity: 'strictArtifact',
        context: {
          pointTokens: contextTokens,
          configuredTokens: contextTokens,
          acceptedTokens: null,
          maxOutputTokens: sampling.maxOutputTokens,
        },
        sampling,
      },
    };
  };

  const runs: MatrixRun[] = [];
  for (const suite of preset.suites) {
    const suitePaths = suite.measurementPaths ?? preset.measurementPaths;
    const paired = suitePaths.length === 2;
    const caseId = suite.fixture.replace('/', '_').replace(/[^A-Za-z0-9_]/g, '_');
    for (const measurementPath of suitePaths) {
      const contextTokens = suite.contextTokens ?? preset.contextTokens;
      const config = buildConfig(measurementPath, contextTokens);
      const pathTag = measurementPath === 'rawRuntime' ? 'raw' : 'plume';
      for (const population of suite.populations) {
        runs.push({
          label: `${measurementPath} ${suite.fixture} ${population}`,
          config,
          groupId: `grp_${presetId.replace(/[^A-Za-z0-9_]/g, '_')}_${caseId}_${pathTag}_${population}`,
          fixtureDir: path.join(FIXTURES_DIR, suite.fixture),
          population,
          repetitions: suite.repetitions,
          pairIdFor: paired
            ? (repetition: number): string => `pair_${caseId}_${population}_${repetition}`
            : (): null => null,
        });
      }
    }
  }
  return runs;
}
