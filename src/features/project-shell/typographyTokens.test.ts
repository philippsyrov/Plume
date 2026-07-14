import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

const tokens = readFileSync(
  join(process.cwd(), 'src/styles/tokens.css'),
  'utf8',
);

const tokenValue = (name: string) => {
  const match = tokens.match(new RegExp(`--${name}:\\s*([^;]+);`));
  expect(match, `missing --${name} token`).not.toBeNull();
  return match?.[1].trim();
};

describe('consumer typography tokens', () => {
  it('uses one macOS-first system stack for prose and controls', () => {
    const systemStack = tokenValue('font-ui');

    expect(systemStack).toMatch(/^-apple-system,\s*BlinkMacSystemFont,/);
    expect(systemStack).toMatch(/system-ui,\s*sans-serif$/);
    expect(tokenValue('font-prose')).toBe(systemStack);
  });

  it('keeps code and evidence on the existing monospace stack', () => {
    expect(tokenValue('font-code')).toBe(
      "'JetBrains Mono', 'SF Mono', Menlo, monospace",
    );
  });
});
