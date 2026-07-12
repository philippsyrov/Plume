// D132: the pure LOAD phase of the D131 catalog, split out of
// catalog.ts so the in-app benchmark results viewer runs the SAME
// producer-strict validation the CLI does — one loader, two callers,
// no display-grade copy that could drift. Everything here is
// browser-safe: no node imports; the one disk question the load
// phase asks (does this fixture exist?) is injected by the caller.
// Node callers stat benchmarks/fixtures directly; the viewer walks
// the same tree through the trust-gated fs API first.
//
// The EXPANSION phase (machine binding: live digest vs pin,
// quantization cross-check, sidecar handshake) stays in catalog.ts —
// it genuinely needs the node side and never runs in the app.

import type { SamplingBlock } from './types.ts';

export const MEASUREMENT_PATHS = ['rawRuntime', 'plumeOrchestration'] as const;
export type MeasurementPath = (typeof MEASUREMENT_PATHS)[number];

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

export function parseModels(text: string, file: string): Map<string, CatalogModel> {
  const parsed = JSON.parse(text) as Record<string, unknown>;
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

export function parsePresets(
  text: string,
  file: string,
  models: Map<string, CatalogModel>,
  fixtureExists: (fixture: string) => boolean,
): Map<string, Preset> {
  const parsed = JSON.parse(text) as Record<string, unknown>;
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
      if (typeof fixture !== 'string' || !fixtureExists(fixture)) {
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

/// Parse and structurally validate a whole catalog from file text.
/// Needs no model on disk and no sidecar — expansion (catalog.ts)
/// does the machine binding.
export function parseCatalog(
  modelsText: string,
  presetsText: string,
  modelsLabel: string,
  presetsLabel: string,
  fixtureExists: (fixture: string) => boolean,
): Catalog {
  const models = parseModels(modelsText, modelsLabel);
  const presets = parsePresets(presetsText, presetsLabel, models, fixtureExists);
  return { models, presets };
}
