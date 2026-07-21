import { createHash } from 'node:crypto';
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

import { describe, expect, it } from 'vitest';

const packageJson = JSON.parse(
  readFileSync(join(process.cwd(), 'package.json'), 'utf8'),
) as { version: string; license?: string };
const packageLock = JSON.parse(
  readFileSync(join(process.cwd(), 'package-lock.json'), 'utf8'),
) as { version: string; packages: { '': { version: string; license?: string } }; license?: string };
const tauriConfig = JSON.parse(
  readFileSync(join(process.cwd(), 'src-tauri/tauri.conf.json'), 'utf8'),
) as {
  version: string;
  identifier: string;
  bundle: {
    targets: string[];
    icon?: string[];
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
const readme = readFileSync(join(process.cwd(), 'README.md'), 'utf8');

function requiredVersion(source: string, pattern: RegExp, label: string): string {
  const match = source.match(pattern);
  if (!match?.[1]) {
    throw new Error(`missing ${label} version`);
  }
  return match[1];
}

function pngDimensions(path: string): [number, number] {
  const png = readFileSync(join(process.cwd(), path));
  expect(png.subarray(1, 4).toString('ascii')).toBe('PNG');
  return [png.readUInt32BE(16), png.readUInt32BE(20)];
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

  it('packages the generated Plume icon set from the canonical artwork', () => {
    const canonical = readFileSync(join(process.cwd(), 'src-tauri/icons/Plume_Icon.png'));
    expect(pngDimensions('src-tauri/icons/Plume_Icon.png')).toEqual([2048, 2048]);
    expect(createHash('sha256').update(canonical).digest('hex')).toBe(
      'b68a4f25a9e5774c1910be0963c2a19a36e80ac4ea6c71ee21aec8959a0ae932',
    );
    expect(tauriConfig.bundle.icon).toEqual([
      'icons/32x32.png',
      'icons/128x128.png',
      'icons/128x128@2x.png',
      'icons/icon.icns',
      'icons/icon.ico',
    ]);
    expect(pngDimensions('src-tauri/icons/32x32.png')).toEqual([32, 32]);
    expect(pngDimensions('src-tauri/icons/128x128.png')).toEqual([128, 128]);
    expect(pngDimensions('src-tauri/icons/128x128@2x.png')).toEqual([256, 256]);
  });

  it('distinguishes ambient project context from pinned exact sources in the demo', () => {
    expect(demoScript).toContain('bounded ambient context');
    expect(demoScript).toContain('pinned exact context');
    expect(demoScript).not.toMatch(/only reaches the model when I deliberately add it/i);
  });

  it('publishes the project consistently under the MIT license', () => {
    const licensePath = join(process.cwd(), 'LICENSE');
    expect(existsSync(licensePath)).toBe(true);
    if (!existsSync(licensePath)) return;

    const license = readFileSync(licensePath, 'utf8');
    expect(packageJson.license).toBe('MIT');
    expect(packageLock.packages[''].license).toBe('MIT');
    expect(cargoToml).toMatch(/^license = "MIT"$/m);
    expect(license).toMatch(/^MIT License$/m);
    expect(license).toContain('Copyright (c) 2026 Plume contributors');
    expect(readme).toContain('[MIT License](LICENSE)');
    expect(readme).not.toMatch(/all rights reserved/i);
  });
});
