// D109: shared byte-count formatter. Extracted from ProvidersPanel.tsx and
// LocalModelsPanel.tsx, whose copies were byte-for-byte identical — a pure
// move, not a rewrite. MemoryPanel, MemoryTopics, and SystemChips have their
// own `formatBytes` variants with different tiering/precision/labels for
// their own display needs and are deliberately left as-is; this is not the
// one-true-formatter, just the dedup of one proven-identical pair.
// `chat/formatters.ts` and `FileBrowser.tsx` were a second identical pair —
// see `formatBytesOneDecimal` below (D113).

export function formatBytes(bytes: number): string {
  const KIB = 1024;
  const MIB = KIB * 1024;
  const GIB = MIB * 1024;
  if (bytes >= GIB) return `${(bytes / GIB).toFixed(1)} GB`;
  if (bytes >= MIB) return `${Math.round(bytes / MIB)} MB`;
  if (bytes >= KIB) return `${Math.round(bytes / KIB)} KB`;
  return `${bytes} B`;
}

// D113: second shared byte formatter — the `chat/formatters.ts` /
// `FileBrowser.tsx` pair was byte-for-byte identical to each other
// (same dedup as `formatBytes` above), but NOT identical to
// `formatBytes`: one decimal place at the KB/MB tiers, and no GB
// tier (a multi-GB file just keeps growing the MB number). Kept as
// a distinctly-named sibling rather than merged into `formatBytes`
// because collapsing them would change displayed output.
export function formatBytesOneDecimal(bytes: number): string {
  const KIB = 1024;
  const MIB = KIB * 1024;
  if (bytes < KIB) return `${bytes} B`;
  if (bytes < MIB) return `${(bytes / KIB).toFixed(1)} KB`;
  return `${(bytes / MIB).toFixed(1)} MB`;
}
