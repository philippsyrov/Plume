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
//     sampling all refuse. Loading needs no model on disk. D132 moved
//     the load phase into `catalog-schema.ts` (pure, browser-safe) so
//     the in-app results viewer runs the SAME loader; this module
//     re-exports it and adds the node-side file reading.
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

import { parseCatalog } from './catalog-schema.ts';
import type { Catalog, MeasurementPath } from './catalog-schema.ts';
import { digestModelDir, plumeIdentity, verifySidecarIdentity } from './model-identity.ts';
import type { HarnessConfig } from './runtime-factory.ts';
import type { SamplingBlock } from './types.ts';
import { readQuantization, REPO_ROOT } from './smoke-support.ts';

export type { Catalog, CatalogModel, MeasurementPath, Preset, PresetSuite } from './catalog-schema.ts';
export { parseCatalog } from './catalog-schema.ts';

export const CATALOG_DIR = path.join(REPO_ROOT, 'benchmarks', 'catalog');
const FIXTURES_DIR = path.join(REPO_ROOT, 'benchmarks', 'fixtures');

/// Load and structurally validate the whole catalog. Needs no model
/// on disk and no sidecar — expansion does the machine binding.
export function loadCatalog(dir: string = CATALOG_DIR): Catalog {
  const modelsFile = path.join(dir, 'models.json');
  const presetsFile = path.join(dir, 'presets.json');
  return parseCatalog(
    readFileSync(modelsFile, 'utf8'),
    readFileSync(presetsFile, 'utf8'),
    modelsFile,
    presetsFile,
    (fixture) => existsSync(path.join(FIXTURES_DIR, fixture, 'manifest.json')),
  );
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
