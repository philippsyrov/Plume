// D129: per-suite functional oracles. Each takes the fixture manifest
// plus the invocation result and produces the suiteEvidence object and
// the suite-scoped outcome verdicts. Pass/fail here is ONLY the
// fixture's functional criterion from docs/MODEL_BENCHMARKS.md —
// nothing in this file looks at speed.
//
// Diff mechanics (documented in docs/BENCHMARK_HARNESS.md): with a
// configured `plumeBench.binary` (D129C), proposed diffs are validated
// and applied through Plume's REAL Rust patch modules (`plume_bench
// patch-check` → validate_patch + apply_patch) inside a disposable
// fixture copy — the product's own verdict, path screening, and
// pre-image checks, no retelling. Without it (the deterministic fake
// path), the documented `git apply --check` / `git apply` mechanics
// remain, after a lexical path screen; those records must not be
// published as Plume results (fake-runtime records never qualify).

import { spawnSync } from 'node:child_process';

import { verifySidecarIdentity } from './model-identity.ts';
import type { PlumeIdentity } from './model-identity.ts';
import { cpSync, mkdtempSync, rmSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';

import type { FixtureManifest } from './fixtures.ts';
import type { InvocationResult, RecordedToolCall } from './runtime-client.ts';
import type {
  CodeExplanationEvidence,
  LongContextRetrievalEvidence,
  MultiFileNavigationEvidence,
  ShortChatEvidence,
  SingleFileBugFixEvidence,
  ToolCallingAgentLoopEvidence,
} from './types.ts';

export interface OracleVerdict {
  /// The suite's functional criterion.
  passed: boolean;
  evidence:
    | ShortChatEvidence
    | LongContextRetrievalEvidence
    | CodeExplanationEvidence
    | SingleFileBugFixEvidence
    | MultiFileNavigationEvidence
    | ToolCallingAgentLoopEvidence;
  outcome: {
    toolCallValid?: boolean | null;
    correctFileDiscovery?: boolean | null;
    validDiff?: boolean | null;
    patchApplySuccess?: boolean | null;
    verificationSuccess?: boolean | null;
  };
}

export function normalizeReply(reply: string): string {
  return reply.replace(/\s+/g, ' ').trim().toLowerCase();
}

const completedStream = (invocation: InvocationResult): 'completed' | 'malformed' | 'cancelled' | 'timedOut' | 'crashed' =>
  invocation.terminal;

// ---- short-chat ------------------------------------------------------------

export function judgeShortChat(manifest: FixtureManifest, invocation: InvocationResult): OracleVerdict {
  const expected = normalizeReply(manifest.expectedAnswer ?? '');
  let classification: string | null = null;
  let passed = false;
  if (invocation.terminal === 'completed') {
    const normalized = normalizeReply(invocation.reply);
    if (normalized.length === 0) classification = 'empty';
    else if (normalized === expected) {
      classification = 'exact-match';
      passed = true;
    } else classification = 'mismatch';
  }
  const evidence: ShortChatEvidence = {
    kind: 'short-chat',
    replyClassification: classification,
    terminalStreamOutcome: completedStream(invocation),
  };
  return { passed, evidence, outcome: {} };
}

// ---- long-context-retrieval -------------------------------------------------

export function judgeLongContextRetrieval(
  manifest: FixtureManifest,
  invocation: InvocationResult,
  contextInfo: { requested: number; accepted: number | null; finalAssembledPromptTokens: number | null; truncated: boolean | null },
): OracleVerdict {
  const retrieved: string[] = [];
  const missing: string[] = [];
  const decoysAsserted: string[] = [];
  if (invocation.terminal === 'completed') {
    const normalized = normalizeReply(invocation.reply);
    for (const key of manifest.requiredKeys ?? []) {
      if (normalized.includes(normalizeReply(key.value))) retrieved.push(key.id);
      else missing.push(key.id);
    }
    for (const decoy of manifest.decoyKeys ?? []) {
      if (normalized.includes(normalizeReply(decoy.value))) decoysAsserted.push(decoy.id);
    }
  } else {
    for (const key of manifest.requiredKeys ?? []) missing.push(key.id);
  }
  const passed = invocation.terminal === 'completed' && missing.length === 0 && decoysAsserted.length === 0;
  const evidence: LongContextRetrievalEvidence = {
    kind: 'long-context-retrieval',
    requestedContextTokens: contextInfo.requested,
    acceptedContextTokens: contextInfo.accepted,
    finalAssembledPromptTokens: contextInfo.finalAssembledPromptTokens,
    retrievedKeys: retrieved,
    missingKeys: missing,
    incorrectDecoyKeys: decoysAsserted,
    truncated: contextInfo.truncated,
  };
  return { passed, evidence, outcome: {} };
}

// ---- code-explanation --------------------------------------------------------

export function judgeCodeExplanation(manifest: FixtureManifest, invocation: InvocationResult): OracleVerdict {
  const items: Array<{ id: string; passed: boolean }> = [];
  let allPass = invocation.terminal === 'completed';
  for (const rule of manifest.rubric ?? []) {
    const matches = invocation.terminal === 'completed' && new RegExp(rule.pattern, 'i').test(invocation.reply);
    const rulePassed = rule.mode === 'required' ? matches : !matches;
    items.push({ id: rule.id, passed: rulePassed });
    if (!rulePassed) allPass = false;
  }
  const evidence: CodeExplanationEvidence = {
    kind: 'code-explanation',
    rubricItems: items,
    responseCharacters: invocation.terminal === 'completed' ? invocation.reply.length : null,
  };
  return { passed: allPass, evidence, outcome: {} };
}

// ---- diff mechanics (shared by the three agent suites) -----------------------

export interface DiffOutcome {
  diffValid: boolean | null;
  applySucceeded: boolean | null;
  verifierSucceeded: boolean | null;
  rollbackSucceeded: boolean | null;
  targetPaths: string[];
}

/// Extract the `b/`-side target paths from a unified diff.
export function diffTargetPaths(diff: string): string[] {
  const targets: string[] = [];
  for (const line of diff.split('\n')) {
    const match = /^\+\+\+ b\/(.+)$/.exec(line);
    if (match?.[1] !== undefined) targets.push(match[1].trim());
  }
  return targets;
}

function pathsAreClean(paths: string[]): boolean {
  return (
    paths.length > 0 &&
    paths.every(
      (p) =>
        !p.startsWith('/') &&
        !p.includes('\\') &&
        !p.includes('\0') &&
        p.split('/').every((c) => c !== '' && c !== '.' && c !== '..'),
    )
  );
}

/// Validate + apply a proposed diff against a disposable copy of the
/// fixture's repo subtree, then run the fixture's allowlisted verifier
/// inside the copy, then remove the copy. Every step's outcome is
/// recorded separately; a failed step leaves later steps null.
/// Diff-mechanics options. `patchCheck` is the Plume-validator bridge
/// command prefix (e.g. `[plume_bench, "patch-check"]`); when present,
/// validate + apply run through Plume's real Rust patch modules and
/// the lexical screen + git mechanics are NOT used at all — parity
/// means Plume's verdict, not an intersection of two validators.
/// `expectedIdentity` is the attempt's pinned Plume identity: the
/// bridge binary is re-verified against it IMMEDIATELY before each
/// spawn, so a rebuild between attempt start and judge time refuses
/// instead of producing verdicts from a different build.
export interface DiffMechanicsOptions {
  patchCheck?: string[];
  expectedIdentity?: PlumeIdentity;
}

export function exerciseDiff(
  fixtureDir: string,
  manifest: FixtureManifest,
  diff: string | null,
  options?: DiffMechanicsOptions,
): DiffOutcome {
  if (diff === null || diff.trim().length === 0) {
    return { diffValid: null, applySucceeded: null, verifierSucceeded: null, rollbackSucceeded: null, targetPaths: [] };
  }
  const targetPaths = diffTargetPaths(diff);
  const fixtureRoot = path.join(fixtureDir, manifest.fixtureRoot ?? 'repo');
  const copy = mkdtempSync(path.join(os.tmpdir(), 'plume-bench-fixture-'));
  let diffValid: boolean | null = null;
  let applySucceeded: boolean | null = null;
  let verifierSucceeded: boolean | null = null;
  let rollbackSucceeded: boolean | null = null;
  try {
    cpSync(fixtureRoot, copy, { recursive: true });

    const patchCheck = options?.patchCheck;
    if (patchCheck !== undefined) {
      // Plume parity: one bridge call runs the product's validate and
      // (only when valid) apply against the disposable copy. A broken
      // bridge is NOT evidence about the diff: both verdicts stay
      // null and the failure is logged.
      const bridgeBin = patchCheck[0];
      if (bridgeBin === undefined) throw new Error('patchCheck command must not be empty');
      if (options?.expectedIdentity === undefined) {
        throw new Error('patchCheck requires expectedIdentity — sidecar provenance must be pinned per attempt');
      }
      // Provenance, re-verified at the moment of use: the binary
      // about to produce Plume verdicts must still be the build this
      // attempt's identity was pinned to.
      verifySidecarIdentity(bridgeBin, options.expectedIdentity);
      const bridge = spawnSync(bridgeBin, patchCheck.slice(1), {
        input: JSON.stringify({ projectRoot: copy, diff, apply: true }),
        encoding: 'utf8',
        timeout: 60_000,
      });
      let verdict: { ok?: unknown; valid?: unknown; applied?: unknown } | null = null;
      try {
        verdict = JSON.parse(bridge.stdout ?? '') as { ok?: unknown; valid?: unknown; applied?: unknown };
      } catch {
        verdict = null;
      }
      if (bridge.status !== 0 || verdict === null || verdict.ok !== true) {
        console.error('plume patch-check bridge failed (recording null diff mechanics):', bridge.stderr ?? bridge.status);
      } else {
        diffValid = typeof verdict.valid === 'boolean' ? verdict.valid : null;
        applySucceeded = typeof verdict.applied === 'boolean' ? verdict.applied : null;
      }
    } else if (!pathsAreClean(targetPaths)) {
      diffValid = false;
    } else {
      const check = spawnSync('git', ['apply', '--check', '--whitespace=nowarn', '-'], {
        cwd: copy,
        input: diff,
        encoding: 'utf8',
      });
      diffValid = check.status === 0;
    }

    if (patchCheck === undefined && diffValid === true) {
      const apply = spawnSync('git', ['apply', '--whitespace=nowarn', '-'], {
        cwd: copy,
        input: diff,
        encoding: 'utf8',
      });
      applySucceeded = apply.status === 0;
    }

    if (applySucceeded) {
      const verifier = manifest.verifier;
      // Allowlist: the verifier must be a manifest-listed fixture file.
      if (verifier === undefined || !manifest.files.includes(path.posix.join(manifest.fixtureRoot ?? 'repo', verifier))) {
        verifierSucceeded = null;
      } else {
        const run = spawnSync('bash', [verifier], {
          cwd: copy,
          encoding: 'utf8',
          env: { PATH: process.env['PATH'] ?? '' },
          timeout: manifest.timeoutMs,
        });
        verifierSucceeded = run.status === 0;
      }
    }
  } finally {
    try {
      rmSync(copy, { recursive: true, force: true });
      rollbackSucceeded = true;
    } catch {
      rollbackSucceeded = false;
    }
  }
  return { diffValid, applySucceeded, verifierSucceeded, rollbackSucceeded, targetPaths };
}

// ---- single-file-bug-fix -------------------------------------------------------

export function judgeSingleFileBugFix(
  fixtureDir: string,
  manifest: FixtureManifest,
  invocation: InvocationResult,
  mechanics?: DiffMechanicsOptions,
): OracleVerdict {
  const diff = invocation.terminal === 'completed' ? invocation.reply : null;
  const result = exerciseDiff(fixtureDir, manifest, diff, mechanics);
  const passed = result.diffValid === true && result.applySucceeded === true && result.verifierSucceeded === true;
  const evidence: SingleFileBugFixEvidence = {
    kind: 'single-file-bug-fix',
    targetFile: manifest.targetFile ?? null,
    diffValid: result.diffValid,
    applySucceeded: result.applySucceeded,
    verifierSucceeded: result.verifierSucceeded,
    rollbackSucceeded: result.rollbackSucceeded,
  };
  return {
    passed,
    evidence,
    outcome: {
      validDiff: result.diffValid,
      patchApplySuccess: result.applySucceeded,
      verificationSuccess: result.verifierSucceeded,
    },
  };
}

// ---- multi-file-navigation -------------------------------------------------------

function discoveredFromToolCalls(toolCalls: RecordedToolCall[]): string[] {
  const paths: string[] = [];
  for (const call of toolCalls) {
    if (call.tool === 'read_file' && typeof call.args['path'] === 'string') {
      paths.push(call.args['path']);
    }
  }
  return paths;
}

export function judgeMultiFileNavigation(
  fixtureDir: string,
  manifest: FixtureManifest,
  invocation: InvocationResult,
  mechanics?: DiffMechanicsOptions,
): OracleVerdict {
  const discovered = discoveredFromToolCalls(invocation.toolCalls);
  const required = manifest.requiredPaths ?? [];
  const forbidden = manifest.forbiddenPaths ?? [];
  const missingRequired = required.filter((p) => !discovered.includes(p));
  const diff = invocation.terminal === 'completed' ? invocation.reply : null;
  const result = exerciseDiff(fixtureDir, manifest, diff, mechanics);
  // "Claimed as the target": a forbidden decoy appearing as a diff
  // target, not merely having been read.
  const claimedForbidden = forbidden.filter((p) => result.targetPaths.includes(p));
  const discoveryOk = missingRequired.length === 0 && claimedForbidden.length === 0;
  const passed =
    discoveryOk && result.diffValid === true && result.applySucceeded === true && result.verifierSucceeded === true;
  const evidence: MultiFileNavigationEvidence = {
    kind: 'multi-file-navigation',
    discoveredPaths: discovered,
    missingRequiredPaths: missingRequired,
    claimedForbiddenPaths: claimedForbidden,
    diffValid: result.diffValid,
    applySucceeded: result.applySucceeded,
    verifierSucceeded: result.verifierSucceeded,
  };
  return {
    passed,
    evidence,
    outcome: {
      correctFileDiscovery: discoveryOk,
      validDiff: result.diffValid,
      patchApplySuccess: result.applySucceeded,
      verificationSuccess: result.verifierSucceeded,
    },
  };
}

// ---- tool-calling-agent-loop -------------------------------------------------------

export function judgeToolCallingAgentLoop(
  fixtureDir: string,
  manifest: FixtureManifest,
  invocation: InvocationResult,
  mechanics?: DiffMechanicsOptions,
): OracleVerdict {
  const tools = manifest.tools ?? [];
  const limit = manifest.toolCallLimit ?? null;
  const calls = invocation.toolCalls.map((call, index) => {
    const spec = tools.find((t) => t.name === call.tool);
    const allowed = spec !== undefined && (limit === null || index < limit);
    const valid =
      spec !== undefined &&
      Object.keys(call.args).every((k) => spec.argKeys.includes(k)) &&
      Object.values(call.args).every((v) => typeof v === 'string');
    return { index, tool: call.tool, valid, allowed };
  });
  const allCallsOk = calls.every((c) => c.valid && c.allowed);
  const discovered = discoveredFromToolCalls(invocation.toolCalls);
  const required = manifest.requiredPaths ?? [];
  const forbidden = manifest.forbiddenPaths ?? [];

  // The proposed diff arrives via the propose_diff tool, not the reply.
  const proposeCall = invocation.toolCalls.filter((c) => c.tool === 'propose_diff').at(-1);
  const diff = typeof proposeCall?.args['diff'] === 'string' ? proposeCall.args['diff'] : null;
  const result = exerciseDiff(fixtureDir, manifest, invocation.terminal === 'completed' ? diff : null, mechanics);

  const discoveryOk =
    required.every((p) => discovered.includes(p)) && !forbidden.some((p) => result.targetPaths.includes(p));
  const passed =
    allCallsOk &&
    discoveryOk &&
    result.diffValid === true &&
    result.applySucceeded === true &&
    result.verifierSucceeded === true;
  const evidence: ToolCallingAgentLoopEvidence = {
    kind: 'tool-calling-agent-loop',
    toolCallLimit: limit,
    toolCalls: calls,
    discoveredPaths: discovered,
    diffValid: result.diffValid,
    applySucceeded: result.applySucceeded,
    verifierSucceeded: result.verifierSucceeded,
    taskSucceeded: passed,
  };
  return {
    passed,
    evidence,
    outcome: {
      toolCallValid: calls.length > 0 ? allCallsOk : null,
      correctFileDiscovery: discoveryOk,
      validDiff: result.diffValid,
      patchApplySuccess: result.applySucceeded,
      verificationSuccess: result.verifierSucceeded,
    },
  };
}
