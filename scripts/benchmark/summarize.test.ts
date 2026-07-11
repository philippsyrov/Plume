// D129: summarizer tests — grouping, populations, derived counts,
// spread math, pair validation, and rendering posture (median as the
// group result, never a fastest run; fake-runtime banner).

import { describe, expect, it } from 'vitest';

import { makeValidRecord } from './example-record.ts';
import { computeStats, readRecords, renderMarkdown, summarizeGroups, summarizePairs } from './summarize-lib.ts';
import { serializeRecord } from './validate.ts';
import type { BenchmarkRecord } from './types.ts';

function attempt(overrides: {
  id: string;
  groupId?: string;
  population?: 'cold' | 'warm';
  repetition?: number;
  endToEndMs?: number | null;
  status?: BenchmarkRecord['outcome']['status'];
  include?: boolean;
  exclusionReason?: string | null;
  measurementPath?: 'rawRuntime' | 'plumeOrchestration';
  pairId?: string | null;
}): BenchmarkRecord {
  const r = makeValidRecord();
  r.run.id = overrides.id;
  r.run.groupId = overrides.groupId ?? 'grp_A';
  r.run.repetition = overrides.repetition ?? 1;
  if (overrides.population === 'cold') {
    r.run.population = 'cold';
    r.run.coldMethod = 'processRestart';
  }
  if (overrides.endToEndMs !== undefined) r.timing.endToEndMs = overrides.endToEndMs;
  if (overrides.status !== undefined) {
    r.outcome.status = overrides.status;
    if (overrides.status === 'timedOut') {
      r.outcome.timeout = true;
      r.outcome.stream = 'timedOut';
    }
    if (overrides.status === 'error') {
      r.outcome.crash = true;
      r.outcome.stream = 'crashed';
      r.outcome.errorClass = 'runtime-crash';
    }
  }
  if (overrides.include === false) {
    r.includeInSummary = false;
    r.exclusionReason = overrides.exclusionReason ?? 'deliberate-cancellation';
  }
  if (overrides.measurementPath !== undefined) r.run.measurementPath = overrides.measurementPath;
  if (overrides.pairId !== undefined) r.run.pairId = overrides.pairId;
  return r;
}

describe('readRecords', () => {
  it('parses valid lines and reports invalid ones by line number', () => {
    const good = serializeRecord(makeValidRecord());
    const text = `${good}\n{nope\n${good}\n`;
    const result = readRecords(text);
    expect(result.records).toHaveLength(2);
    expect(result.lineErrors).toHaveLength(1);
    expect(result.lineErrors[0]).toContain('line 2');
  });

  it('refuses a whole input containing a newer schema version', () => {
    const newer = makeValidRecord() as unknown as Record<string, unknown>;
    newer['schemaVersion'] = 2;
    expect(() => readRecords(JSON.stringify(newer))).toThrow(/newer than supported/);
  });
});

