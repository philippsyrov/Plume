import { readFileSync } from 'node:fs';
import { join } from 'node:path';

import { describe, expect, it } from 'vitest';

const packageJson = JSON.parse(
  readFileSync(join(process.cwd(), 'package.json'), 'utf8'),
) as { version: string };
const packageLock = JSON.parse(
  readFileSync(join(process.cwd(), 'package-lock.json'), 'utf8'),
) as { version: string; packages: { '': { version: string } } };
const tauriConfig = JSON.parse(
  readFileSync(join(process.cwd(), 'src-tauri/tauri.conf.json'), 'utf8'),
) as {
  version: string;
  identifier: string;
  bundle: {
    targets: string[];
    macOS?: { signingIdentity?: string };
  };
};
const cargoToml = readFileSync(
  join(process.cwd(), 'src-tauri/Cargo.toml'),
  'utf8',
);
const cargoLock = readFileSync(
  join(process.cwd(), 'src-tauri/Cargo.lock'),
  'utf8',
);
const demoScript = readFileSync(
  join(process.cwd(), 'docs/build-week/demo-script.md'),
  'utf8',
);

function requiredVersion(source: string, pattern: RegExp, label: string): string {
  const match = source.match(pattern);
  if (!match?.[1]) {
    throw new Error(`missing ${label} version`);
  }
  return match[1];
}

describe('Build Week release metadata', () => {
  it('pins one 0.1.0 app version across JavaScript, Rust, and Tauri', () => {
    expect([
      packageJson.version,
      packageLock.version,
      packageLock.packages[''].version,
      tauriConfig.version,
      requiredVersion(cargoToml, /^version = "([^"]+)"/m, 'Cargo.toml'),
      requiredVersion(
        cargoLock,
        /\[\[package\]\]\nname = "plume"\nversion = "([^"]+)"/,
        'Cargo.lock plume package',
      ),
    ]).toEqual(Array(6).fill('0.1.0'));
  });

  it('keeps the production identity and emits app plus DMG bundles', () => {
    expect(tauriConfig.identifier).toBe('dev.plume.app');
    expect(tauriConfig.bundle.targets).toEqual(['app', 'dmg']);
    expect(tauriConfig.bundle.macOS?.signingIdentity).toBe('-');
  });

  it('distinguishes ambient project context from pinned exact sources in the demo', () => {
    expect(demoScript).toContain('bounded ambient context');
    expect(demoScript).toContain('pinned exact context');
    expect(demoScript).not.toMatch(/only reaches the model when I deliberately add it/i);
  });
});
