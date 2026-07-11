// D129B: machine resource probes for real-runtime records.
//
// Fills the schema-v1 fields the harness previously recorded as null:
//   * resources.peakUnifiedMemoryBytes — maximum machine unified-memory
//     usage observed over the measured request (vm_stat: active +
//     wired + occupied-by-compressor pages × page size, sampled on an
//     interval; the formula is a documented proxy for "memory used").
//   * resources.swapDeltaBytes — sysctl vm.swapusage "used" at request
//     end minus at request start; may be negative, never clamped.
//   * host.thermalStart / resources.thermalEnd — NSProcessInfo
//     thermalState via osascript JXA (a genuine 4-level macOS probe:
//     0 nominal, 1 fair, 2 serious, 3 critical). An integer outside
//     that range is `unknown` (supported probe, unclassified state);
//     a failed probe is null.
//   * resources.wallEnergyJoules — ALWAYS null here: wall energy needs
//     an external meter; nothing on this hardware measures at the
//     wall, and package-power estimates are not wall energy.
//
// Failure posture (docs/MODEL_BENCHMARKS.md § Field rules): a probe
// that fails or does not exist records null — never 0, never a made-up
// enum — and never fails or delays the model run itself. Start probes
// complete BEFORE the request is sent and end probes run AFTER the
// terminal event, so no probe work sits inside the timed windows; only
// the lightweight memory sampler ticks concurrently (unavoidable —
// a peak is by definition observed during the run).

import { execFile } from 'node:child_process';

import { THERMAL_STATES } from './types.ts';

export type ThermalState = (typeof THERMAL_STATES)[number];

/// Everything runOne needs to fill host.thermalStart and the
/// resources block.
export interface ResourceReadings {
  thermalStart: ThermalState | null;
  peakUnifiedMemoryBytes: number | null;
  swapDeltaBytes: number | null;
  thermalEnd: ThermalState | null;
  wallEnergyJoules: number | null;
}

export const NULL_READINGS: ResourceReadings = {
  thermalStart: null,
  peakUnifiedMemoryBytes: null,
  swapDeltaBytes: null,
  thermalEnd: null,
  wallEnergyJoules: null,
};

/// Runs one probe command and resolves its stdout. Injectable so the
/// sampler is deterministically testable without macOS.
export type CommandRunner = (bin: string, args: string[]) => Promise<string>;

const defaultRunner: CommandRunner = (bin, args) =>
  new Promise((resolve, reject) => {
    execFile(bin, args, { encoding: 'utf8', timeout: 10_000 }, (error, stdout) => {
      if (error !== null) reject(error);
      else resolve(stdout);
    });
  });

/// vm_stat → machine "memory used" bytes: (active + wired down +
/// occupied by compressor) pages × the page size from the header.
/// Any missing component → null (never a partial sum).
export function parseVmStatUsedBytes(output: string): number | null {
  const pageSizeMatch = output.match(/page size of (\d+) bytes/);
  if (pageSizeMatch?.[1] === undefined) return null;
  const pageSize = Number(pageSizeMatch[1]);
  const page = (label: string): number | null => {
    const match = output.match(new RegExp(`^${label}:\\s+(\\d+)\\.`, 'm'));
    return match?.[1] === undefined ? null : Number(match[1]);
  };
  const active = page('Pages active');
  const wired = page('Pages wired down');
  const compressor = page('Pages occupied by compressor');
  if (active === null || wired === null || compressor === null) return null;
  return (active + wired + compressor) * pageSize;
}

/// `sysctl vm.swapusage` → "used" bytes. macOS prints fixed-point
/// megabyte-ish values with a unit suffix (e.g. `used = 494.44M`).
export function parseSwapUsedBytes(output: string): number | null {
  const match = output.match(/used = ([\d.]+)([KMG])/);
  if (match?.[1] === undefined || match[2] === undefined) return null;
  const value = Number(match[1]);
  if (!Number.isFinite(value)) return null;
  const scale = { K: 1024, M: 1024 ** 2, G: 1024 ** 3 }[match[2]];
  if (scale === undefined) return null;
  return Math.round(value * scale);
}

