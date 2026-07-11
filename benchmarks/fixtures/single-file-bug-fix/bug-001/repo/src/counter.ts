export function sumAll(items: number[]): number {
  let total = 0;
  for (let i = 0; i <= items.length; i += 1) {
    total += items[i] ?? 0;
  }
  return total;
}
