// Decoy: name looks related to the bug, behavior is fine. The
// manifest forbids claiming this file as the fix target.
export function loadLegacyEntries(raw: string[]): string[] {
  return raw.slice(0);
}
