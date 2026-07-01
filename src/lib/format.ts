// D109: shared byte-count formatter. Extracted from ProvidersPanel.tsx and
// LocalModelsPanel.tsx, whose copies were byte-for-byte identical — a pure
// move, not a rewrite. Other `formatBytes` variants in the codebase
// (MemoryPanel, MemoryTopics, SystemChips, chat/formatters, FileBrowser) use
// different tiering/precision/labels for their own display needs and are
// deliberately left as-is; this is not the one-true-formatter, just the
// dedup of one proven-identical pair.

export function formatBytes(bytes: number): string {
  const KIB = 1024;
  const MIB = KIB * 1024;
  const GIB = MIB * 1024;
  if (bytes >= GIB) return `${(bytes / GIB).toFixed(1)} GB`;
  if (bytes >= MIB) return `${Math.round(bytes / MIB)} MB`;
  if (bytes >= KIB) return `${Math.round(bytes / KIB)} KB`;
  return `${bytes} B`;
}
