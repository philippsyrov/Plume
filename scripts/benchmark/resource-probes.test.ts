// @vitest-environment node
//
// D129B: resource-probe tests — deterministic throughout. Parsers get
// captured real command output; the sampler gets a scripted command
// runner (no vm_stat/sysctl/osascript spawned); the runOne integration
// uses the fake runtime plus an injected fake sampler. Nothing here
// depends on the machine's actual memory, swap, or thermal state.

import { mkdtempSync, rmSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterAll, describe, expect, it } from 'vitest';

import { runOne } from './run-model.ts';
import {
  NULL_READINGS,
  parseSwapUsedBytes,
  parseVmStatUsedBytes,
  startResourceSampler,
  thermalStateFromProbe,
} from './resource-probes.ts';
import type { CommandRunner, ResourceReadings } from './resource-probes.ts';
import { fakeConfig, fixtureDir, withPlumeEnv } from './test-support.ts';

const VM_STAT_OUTPUT = `Mach Virtual Memory Statistics: (page size of 16384 bytes)
Pages free:                                    66362.
Pages active:                                 246990.
Pages inactive:                               212058.
Pages speculative:                             35048.
Pages throttled:                                   0.
Pages wired down:                             153448.
Pages purgeable:                                8536.
"Translation faults":                      923274056.
Pages stored in compressor:                   673832.
Pages occupied by compressor:                 280428.
Pageins:                                    34689476.
`;

const SWAP_OUTPUT = 'vm.swapusage: total = 2048.00M  used = 494.44M  free = 1553.56M  (encrypted)\n';

describe('probe output parsers', () => {
  it('computes machine memory-used bytes from vm_stat', () => {
    // (active + wired down + occupied by compressor) × page size.
    expect(parseVmStatUsedBytes(VM_STAT_OUTPUT)).toBe((246990 + 153448 + 280428) * 16384);
  });

  it('returns null when any component line is missing — never a partial sum', () => {
    const missingCompressor = VM_STAT_OUTPUT.replace(/^Pages occupied by compressor:.*\n/m, '');
    expect(parseVmStatUsedBytes(missingCompressor)).toBeNull();
    expect(parseVmStatUsedBytes('page size of 16384 bytes\n')).toBeNull();
    expect(parseVmStatUsedBytes('Pages active: 5.\n')).toBeNull(); // no page size
    expect(parseVmStatUsedBytes('')).toBeNull();
  });

  it('parses sysctl vm.swapusage used bytes across unit suffixes', () => {
    expect(parseSwapUsedBytes(SWAP_OUTPUT)).toBe(Math.round(494.44 * 1024 ** 2));
    expect(parseSwapUsedBytes('vm.swapusage: total = 4.00G  used = 1.50G  free = 2.50G')).toBe(
      Math.round(1.5 * 1024 ** 3),
    );
    expect(parseSwapUsedBytes('vm.swapusage: total = 0.00M  used = 512.00K  free = 0.00M')).toBe(512 * 1024);
  });

  it('returns null for unparseable swap output', () => {
    expect(parseSwapUsedBytes('')).toBeNull();
    expect(parseSwapUsedBytes('vm.swapusage: total = 2048.00M')).toBeNull();
  });

  it('maps NSProcessInfo thermal states, reserving unknown for unclassified integers', () => {
    expect(thermalStateFromProbe('0\n')).toBe('nominal');
    expect(thermalStateFromProbe('1')).toBe('fair');
    expect(thermalStateFromProbe('2')).toBe('serious');
    expect(thermalStateFromProbe('3')).toBe('critical');
    // A working probe reporting a state outside the mapping is
    // `unknown` — per contract, exactly and only that case.
    expect(thermalStateFromProbe('7')).toBe('unknown');
    // A probe that did not work is null, never a guessed enum.
    expect(thermalStateFromProbe('execution error: something')).toBeNull();
    expect(thermalStateFromProbe('')).toBeNull();
  });
});

/// Scripted runner: each command key holds a queue of replies; the
/// last entry repeats once exhausted (interval tick count is not
/// deterministic). An Error entry makes that call reject.
function scriptedRunner(script: Record<string, Array<string | Error>>): { runner: CommandRunner; calls: string[] } {
  const calls: string[] = [];
  const runner: CommandRunner = (bin) => {
    calls.push(bin);
    const queue = script[bin];
    if (queue === undefined || queue.length === 0) return Promise.reject(new Error(`no script for ${bin}`));
    const reply = queue.length > 1 ? queue.shift() : queue[0];
    if (reply instanceof Error) return Promise.reject(reply);
    if (reply === undefined) return Promise.reject(new Error(`no script for ${bin}`));
    return Promise.resolve(reply);
  };
  return { runner, calls };
}

const vmStat = (activePages: number): string =>
  `Mach Virtual Memory Statistics: (page size of 16384 bytes)\nPages active: ${activePages}.\nPages wired down: 0.\nPages occupied by compressor: 0.\n`;
const swap = (usedMegabytes: string): string => `vm.swapusage: total = 2048.00M  used = ${usedMegabytes}M  free = 0.00M`;

