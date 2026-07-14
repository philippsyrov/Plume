import { useSyncExternalStore } from 'react';

let userMemoryRevision = 0;
const listeners = new Set<() => void>();

export function bumpUserMemoryRevision(): void {
  userMemoryRevision += 1;
  for (const listener of [...listeners]) listener();
}

export function useUserMemoryRevision(): number {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => userMemoryRevision,
    () => userMemoryRevision,
  );
}
