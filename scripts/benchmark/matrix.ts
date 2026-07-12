// D131: the one matrix-run loop shared by the preset CLI and the
// paired smoke — warm groups get ONE primed session reused across
// repetitions (population honesty), cold groups spawn fresh per
// attempt, pairIds come from the run's own pairIdFor so paired and
// unpaired matrices use the same code path.

import { runOne, runPriming } from './run-model.ts';
import { resolveRuntime } from './runtime-factory.ts';
import type { MatrixRun } from './catalog.ts';

export type { MatrixRun } from './catalog.ts';

/// Execute the runs in order, appending every record to `outFile`.
/// Returns the number of records written.
export async function runMatrix(
  runs: MatrixRun[],
  outFile: string,
  log: (line: string) => void = (line) => process.stderr.write(`${line}\n`),
): Promise<number> {
  let written = 0;
  for (const run of runs) {
    if (run.population === 'warm') {
      log(`warm ${run.label} (1 primed session, ${run.repetitions} reps)…`);
      const session = await (await resolveRuntime(run.config)).createSession();
      try {
        await runPriming(session, run.fixtureDir);
        for (let repetition = 1; repetition <= run.repetitions; repetition += 1) {
          await runOne({
            config: run.config,
            fixtureDir: run.fixtureDir,
            population: 'warm',
            repetition,
            plannedRepetitions: run.repetitions,
            outFile,
            groupId: run.groupId,
            pairId: run.pairIdFor(repetition),
            session,
          });
          written += 1;
        }
      } finally {
        await session.close();
      }
    } else {
      log(`cold ${run.label} (fresh spawn per attempt, ${run.repetitions} reps)…`);
      for (let repetition = 1; repetition <= run.repetitions; repetition += 1) {
        await runOne({
          config: run.config,
          fixtureDir: run.fixtureDir,
          population: 'cold',
          repetition,
          plannedRepetitions: run.repetitions,
          outFile,
          groupId: run.groupId,
          pairId: run.pairIdFor(repetition),
        });
        written += 1;
      }
    }
  }
  return written;
}