/// NSProcessInfo.thermalState (via osascript JXA) → schema enum.
/// A non-integer reply means the probe did not work → null. An
/// integer outside 0..3 means a working probe reported a state this
/// mapping does not classify → 'unknown' (per the contract, `unknown`
/// is reserved for exactly that case).
export function thermalStateFromProbe(output: string): ThermalState | null {
  const raw = output.trim();
  if (!/^\d+$/.test(raw)) return null;
  const mapped = (['nominal', 'fair', 'serious', 'critical'] as const)[Number(raw)];
  return mapped ?? 'unknown';
}

const THERMAL_PROBE_ARGS = [
  '-l',
  'JavaScript',
  '-e',
  'ObjC.import("Foundation"); $.NSProcessInfo.processInfo.thermalState',
];

export interface ResourceSampler {
  /// Stop sampling, run the end-of-window probes, and return the
  /// readings. Never rejects — a broken probe is a null field.
  stop(): Promise<ResourceReadings>;
}

export interface SamplerOptions {
  runner?: CommandRunner;
  intervalMs?: number;
  /// Overrides the darwin gate (tests). Real runs rely on the
  /// platform check: on anything but macOS none of these probes
  /// exist, so everything is null without spawning garbage.
  platform?: string;
}

/// Start the resource window: runs the start-of-window probes to
/// completion (so none of their work overlaps the request), then
/// samples memory on an interval until stop(). Never throws.
export async function startResourceSampler(options?: SamplerOptions): Promise<ResourceSampler> {
  const platform = options?.platform ?? process.platform;
  if (platform !== 'darwin') {
    return { stop: () => Promise.resolve({ ...NULL_READINGS }) };
  }
  const runner = options?.runner ?? defaultRunner;
  const intervalMs = options?.intervalMs ?? 500;
  const failed = new Set<string>();
  const probe = async <T>(kind: string, bin: string, args: string[], parse: (out: string) => T | null): Promise<T | null> => {
    try {
      return parse(await runner(bin, args));
    } catch (err) {
      if (!failed.has(kind)) {
        failed.add(kind);
        console.error(`resource probe ${kind} failed (recording null):`, err instanceof Error ? err.message : String(err));
      }
      return null;
    }
  };
  const memorySample = (): Promise<number | null> => probe('memory', 'vm_stat', [], parseVmStatUsedBytes);
  const swapSample = (): Promise<number | null> => probe('swap', 'sysctl', ['vm.swapusage'], parseSwapUsedBytes);
  const thermalSample = (): Promise<ThermalState | null> =>
    probe('thermal', 'osascript', THERMAL_PROBE_ARGS, thermalStateFromProbe);

  // Start-of-window probes, fully awaited BEFORE the caller sends the
  // measured request.
  const [thermalStart, swapStart, firstMemory] = await Promise.all([thermalSample(), swapSample(), memorySample()]);

  let peak = firstMemory;
  let sampling = false;
  const takeMemorySample = async (): Promise<void> => {
    if (sampling) return; // never stack overlapping vm_stat spawns
    sampling = true;
    try {
      const sample = await memorySample();
      if (sample !== null && (peak === null || sample > peak)) peak = sample;
    } finally {
      sampling = false;
    }
  };
  const timer = setInterval(() => {
    takeMemorySample().catch((err) => console.error('resource sampler tick failed:', err instanceof Error ? err.message : String(err)));
  }, intervalMs);

  return {
    stop: async (): Promise<ResourceReadings> => {
      clearInterval(timer);
      try {
        // End-of-window probes run AFTER the terminal event — outside
        // every timed window.
        const [finalMemory, swapEnd, thermalEnd] = await Promise.all([memorySample(), swapSample(), thermalSample()]);
        if (finalMemory !== null && (peak === null || finalMemory > peak)) peak = finalMemory;
        return {
          thermalStart,
          peakUnifiedMemoryBytes: peak,
          swapDeltaBytes: swapStart !== null && swapEnd !== null ? swapEnd - swapStart : null,
          thermalEnd,
          // No supported wall-power meter exists here; a package-power
          // estimate would not be wall energy. Null until real
          // metering hardware is wired in.
          wallEnergyJoules: null,
        };
      } catch (err) {
        console.error('resource sampler stop failed (recording nulls):', err instanceof Error ? err.message : String(err));
        return { ...NULL_READINGS };
      }
    },
  };
}
