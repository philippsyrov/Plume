// Status-strip chips for host machine state.
//
// Lives inside `ProjectStatusStrip` between the package-manager
// badges and the Close button.
//
// Visible-by-default chips:
//
//   * pressure — coloured headline (normal / warn / high / unknown).
//     The small "mem ok" verdict is the resting state of the strip
//     so the user always has a single, glanceable signal about
//     whether the machine has headroom for a local model.
//   * memory — "used / total G" headline always visible. The chip
//     IS the keyboard- and touch-accessible carrier of the numbers;
//     putting that data only in a hover-only `title` would make it
//     mouse-only (a non-focusable `<span>` does not surface `title`
//     on keyboard focus, so the "fold it into the pressure tooltip"
//     pattern is mouse-hover-only by definition).
//
// D19: the swap chip is gated on pressure (it has nothing useful
// to show on a healthy machine — `swap.usedBytes > 0` is also
// false then). The pressure + memory chips remain. The Codex P2
// fix restored memory to the always-on set: on `normal` the
// resting state is two chips (pressure + memory) rather than the
// previous one (pressure only), which keeps the calm-default goal
// of D19 (down from three) without making the headline numbers
// keyboard- or touch-unreachable.
//
// Honest wording: the chips are best-effort estimates, not perfect
// telemetry. The pressure verdict is a heuristic — see
// `src-tauri/src/system/mod.rs::MemoryPressure::derive` — so the
// tooltip explains how it was computed.

import { useSystemSnapshot } from './useSystemSnapshot';
import {
  pressureLabel,
  type MachineSnapshot,
  type MemoryPressure,
} from '../../lib/api/system';

export function SystemChips() {
  const state = useSystemSnapshot();
  if (state.kind === 'loading') {
    return (
      <span className="ink-badge plume-system-chip plume-system-chip-loading" role="status">
        reading host…
      </span>
    );
  }
  if (state.kind === 'error') {
    // First-load failure: surface the message so the user knows the
    // chips are missing for a reason.
    return (
      <span
        className="ink-badge plume-system-chip plume-system-chip-error"
        role="alert"
        title={state.message}
      >
        host ?
      </span>
    );
  }
  return <ReadyChips snapshot={state.snapshot} staleError={state.lastErrorMessage} />;
}

function ReadyChips({
  snapshot,
  staleError,
}: {
  snapshot: MachineSnapshot;
  staleError: string | null;
}) {
  // Pressure + memory are always visible. Memory carries the
  // headline "used / total G" string that sighted keyboard or
  // touch users need — a non-focusable `<span>` does not surface
  // its `title` on focus, so folding the numbers into the
  // pressure chip's tooltip alone would leave them mouse-only.
  // Swap stays conditional because it has nothing meaningful to
  // show on a healthy machine and adds visual weight without
  // earning it.
  const showSwap =
    snapshot.pressure !== 'normal' &&
    snapshot.swap !== null &&
    snapshot.swap.usedBytes > 0;
  return (
    <>
      <PressureChip snapshot={snapshot} staleError={staleError} />
      <MemoryChip snapshot={snapshot} />
      {showSwap ? <SwapChip snapshot={snapshot} /> : null}
    </>
  );
}

function PressureChip({
  snapshot,
  staleError,
}: {
  snapshot: MachineSnapshot;
  staleError: string | null;
}) {
  const tooltip = pressureTooltip(snapshot, staleError);
  return (
    <span
      className={`ink-badge plume-system-chip plume-pressure plume-pressure-${snapshot.pressure}`}
      title={tooltip}
      aria-label={tooltip}
    >
      {pressureLabel(snapshot.pressure)}
    </span>
  );
}

function MemoryChip({ snapshot }: { snapshot: MachineSnapshot }) {
  const total = snapshot.physicalMemoryBytes ?? snapshot.memory?.totalBytes ?? null;
  const used = snapshot.memory?.usedBytes ?? null;
  if (total === null || used === null) {
    return null;
  }
  const tooltip = memoryTooltip(snapshot, total, used);
  return (
    <span className="ink-badge plume-system-chip" title={tooltip} aria-label={tooltip}>
      {formatGib(used)} / {formatGib(total)}
    </span>
  );
}