describe('resource sampler (scripted runner)', () => {
  it('tracks the peak memory sample, start/end thermal, and a signed swap delta', async () => {
    const { runner } = scriptedRunner({
      // initial 100 pages, mid-run spike to 900, final 300 — the peak
      // must be the spike, not the last sample.
      vm_stat: [vmStat(100), vmStat(900), vmStat(300)],
      sysctl: [swap('100.00'), swap('40.00')],
      osascript: ['0', '2'],
    });
    const sampler = await startResourceSampler({ runner, intervalMs: 5, platform: 'darwin' });
    await new Promise((resolve) => setTimeout(resolve, 40)); // let ticks consume the spike
    const readings = await sampler.stop();
    expect(readings.peakUnifiedMemoryBytes).toBe(900 * 16384);
    expect(readings.thermalStart).toBe('nominal');
    expect(readings.thermalEnd).toBe('serious');
    // Swap shrank: the delta is negative and NOT clamped to zero.
    expect(readings.swapDeltaBytes).toBe(Math.round(40 * 1024 ** 2) - Math.round(100 * 1024 ** 2));
    expect(readings.wallEnergyJoules).toBeNull();
  });

  it('records null for a failing probe without disturbing the others', async () => {
    const { runner } = scriptedRunner({
      vm_stat: [vmStat(500)],
      sysctl: [new Error('sysctl exploded')],
      osascript: ['1'],
    });
    const sampler = await startResourceSampler({ runner, intervalMs: 60_000, platform: 'darwin' });
    const readings = await sampler.stop();
    expect(readings.swapDeltaBytes).toBeNull();
    expect(readings.peakUnifiedMemoryBytes).toBe(500 * 16384);
    expect(readings.thermalStart).toBe('fair');
    expect(readings.thermalEnd).toBe('fair');
  });

  it('records a null delta when only one swap endpoint is available', async () => {
    const { runner } = scriptedRunner({
      vm_stat: [vmStat(500)],
      sysctl: [swap('100.00'), new Error('gone at end')],
      osascript: ['0'],
    });
    const sampler = await startResourceSampler({ runner, intervalMs: 60_000, platform: 'darwin' });
    const readings = await sampler.stop();
    expect(readings.swapDeltaBytes).toBeNull();
  });

  it('never rejects: every probe failing yields all-null readings', async () => {
    const { runner } = scriptedRunner({});
    const sampler = await startResourceSampler({ runner, intervalMs: 60_000, platform: 'darwin' });
    const readings = await sampler.stop();
    expect(readings).toEqual(NULL_READINGS);
  });

  it('spawns nothing and reads all-null off macOS', async () => {
    const { runner, calls } = scriptedRunner({ vm_stat: [vmStat(1)] });
    const sampler = await startResourceSampler({ runner, intervalMs: 5, platform: 'linux' });
    await new Promise((resolve) => setTimeout(resolve, 20));
    const readings = await sampler.stop();
    expect(readings).toEqual(NULL_READINGS);
    expect(calls).toEqual([]);
  });
});

const outDir = mkdtempSync(path.join(os.tmpdir(), 'plume-probe-int-'));
afterAll(() => rmSync(outDir, { recursive: true, force: true }));

let fileCounter = 0;
async function fakeRun(extra?: {
  samplerFactory?: () => Promise<{ stop(): Promise<ResourceReadings> }>;
}): Promise<Awaited<ReturnType<typeof runOne>>> {
  fileCounter += 1;
  return withPlumeEnv(() =>
    runOne({
      config: fakeConfig('short-chat-pass'),
      fixtureDir: fixtureDir('short-chat', 'fact-001'),
      population: 'warm',
      repetition: 1,
      plannedRepetitions: 3,
      outFile: path.join(outDir, `records-${fileCounter}.jsonl`),
      timestampUtc: '2026-07-11T12:00:00Z',
      ...extra,
    }),
  );
}

describe('runOne resource integration', () => {
  it('keeps fake-runtime records fully null — probes are gated to real transports', async () => {
    const record = await fakeRun();
    expect(record.host.thermalStart).toBeNull();
    expect(record.resources).toEqual({
      peakUnifiedMemoryBytes: null,
      swapDeltaBytes: null,
      thermalEnd: null,
      wallEnergyJoules: null,
    });
  });

  it('carries sampled readings into the record when a sampler is active', async () => {
    const readings: ResourceReadings = {
      thermalStart: 'nominal',
      peakUnifiedMemoryBytes: 11_003_904,
      swapDeltaBytes: -262_144,
      thermalEnd: 'fair',
      wallEnergyJoules: null,
    };
    let stopped = 0;
    const record = await fakeRun({
      samplerFactory: () => Promise.resolve({ stop: async () => (stopped += 1, readings) }),
    });
    expect(stopped).toBe(1);
    expect(record.host.thermalStart).toBe('nominal');
    expect(record.resources).toEqual({
      peakUnifiedMemoryBytes: 11_003_904,
      swapDeltaBytes: -262_144,
      thermalEnd: 'fair',
      wallEnergyJoules: null,
    });
    expect(record.outcome.status).toBe('passed'); // probes never affect the run
  });

  it('still completes the run and records nulls when the sampler fails to start', async () => {
    const record = await fakeRun({
      samplerFactory: () => Promise.reject(new Error('probe environment broken')),
    });
    expect(record.outcome.status).toBe('passed');
    expect(record.resources.peakUnifiedMemoryBytes).toBeNull();
    expect(record.host.thermalStart).toBeNull();
  });

  it('still completes the run and records nulls when stop() rejects', async () => {
    const record = await fakeRun({
      samplerFactory: () => Promise.resolve({ stop: () => Promise.reject(new Error('stop exploded')) }),
    });
    expect(record.outcome.status).toBe('passed');
    expect(record.resources.swapDeltaBytes).toBeNull();
  });
});
