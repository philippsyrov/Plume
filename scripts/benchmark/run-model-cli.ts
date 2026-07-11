// D129: CLI for one benchmark invocation (wrapped by
// scripts/benchmark-model.sh). Flags mirror the responsibilities the
// D128 contract reserves for benchmark-model.sh: sanitized config,
// fixture manifest, repetition and population selection.

import { loadHarnessConfig, runOne } from './run-model.ts';

function usage(): never {
  process.stderr.write(
    'usage: benchmark-model.sh --config <config.json> --fixture <fixture-dir> --out <records.jsonl>\n' +
      '                          [--population warm|cold] [--repetition N] [--planned N]\n' +
      '                          [--run-id ID] [--group-id ID] [--pair-id ID] [--timestamp RFC3339]\n',
  );
  process.exit(2);
}

function parseArgs(argv: string[]): Map<string, string> {
  const flags = new Map<string, string>();
  for (let i = 0; i < argv.length; i += 2) {
    const flag = argv[i];
    const value = argv[i + 1];
    if (flag === undefined || !flag.startsWith('--') || value === undefined) usage();
    flags.set(flag.slice(2), value);
  }
  return flags;
}

const flags = parseArgs(process.argv.slice(2));
const configPath = flags.get('config');
const fixtureDir = flags.get('fixture');
const outFile = flags.get('out');
if (configPath === undefined || fixtureDir === undefined || outFile === undefined) usage();

const population = flags.get('population') ?? 'warm';
if (population !== 'warm' && population !== 'cold') usage();

const runId = flags.get('run-id');
const groupId = flags.get('group-id');
const pairId = flags.get('pair-id');
const timestampUtc = flags.get('timestamp');

runOne({
  config: loadHarnessConfig(configPath),
  fixtureDir,
  population,
  repetition: Number(flags.get('repetition') ?? '1'),
  plannedRepetitions: Number(flags.get('planned') ?? '3'),
  outFile,
  ...(runId !== undefined ? { runId } : {}),
  ...(groupId !== undefined ? { groupId } : {}),
  ...(pairId !== undefined ? { pairId } : {}),
  ...(timestampUtc !== undefined ? { timestampUtc } : {}),
})
  .then((record) => {
    process.stdout.write(`recorded ${record.run.id} (${record.outcome.status}) → ${outFile}\n`);
  })
  .catch((err: unknown) => {
    process.stderr.write(`benchmark-model: ${err instanceof Error ? err.message : String(err)}\n`);
    process.exit(1);
  });