function SwapChip({ snapshot }: { snapshot: MachineSnapshot }) {
  const swap = snapshot.swap!;
  const tooltip = `swap ${formatBytes(swap.usedBytes)} used of ${formatBytes(swap.totalBytes)}`;
  return (
    <span className="ink-badge plume-system-chip" title={tooltip} aria-label={tooltip}>
      swap {formatGib(swap.usedBytes)}
    </span>
  );
}

function pressureTooltip(snapshot: MachineSnapshot, staleError: string | null): string {
  const parts: string[] = [];
  parts.push(`Memory pressure: ${labelForTooltip(snapshot.pressure)}`);
  parts.push(
    'Estimate based on (active + wired + compressed) ÷ total; flips to "high" when swap is more than half used.',
  );
  // Fold the headline "used / total" + swap into the pressure
  // tooltip as well as the dedicated chips so a mouse-hover user
  // gets a one-stop summary. Duplicative with the memory chip on
  // screen, deliberately — the tooltip is a fallback path and a
  // single-hover context aid, not the only carrier of the data.
  const total = snapshot.physicalMemoryBytes ?? snapshot.memory?.totalBytes ?? null;
  const used = snapshot.memory?.usedBytes ?? null;
  if (total !== null && used !== null) {
    parts.push(`Memory used: ${formatBytes(used)} of ${formatBytes(total)}`);
  }
  if (snapshot.swap !== null && snapshot.swap.usedBytes > 0) {
    parts.push(
      `Swap used: ${formatBytes(snapshot.swap.usedBytes)} of ${formatBytes(snapshot.swap.totalBytes)}`,
    );
  }
  const machine = machineLabel(snapshot);
  if (machine !== null) parts.push(machine);
  if (snapshot.loadAverage !== null) {
    const { one, five, fifteen } = snapshot.loadAverage;
    parts.push(`Load avg (1/5/15): ${one.toFixed(2)} ${five.toFixed(2)} ${fifteen.toFixed(2)}`);
  }
  if (staleError !== null) parts.push(`Last probe failed: ${staleError}`);
  return parts.join('\n');
}

function memoryTooltip(snapshot: MachineSnapshot, total: number, used: number): string {
  const lines: string[] = [];
  lines.push(`Memory used: ${formatBytes(used)} of ${formatBytes(total)}`);
  if (snapshot.memory !== null) {
    const m = snapshot.memory;
    lines.push(`  active     ${formatBytes(m.activeBytes)}`);
    lines.push(`  wired      ${formatBytes(m.wiredBytes)}`);
    lines.push(`  compressed ${formatBytes(m.compressedBytes)}`);
    lines.push(`  inactive   ${formatBytes(m.inactiveBytes)}`);
    lines.push(`  free       ${formatBytes(m.freeBytes)}`);
    lines.push(`  available  ${formatBytes(m.availableBytes)}`);
  }
  lines.push('Same accounting as Activity Monitor; numbers are an estimate from vm_stat.');
  return lines.join('\n');
}

function machineLabel(snapshot: MachineSnapshot): string | null {
  const cpu = snapshot.cpuBrand;
  const arch = snapshot.arch;
  const os =
    snapshot.osName && snapshot.osVersion
      ? `${snapshot.osName} ${snapshot.osVersion}`
      : snapshot.osVersion;
  const bits = [cpu, arch, os].filter((s): s is string => !!s);
  return bits.length === 0 ? null : bits.join(' · ');
}

function labelForTooltip(state: MemoryPressure): string {
  switch (state) {
    case 'normal':
      return 'normal';
    case 'warn':
      return 'warn';
    case 'high':
      return 'high';
    case 'unknown':
      return 'unknown';
  }
}

function formatGib(bytes: number): string {
  const gib = bytes / 1024 / 1024 / 1024;
  // < 10 GiB → one decimal; ≥ 10 GiB → integer. Keeps the chip the
  // same visual width across machine sizes.
  return gib < 10 ? `${gib.toFixed(1)}G` : `${Math.round(gib)}G`;
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KiB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(0)} MiB`;
  return `${(bytes / 1024 / 1024 / 1024).toFixed(2)} GiB`;
}
