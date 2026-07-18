import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

import { describe, expect, it } from 'vitest';

const root = process.cwd();
const read = (path: string): string => readFileSync(join(root, path), 'utf8');

describe('model runtime packaging', () => {
  it('pins the supported MLX stack and a hash-locked install', () => {
    expect(read('scripts/mlx-runtime-requirements.in')).toBe(
      'mlx-lm==0.31.3\nmlx==0.32.0\nmlx-metal==0.32.0\n',
    );

    const lock = read('scripts/mlx-runtime-requirements.lock');
    for (const requirement of [
      'mlx-lm==0.31.3',
      'mlx==0.32.0',
      'mlx-metal==0.32.0',
    ]) {
      expect(lock).toContain(requirement);
    }
    expect(lock).toContain('--hash=sha256:');
  });

  it('declares generated runtime resources without embedding model weights', () => {
    const config = JSON.parse(read('src-tauri/tauri.conf.json')) as {
      bundle: { resources?: Record<string, string> };
    };

    expect(config.bundle.resources).toEqual({
      'runtime/generated/mlx-runtime/': 'mlx-runtime/',
      'runtime/generated/apple-model/': 'apple-model/',
      'third-party/NOTICE.md': 'NOTICE.md',
    });
    expect(JSON.stringify(config.bundle.resources)).not.toMatch(
      /(?:safetensors|\.gguf|plume-models|models\/catalog)/i,
    );
  });

  it('creates empty generated resource roots before Tauri inspects the bundle config', () => {
    const buildScript = read('src-tauri/build.rs');
    const prepareCall = buildScript.indexOf('ensure_bundle_resource_dirs()');
    const tauriBuildCall = buildScript.indexOf('tauri_build::try_build');

    expect(prepareCall).toBeGreaterThan(-1);
    expect(prepareCall).toBeLessThan(tauriBuildCall);
    expect(buildScript).toContain('runtime/generated/mlx-runtime');
    expect(buildScript).toContain('runtime/generated/apple-model');
    expect(buildScript).toContain('create_dir_all');
  });

  it('keeps generated payloads ignored and preserves third-party notices', () => {
    expect(read('.gitignore')).toMatch(/^src-tauri\/runtime\/generated\/$/m);
    expect(existsSync(join(root, 'src-tauri/runtime/README.md'))).toBe(true);
    expect(read('src-tauri/third-party/NOTICE.md')).toContain('MLX-LM');
    expect(read('src-tauri/third-party/NOTICE.md')).toContain('Apple Foundation Models');
  });

  it('prepares both payloads only through explicit packaging commands', () => {
    const packageJson = JSON.parse(read('package.json')) as {
      scripts: Record<string, string>;
    };
    expect(packageJson.scripts['prepare:model-runtime']).toBe(
      './scripts/dev-env.sh ./scripts/prepare-model-runtime-bundle.sh',
    );

    const prepare = read('scripts/prepare-model-runtime-bundle.sh');
    expect(prepare).toContain('build-mlx-runtime.sh');
    expect(prepare).toContain('build-apple-model-helper.sh');
    expect(prepare).toContain('runtime-identity.json');

    const smoke = read('scripts/smoke-app.sh');
    expect(smoke.indexOf('npm run prepare:model-runtime')).toBeGreaterThan(-1);
    expect(smoke.indexOf('npm run prepare:model-runtime')).toBeLessThan(
      smoke.indexOf('npm run tauri -- build'),
    );
  });

  it('build scripts verify identity, architecture, and the absence of weights', () => {
    const mlx = read('scripts/build-mlx-runtime.sh');
    expect(mlx).toContain('EXPECTED_UV_VERSION="0.11.18"');
    expect(mlx).toContain('export UV_PYTHON_CPYTHON_BUILD="$EXPECTED_PYTHON_BUILD"');
    expect(mlx).toContain('uv python install 3.12.13');
    expect(mlx).toContain('--require-hashes');
    expect(mlx).toContain('--break-system-packages');
    expect(mlx).toContain('runtime-identity.json');
    expect(mlx).toContain('pythonBuild');
    expect(mlx).toContain('uvVersion');
    expect(mlx).toContain('pythonExecutableSha256');
    expect(mlx).toContain('mlx_lm');
    expect(mlx).toMatch(/sha256/i);
    expect(mlx).not.toContain('mlx.__version__');
    expect(mlx).toContain('PYTHONDONTWRITEBYTECODE=1');
    expect(mlx).toContain('rm -f "$OUTPUT/bin/python3"');
    expect(mlx).toContain('cp "$OUTPUT/bin/python3.12" "$OUTPUT/bin/python3"');
    expect(mlx).toContain('! -name python3.12');
    expect(mlx).toContain('@PLUME_RUNTIME_PREFIX@');
    expect(mlx).toContain('install_name_tool -id "@rpath/libpython3.12.dylib"');
    expect(mlx).toContain('codesign --force --sign -');

    const apple = read('scripts/build-apple-model-helper.sh');
    expect(apple).toContain('swift build');
    expect(apple).toContain('arm64');

    const prepare = read('scripts/prepare-model-runtime-bundle.sh');
    expect(prepare).toMatch(/safetensors|gguf/);
    expect(prepare).not.toContain('-name models');
    expect(prepare).toContain('[ -L "$PYTHON" ]');
    expect(prepare).toContain('EXPECTED_PYTHON_BUILD="20260510"');
    expect(prepare).toContain('pythonExecutableSha256');
    expect(prepare).toContain('PYTHONDONTWRITEBYTECODE=1');
    expect(prepare).toContain("-name '*.pyc'");
    expect(prepare).toContain('grep -R -a -F');
    expect(prepare).toContain('"$GENERATED/mlx-runtime"');
  });
});
