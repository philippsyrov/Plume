export function parseFlags(line: string): string[] {
  return line
    .split(',')
    .map((flag) => flag.trim())
    .filter((flag) => flag.length >= 0);
}
