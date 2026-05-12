// Typed wrapper for the `system.snapshot` IPC verb. Mirrors
// `docs/IPC_CONTRACT.md § system`. The status strip polls this on a
// slow tick (5–10 s) to render RAM / swap / pressure chips; the
// signal is cheap on macOS (Activity Monitor's underlying CLI tools)
// but every field is optional so callers must render "unknown"
// distinctly from "low".

import { invokeIpc } from './ipc';

export type MemoryPressure = 'normal' | 'warn' | 'high' | 'unknown';

export type MemoryStats = {
  pageSizeBytes: number;
  freeBytes: number;
  activeBytes: number;
  inactiveBytes: number;
  wiredBytes: number;
  compressedBytes: number;
  /** "Memory Used" as Activity Monitor displays it. */
  usedBytes: number;
  /** Best-effort headroom (free + inactive). */
  availableBytes: number;
  totalBytes: number;
};

export type SwapStats = {
  totalBytes: number;
  usedBytes: number;
  freeBytes: number;
};

export type LoadAverage = {
  one: number;
  five: number;
  fifteen: number;
};

export type MachineSnapshot = {
  probedAtMs: number;
  /** Authoritative total RAM. Prefer this over MemoryStats.totalBytes. */
  physicalMemoryBytes: number | null;
  memory: MemoryStats | null;
  swap: SwapStats | null;
  loadAverage: LoadAverage | null;
  pressure: MemoryPressure;
  arch: string | null;
  osName: string | null;
  osVersion: string | null;
  cpuBrand: string | null;
};

type EmptyPayload = Record<string, never>;

/**
 * Fetch one host machine snapshot. The status strip polls this on a
 * slow tick; do NOT call it more often than every few seconds — the
 * underlying readers are cheap but they still spawn a few processes
 * on each call.
 */
export function getSystemSnapshot(): Promise<MachineSnapshot> {
  return invokeIpc<EmptyPayload, MachineSnapshot>('system_snapshot', {});
}

/** Render-friendly label for a memory-pressure verdict. */
export function pressureLabel(state: MemoryPressure): string {
  switch (state) {
    case 'normal':
      return 'mem ok';
    case 'warn':
      return 'mem warn';
    case 'high':
      return 'mem high';
    case 'unknown':
      return 'mem ?';
  }
}
