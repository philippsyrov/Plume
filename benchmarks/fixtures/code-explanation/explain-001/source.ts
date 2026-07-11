// Synthetic benchmark fixture — deliberately buggy; never imported.
export function lastOf(items: string[]): string | undefined {
  // BUG: items.length is one past the final index.
  return items[items.length];
}
