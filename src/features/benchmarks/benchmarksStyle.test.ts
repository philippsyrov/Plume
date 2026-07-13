import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

// D132 stylesheet contract (Codex packaged visual review): the
// benchmark viewer must speak the D62-D64 unified-workspace language,
// not the legacy ink-panel scaffold. Layout lives in CSS that
// happy-dom doesn't load, so (like windowFill.test.ts and the
// D61/D87/D98 layout contracts) we assert against the stylesheet and
// component source directly.
const read = (rel: string) => readFileSync(join(process.cwd(), rel), 'utf8');
const benchmarksCss = read('src/styles/layout/benchmarks.css');
const panelSource = read('src/features/benchmarks/BenchmarksPanel.tsx');

/** The body of the first `selector { … }` block in `css`. */
function blockOf(css: string, selector: string): string {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const match = css.match(new RegExp(`${escaped}\\s*\\{([^}]*)\\}`, 's'));
  if (!match?.[1]) throw new Error(`no rule block found for ${selector}`);
  return match[1];
}

describe('Benchmarks viewer stylesheet contract (D132)', () => {
  it('never applies the legacy ink-panel frame to the workspace surface', () => {
    // The heavy black ink frame belongs to the pre-D62 hero views;
    // inside the white/shadow shell it reads as an old scaffold.
    expect(panelSource).not.toContain('ink-panel');
  });

  it('renders sections as chrome cards, not border-top scaffold rows', () => {
    const section = blockOf(benchmarksCss, '.plume-benchmarks-section');
    expect(section).toMatch(/background:\s*var\(--plume-chrome-fill\)/);
    expect(section).toMatch(/border:\s*1px solid var\(--plume-chrome-line\)/);
    expect(section).toMatch(/border-radius:\s*var\(--plume-chrome-radius-panel\)/);
  });

  it('styles the page heading in the prose face like the settings/tool-drawer headers', () => {
    const heading = blockOf(benchmarksCss, '.plume-benchmarks-header h2');
    expect(heading).toMatch(/font-family:\s*var\(--font-prose\)/);
    expect(heading).toMatch(/font-size:\s*18px/);
    expect(heading).toMatch(/font-weight:\s*500/);
  });

  it('gives tables deliberate typography and chrome hairlines instead of browser defaults', () => {
    const table = blockOf(benchmarksCss, '.plume-benchmarks-table');
    expect(table).toMatch(/font-family:\s*var\(--font-ui\)/);
    expect(table).toMatch(/font-size:\s*12px/);
    const cells = blockOf(benchmarksCss, '.plume-benchmarks-table th,\n.plume-benchmarks-table td');
    expect(cells).toMatch(/border-bottom:\s*1px solid var\(--plume-chrome-line\)/);
    expect(cells).toMatch(/padding:\s*var\(--space-2\)/);
    const headers = blockOf(benchmarksCss, '.plume-benchmarks-table th');
    expect(headers).toMatch(/border-bottom:\s*1px solid var\(--plume-chrome-line-strong\)/);
  });

  it('styles controls with the shared chrome control shadow', () => {
    const button = blockOf(benchmarksCss, '.plume-benchmarks .ink-button');
    expect(button).toMatch(/box-shadow:\s*var\(--plume-chrome-control-shadow\)/);
    expect(button).toMatch(/border-radius:\s*var\(--plume-chrome-radius-control\)/);
  });

  it('keeps the evidence preview a chrome floating surface like the settings window', () => {
    const preview = blockOf(benchmarksCss, '.plume-benchmarks-preview');
    expect(preview).toMatch(/border-radius:\s*var\(--plume-chrome-radius-window\)/);
    expect(preview).toMatch(/box-shadow:\s*var\(--plume-chrome-shadow-panel\)/);
    expect(preview).toMatch(/background:\s*var\(--plume-chrome-fill\)/);
  });

  it('gives tables a horizontal scroll owner so constrained windows never clip columns', () => {
    const scroll = blockOf(benchmarksCss, '.plume-benchmarks-table-scroll');
    expect(scroll).toMatch(/overflow-x:\s*auto/);
    expect(scroll).toMatch(/min-width:\s*0/);
  });

  it('keeps the fake-data banner loud', () => {
    const banner = blockOf(benchmarksCss, '.plume-benchmarks-fake-banner');
    expect(banner).toMatch(/border:\s*1px solid var\(--warn\)/);
    expect(banner).toMatch(/font-weight:\s*600/);
  });
});

describe('Packaged-app binary selection contract (D132)', () => {
  // The crate ships two binaries (plume + plume_bench); an ambiguous
  // bundler selection once packaged plume_bench as Plume.app's
  // executable and the app died on launch. The Rust-side pin lives in
  // src-tauri (manifest_tests); this mirrors it where the frontend
  // suite runs so a manifest edit cannot land through a JS-only PR
  // unnoticed, and pins the smoke script's own assertion.
  it('Cargo.toml pins default-run to the desktop binary', () => {
    const manifest = read('src-tauri/Cargo.toml');
    expect(manifest).toContain('default-run = "plume"');
  });

  it('smoke-app.sh refuses a bundle whose declared executable is not the desktop shell', () => {
    const smoke = read('scripts/smoke-app.sh');
    expect(smoke).toContain('CFBundleExecutable');
    expect(smoke).toMatch(/DECLARED_EXEC.*!=.*"plume"/s);
    expect(smoke).toContain('Contents/MacOS/plume" ]');
  });
});

describe('Packaged-app smoke isolation contract', () => {
  it('uses a smoke-only Tauri identity without changing the production identity', () => {
    const production = JSON.parse(read('src-tauri/tauri.conf.json')) as {
      productName: string;
      identifier: string;
    };
    const smoke = JSON.parse(read('src-tauri/tauri.smoke.conf.json')) as {
      productName: string;
      identifier: string;
    };

    expect(production).toMatchObject({
      productName: 'Plume',
      identifier: 'dev.plume.app',
    });
    expect(smoke).toEqual({
      productName: 'Plume Smoke',
      identifier: 'dev.plume.smoke',
    });
  });

  it('builds, validates, and launches only the isolated smoke bundle', () => {
    const smoke = read('scripts/smoke-app.sh');

    expect(smoke).toContain('--config src-tauri/tauri.smoke.conf.json');
    expect(smoke).toContain('bundle/macos/Plume Smoke.app');
    expect(smoke).toContain('CFBundleIdentifier');
    expect(smoke).toMatch(/DECLARED_BUNDLE_ID.*!=.*"dev\.plume\.smoke"/s);
    expect(smoke).toContain('Bundle id: dev.plume.smoke');
    expect(smoke).toContain('managed worktree or /private/tmp');
    expect(smoke).toContain('NEVER open Desktop-root projects');
  });

  it('documents state isolation without claiming stable TCC permissions', () => {
    const operability = read('docs/AGENT_OPERABILITY.md');
    const smokeTesting = read('docs/SMOKE_TESTING.md');

    for (const document of [operability, smokeTesting]) {
      expect(document).toContain('dev.plume.smoke');
      expect(document).toContain('does not stabilize TCC permission persistence');
      expect(document).toContain('Apple Development');
      expect(document).toContain('Full Disk Access');
      expect(document).toContain('/private/tmp');
    }
    expect(operability).toContain('allowlist `Plume Smoke` and target its window');
    expect(smokeTesting).not.toContain('/Users/philippsyrov/Desktop');
    expect(smokeTesting).not.toContain('TODO: step 1');
    expect(smokeTesting).toMatch(
      /\| 1 \| Open a project fixture in a managed worktree or `\/private\/tmp` \|/,
    );
  });
});
