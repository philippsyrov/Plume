// D131: select-and-run (wrapped by scripts/benchmark-preset.sh).
// With no argument, lists the catalog's presets. With a preset id,
// binds it to this machine (catalog pins re-verified against the live
// checkpoint and the verified sidecar handshake), runs the matrix
// through the unchanged D129 machinery, and prints the summary
// tables. Records land in gitignored benchmark-artifacts/presets/
// and are never a performance claim by themselves.

import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';

import { distinctConfigs, expandPreset, loadCatalog } from './catalog.ts';
import { runMatrix } from './matrix.ts';
import { probeMlxLmVersion } from './model-identity.ts';
import { fail as failWith, resolveFolderDir, resolvePython, REPO_ROOT } from './smoke-support.ts';
import { readRecords, renderMarkdown } from './summarize-lib.ts';

const PREFIX = 'benchmark-preset';

function fail(message: string): never {
  failWith(PREFIX, message);
}

function resolveSidecar(): string | undefined {
  const candidate =
    process.env['PLUME_BENCH_BIN'] ?? path.join(REPO_ROOT, 'src-tauri', 'target', 'debug', 'plume_bench');
  return existsSync(candidate) ? candidate : undefined;
}

async function main(): Promise<void> {
  const catalog = loadCatalog();
  const presetId = process.argv[2];
  if (presetId === undefined || presetId.length === 0) {
    process.stdout.write('Available presets (scripts/benchmark-preset.sh <id>):\n\n');
    for (const preset of catalog.presets.values()) {
      const model = catalog.models.get(preset.model);
      process.stdout.write(`  ${preset.id}\n`);
      process.stdout.write(`      model: ${model?.displayName ?? preset.model}\n`);
      process.stdout.write(`      ${preset.description}\n\n`);
    }
    return;
  }

  const python = resolvePython(PREFIX);
  const version = probeMlxLmVersion(python);
  const sidecar = resolveSidecar();
  process.stderr.write(`interpreter: ${python} (mlx-lm ${version ?? 'unknown'})\n`);
  process.stderr.write(`sidecar:     ${sidecar ?? 'not built'}\n`);
  process.stderr.write('binding preset to this machine (catalog pins re-verified)…\n');
  const runs = expandPreset(catalog, presetId, {
    python,
    resolveFolder: (folder) => resolveFolderDir(PREFIX, folder),
    ...(sidecar !== undefined ? { sidecar } : {}),
  });

  const outDir = path.join(REPO_ROOT, 'benchmark-artifacts', 'presets', presetId);
  mkdirSync(outDir, { recursive: true });
  const outFile = path.join(outDir, 'records.jsonl');
  writeFileSync(outFile, ''); // fresh matrix per run
  // Evidence: every DISTINCT config the matrix runs, with the groups
  // it serves — per-suite overrides (e.g. a long-context window) are
  // never collapsed into a neighbor's file.
  writeFileSync(path.join(outDir, 'configs.json'), JSON.stringify(distinctConfigs(runs), null, 2));

  const written = await runMatrix(runs, outFile);
  const result = readRecords(readFileSync(outFile, 'utf8'));
  if (result.lineErrors.length > 0) {
    fail(`recorded lines failed reader validation:\n${result.lineErrors.join('\n')}`);
  }
  process.stderr.write(`recorded ${written} attempts → ${outFile}\n\n`);
  process.stdout.write(renderMarkdown(result.records));
}

main().catch((err: unknown) => {
  fail(err instanceof Error ? err.message : String(err));
});