describe('summarizeGroups', () => {
  it('never folds cold and warm into one summary', () => {
    const records = [
      attempt({ id: 'a1', repetition: 1, endToEndMs: 10 }),
      attempt({ id: 'a2', repetition: 2, endToEndMs: 20 }),
      attempt({ id: 'a3', repetition: 3, endToEndMs: 30 }),
      attempt({ id: 'c1', repetition: 1, population: 'cold', endToEndMs: 100 }),
      attempt({ id: 'c2', repetition: 2, population: 'cold', endToEndMs: 200 }),
      attempt({ id: 'c3', repetition: 3, population: 'cold', endToEndMs: 300 }),
    ];
    const groups = summarizeGroups(records);
    expect(groups).toHaveLength(2);
    const warm = groups.find((g) => g.population === 'warm');
    const cold = groups.find((g) => g.population === 'cold');
    expect(warm?.endToEndMs?.median).toBe(20);
    expect(cold?.endToEndMs?.median).toBe(200);
  });

  it('marks fewer than three completed repetitions as incomplete evidence', () => {
    const groups = summarizeGroups([
      attempt({ id: 'a1', repetition: 1, endToEndMs: 10 }),
      attempt({ id: 'a2', repetition: 2, endToEndMs: 20 }),
    ]);
    expect(groups[0]?.incomplete).toBe(true);
    expect(groups[0]?.endToEndMs).toBeNull();
  });

  it('keeps excluded attempts in reliability totals but out of stats', () => {
    const records = [
      attempt({ id: 'a1', repetition: 1, endToEndMs: 10 }),
      attempt({ id: 'a2', repetition: 2, endToEndMs: 20 }),
      attempt({ id: 'a3', repetition: 3, endToEndMs: 30 }),
      attempt({ id: 'a4', repetition: 4, endToEndMs: 999, include: false }),
      attempt({ id: 'a5', repetition: 5, endToEndMs: null, status: 'timedOut' }),
      attempt({ id: 'a6', repetition: 6, endToEndMs: null, status: 'error' }),
    ];
    const groups = summarizeGroups(records);
    const g = groups[0];
    expect(g?.reliability.attempts).toBe(6);
    expect(g?.reliability.timeouts).toBe(1);
    expect(g?.reliability.crashes).toBe(1);
    expect(g?.excluded).toBe(1);
    // 999 (excluded) and the two non-completions never enter the stats.
    expect(g?.endToEndMs?.count).toBe(3);
    expect(g?.endToEndMs?.max).toBe(30);
  });

  it('REFUSES statistics for a group with configuration drift', () => {
    const drifted = attempt({ id: 'a3', repetition: 3, endToEndMs: 30 });
    drifted.model.sampling.temperature = 0.7;
    const groups = summarizeGroups([
      attempt({ id: 'a1', repetition: 1, endToEndMs: 10 }),
      attempt({ id: 'a2', repetition: 2, endToEndMs: 20 }),
      drifted,
    ]);
    const g = groups[0];
    expect(g?.configErrors.join('\n')).toContain('different configuration');
    expect(g?.refused).toBe(true);
    // Three completed attempts — NOT incomplete — yet no joint median:
    // mixed configurations are refused, not blended.
    expect(g?.incomplete).toBe(false);
    expect(g?.endToEndMs).toBeNull();
    expect(g?.generationTokensPerSecond).toBeNull();
    // Reliability totals still count every attempt.
    expect(g?.reliability.attempts).toBe(3);
  });

  it('refuses a group with a duplicated repetition', () => {
    const groups = summarizeGroups([
      attempt({ id: 'a1', repetition: 1, endToEndMs: 10 }),
      attempt({ id: 'a2', repetition: 1, endToEndMs: 20 }),
      attempt({ id: 'a3', repetition: 2, endToEndMs: 30 }),
    ]);
    expect(groups[0]?.refused).toBe(true);
    expect(groups[0]?.configErrors.join('\n')).toContain('repetition 1 recorded twice');
    expect(groups[0]?.endToEndMs).toBeNull();
  });

  it('refuses groups containing a duplicated run id', () => {
    const groups = summarizeGroups([
      attempt({ id: 'a1', repetition: 1, endToEndMs: 10 }),
      attempt({ id: 'a1', repetition: 2, endToEndMs: 20 }),
      attempt({ id: 'a3', repetition: 3, endToEndMs: 30 }),
    ]);
    expect(groups[0]?.refused).toBe(true);
    expect(groups[0]?.configErrors.join('\n')).toContain('appears more than once');
    expect(groups[0]?.endToEndMs).toBeNull();
  });

  it('refuses a group whose planned repetition counts disagree', () => {
    const odd = attempt({ id: 'a3', repetition: 3, endToEndMs: 30 });
    odd.run.plannedRepetitions = 10;
    const groups = summarizeGroups([
      attempt({ id: 'a1', repetition: 1, endToEndMs: 10 }),
      attempt({ id: 'a2', repetition: 2, endToEndMs: 20 }),
      odd,
    ]);
    expect(groups[0]?.refused).toBe(true);
    expect(groups[0]?.configErrors.join('\n')).toContain('plans 10 repetitions');
    expect(groups[0]?.endToEndMs).toBeNull();
  });
});

