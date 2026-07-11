// D129: suite coordinator (wrapped by scripts/benchmark-suite.sh).
// Reads a plan, orders warm/cold groups and repetitions, primes warm
// populations with one unrecorded request, and appends every attempt
// record to one JSONL collection via the single-invocation runner.

import { readFileSync } from 'node:fs';
import { randomUUID } from 'node:crypto';

import { loadHarnessConfig, runOne, runPriming } from './run-model.ts';
import { resolveRuntime } from './runtime-factory.ts';
import type { HarnessConfig } from './runtime-factory.ts';

export interface PlanGroup {
  groupId?: string;
  fixture: string;
  population: 'cold' | 'warm';
  repetitions: number;
}

export interface SuitePlan {
  config: string;
  outFile: string;
  groups: PlanGroup[];
}

export function loadPlan(planPath: string): SuitePlan {
  const parsed: unknown = JSON.parse(readFileSync(planPath, 'utf8'));
  const plan = parsed as SuitePlan;
  if (typeof plan.config !== 'string' || typeof plan.outFile !== 'string' || !Array.isArray(plan.groups)) {
    throw new Error(`${planPath}: plan needs config, outFile, and groups`);
  }
  for (const group of plan.groups) {
    if (typeof group.fixture !== 'string') throw new Error(`${planPath}: every group needs a fixture directory`);
    if (group.population !== 'warm' && group.population !== 'cold') {
      throw new Error(`${planPath}: population must be warm or cold`);
    }
    if (!Number.isInteger(group.repetitions) || group.repetitions < 3 || group.repetitions > 30) {
      throw new Error(`${planPath}: repetitions must be 3..30 (incomplete evidence below three)`);
    }
  }
  return plan;
}

export async function runPlan(plan: SuitePlan, config?: HarnessConfig): Promise<number> {
  const harnessConfig = config ?? loadHarnessConfig(plan.config);
  let written = 0;
  for (const group of plan.groups) {
    const groupId = group.groupId ?? `grp_${randomUUID()}`;
    // A warm group is one LIVE runtime session: primed once with an
    // unrecorded request, then every measured repetition runs in that
    // same loaded process. A cold group spawns a fresh process per
    // repetition (coldMethod: processRestart).
    const session = group.population === 'warm' ? await (await resolveRuntime(harnessConfig)).createSession() : undefined;
    try {
      if (session !== undefined) {
        await runPriming(session, group.fixture);
      }
      for (let repetition = 1; repetition <= group.repetitions; repetition += 1) {
        await runOne({
          config: harnessConfig,
          fixtureDir: group.fixture,
          population: group.population,
          repetition,
          plannedRepetitions: group.repetitions,
          outFile: plan.outFile,
          groupId,
          ...(session !== undefined ? { session } : {}),
        });
        written += 1;
      }
    } finally {
      await session?.close();
    }
  }
  return written;
}
