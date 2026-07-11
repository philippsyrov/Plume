// D129: CLI for the suite coordinator (wrapped by
// scripts/benchmark-suite.sh). One argument: the plan file.

import { loadPlan, runPlan } from './run-suite.ts';

const planPath = process.argv[2];
if (planPath === undefined || planPath === '--help') {
  process.stderr.write('usage: benchmark-suite.sh <plan.json>\n');
  process.exit(2);
}

const plan = loadPlan(planPath);
runPlan(plan)
  .then((written) => {
    process.stdout.write(`recorded ${written} attempts → ${plan.outFile}\n`);
  })
  .catch((err: unknown) => {
    process.stderr.write(`benchmark-suite: ${err instanceof Error ? err.message : String(err)}\n`);
    process.exit(1);
  });