describe('computeStats', () => {
  it('computes median, spread, and IQR only at four or more values', () => {
    const stats = computeStats([40, 10, 30, 20]);
    expect(stats).toEqual({ count: 4, median: 25, min: 10, max: 40, iqr: 15 });
    expect(computeStats([10, 20, 30])?.iqr).toBeNull();
    expect(computeStats([])).toBeNull();
  });
});

describe('summarizePairs', () => {
  it('derives extraOverheadMs from a valid raw/plume pair', () => {
    const raw = attempt({ id: 'r1', pairId: 'pair_1', endToEndMs: 100, measurementPath: 'rawRuntime' });
    const plume = attempt({ id: 'p1', pairId: 'pair_1', endToEndMs: 130, measurementPath: 'plumeOrchestration' });
    const pairs = summarizePairs([raw, plume]);
    expect(pairs[0]).toMatchObject({ pairId: 'pair_1', valid: true, extraOverheadMs: 30 });
  });

  it('invalidates a pair on configuration mismatch, keeping the attempts', () => {
    const raw = attempt({ id: 'r1', pairId: 'pair_1', endToEndMs: 100, measurementPath: 'rawRuntime' });
    const plume = attempt({ id: 'p1', pairId: 'pair_1', endToEndMs: 130, measurementPath: 'plumeOrchestration' });
    plume.tokens.outputTokens = 99; // different completed output-token count
    const pairs = summarizePairs([raw, plume]);
    expect(pairs[0]).toMatchObject({ valid: false, extraOverheadMs: null });
    expect(pairs[0]?.reason).toContain('configuration mismatch');
  });

  it('requires exactly one completed attempt per path', () => {
    const raw1 = attempt({ id: 'r1', pairId: 'pair_1', endToEndMs: 100, measurementPath: 'rawRuntime' });
    const raw2 = attempt({ id: 'r2', pairId: 'pair_1', endToEndMs: 110, measurementPath: 'rawRuntime' });
    const pairs = summarizePairs([raw1, raw2]);
    expect(pairs[0]?.valid).toBe(false);
    expect(pairs[0]?.reason).toContain('exactly one completed');
  });
});

describe('renderMarkdown', () => {
  it('banners fake-runtime records and reports the median, never the fastest run', () => {
    const records = [
      attempt({ id: 'a1', repetition: 1, endToEndMs: 10 }),
      attempt({ id: 'a2', repetition: 2, endToEndMs: 20 }),
      attempt({ id: 'a3', repetition: 3, endToEndMs: 30 }),
    ];
    const md = renderMarkdown(records);
    expect(md).toContain('HARNESS TEST DATA');
    expect(md).toContain('20.0 (min 10.0, max 30.0');
    // The headline cell leads with the median, not min.
    expect(md).not.toMatch(/\| 10\.0 \(/);
  });

  it('renders refusal instead of statistics for an inconsistent group', () => {
    const drifted = attempt({ id: 'a3', repetition: 3, endToEndMs: 30 });
    drifted.model.sampling.temperature = 0.7;
    const md = renderMarkdown([
      attempt({ id: 'a1', repetition: 1, endToEndMs: 10 }),
      attempt({ id: 'a2', repetition: 2, endToEndMs: 20 }),
      drifted,
    ]);
    expect(md).toContain('refused (inconsistent group)');
    expect(md).toContain('**Configuration errors:**');
    expect(md).not.toContain('20.0 (min');
  });

  it('omits the banner for non-fake engines', () => {
    const record = attempt({ id: 'a1', endToEndMs: 10 });
    record.runtime.engine = 'mlx-lm';
    expect(renderMarkdown([record])).not.toContain('HARNESS TEST DATA');
  });
});
