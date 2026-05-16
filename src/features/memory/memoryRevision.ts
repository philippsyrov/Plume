// D42 Codex fix — tiny revision bus so the chat panel's memory
// preview refreshes after the user remembers / forgets an entry
// from the Memory panel.
//
// The bug: `useChatContextPreview` only refetched on attachment or
// `projectHasInstructions` changes. Remember/forget happens through
// `memory.*` IPC, which doesn't emit a frontend event today. So the
// chat header's `MemoryBadge` could keep showing yesterday's
// "Memory · 3 entries · 412 B" even after the user clicked Forget
// on two of them. The actual `chat.send` was fine — assemble re-reads
// the store on every send — but the preview lied.
//
// The fix is the smallest honest shim: a module-scoped counter that
// remember/forget bump, plus a hook other consumers add to their
// effect deps. No new dependencies; no globals on `window`. The
// `useSyncExternalStore` API is the right primitive for "give me a
// number that increases when something changes," same shape we'd use
// for a future zustand/jotai store if we land one.

import { useSyncExternalStore } from 'react';

let revision = 0;
const listeners = new Set<() => void>();

/** Bump the revision counter and notify subscribers. Called from
 *  `MemoryPanel` after a successful `rememberMemory` /
 *  `forgetMemory` IPC round-trip. */
export function bumpMemoryRevision(): void {
  revision += 1;
  // Iterate over a snapshot so a listener that unsubscribes during
  // its own callback can't shorten the live iteration.
  for (const listener of [...listeners]) {
    listener();
  }
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function getSnapshot(): number {
  return revision;
}

/** Read the current revision. Bumps when `bumpMemoryRevision` runs;
 *  consumers depend on the return value in their effect deps to
 *  trigger a refetch after a remember/forget. */
export function useMemoryRevision(): number {
  return useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
}

// Test-only escape hatch. Production code must NOT reach for this —
// the bump function is the only legitimate way to advance the
// counter. Tests that exercise the bus without going through
// MemoryPanel call this to reset between cases.
//
// Not exported through the package barrel; the explicit import path
// keeps misuse visible in code review.
export function __resetMemoryRevisionForTests(): void {
  revision = 0;
  listeners.clear();
}
