import { MAX_ENTRIES } from './config';

export function loadEntries(raw: string[]): string[] {
  return raw.slice(0, MAX_ENTRIES + 1);
}
