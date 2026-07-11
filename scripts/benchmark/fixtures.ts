// D129: deterministic fixture loading. A fixture is one directory:
//
//   benchmarks/fixtures/<suite-id>/<case-id>/
//     manifest.json   — prompt, oracle config, files list, contentDigest
//     ...files        — synthetic content only (repo/, padding, verifier)
//
// The manifest's `contentDigest` pins the listed files; the recorded
// `suite.fixtureDigest` is the digest of the manifest bytes themselves,
// so one value pins prompt + oracle + files. Both are verified on
// every load — a drifted fixture refuses to run rather than producing
// evidence against unknown content.

import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import path from 'node:path';

import { SUITE_IDS } from './types.ts';
import type { SuiteId } from './types.ts';

export interface RubricRule {
  id: string;
  /// RegExp source, evaluated case-insensitively against the reply.
  pattern: string;
  /// 'required' must match; 'prohibited' must not.
  mode: 'required' | 'prohibited';
}

export interface PlantedKey {
  id: string;
  value: string;
}

export interface ToolSpec {
  name: string;
  /// Exact allowed argument keys; every arg must be a string value.
  argKeys: string[];
}

export interface FixtureManifest {
  suiteId: SuiteId;
  caseId: string;
  fixtureRevision: string;
  timeoutMs: number;
  prompt: string;
  files: string[];
  contentDigest: string;
  // Suite-specific oracle configuration (only the relevant ones set).
  expectedAnswer?: string;
  paddingFile?: string;
  requiredKeys?: PlantedKey[];
  decoyKeys?: PlantedKey[];
  rubric?: RubricRule[];
  targetFile?: string;
  fixtureRoot?: string;
  verifier?: string;
  requiredPaths?: string[];
  forbiddenPaths?: string[];
  tools?: ToolSpec[];
  toolCallLimit?: number;
  cancelAfterTokens?: number;
  followUpPrompt?: string;
  followUpExpectedAnswer?: string;
}

export interface LoadedFixture {
  dir: string;
  manifest: FixtureManifest;
  /// sha256:<hex> of the manifest bytes — recorded as suite.fixtureDigest.
  manifestDigest: string;
}

export function sha256Hex(data: string | Buffer): string {
  return createHash('sha256').update(data).digest('hex');
}

/// Digest the fixture's listed content files: sha256 over
/// `<path>\n<content bytes>` in the manifest's file order.
export function computeContentDigest(dir: string, files: string[]): string {
  const hash = createHash('sha256');
  for (const rel of files) {
    assertCleanRelPath(rel);
    hash.update(`${rel}\n`);
    hash.update(readFileSync(path.join(dir, rel)));
  }
  return `sha256:${hash.digest('hex')}`;
}

function assertCleanRelPath(rel: string): void {
  if (
    rel.length === 0 ||
    rel.startsWith('/') ||
    rel.includes('\\') ||
    rel.includes('\0') ||
    rel.split('/').some((c) => c === '' || c === '.' || c === '..')
  ) {
    throw new Error(`fixture file path ${JSON.stringify(rel)} is not a clean relative path`);
  }
}

export function loadFixture(dir: string): LoadedFixture {
  const manifestPath = path.join(dir, 'manifest.json');
  const bytes = readFileSync(manifestPath);
  const parsed: unknown = JSON.parse(bytes.toString('utf8'));
  if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed)) {
    throw new Error(`${manifestPath}: manifest must be a JSON object`);
  }
  const manifest = parsed as FixtureManifest;
  if (!(SUITE_IDS as readonly string[]).includes(manifest.suiteId)) {
    throw new Error(`${manifestPath}: unknown suiteId ${JSON.stringify(manifest.suiteId)}`);
  }
  for (const field of ['caseId', 'fixtureRevision', 'prompt', 'contentDigest'] as const) {
    if (typeof manifest[field] !== 'string' || manifest[field].length === 0) {
      throw new Error(`${manifestPath}: ${field} must be a non-empty string`);
    }
  }
  if (!Number.isInteger(manifest.timeoutMs) || manifest.timeoutMs <= 0) {
    throw new Error(`${manifestPath}: timeoutMs must be a positive integer`);
  }
  if (!Array.isArray(manifest.files) || manifest.files.some((f) => typeof f !== 'string')) {
    throw new Error(`${manifestPath}: files must be an array of strings`);
  }

  const actual = computeContentDigest(dir, manifest.files);
  if (actual !== manifest.contentDigest) {
    throw new Error(
      `${manifestPath}: contentDigest mismatch — manifest says ${manifest.contentDigest}, files hash to ${actual}. ` +
        'Fixture content drifted; refusing to run against unknown content.',
    );
  }

  return { dir, manifest, manifestDigest: `sha256:${sha256Hex(bytes)}` };
}
