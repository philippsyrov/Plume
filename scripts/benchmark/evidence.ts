// D129: suiteEvidence validation — the closed per-suite shapes from
// docs/MODEL_BENCHMARKS.md § "Suite evidence extension", plus the
// contradiction rules between evidence fields and the common fields
// they duplicate. A record that carries, say, `outcome.validDiff:
// true` and `suiteEvidence.diffValid: false` is invalid, full stop —
// the validator is where the D128 review nit ("no stated equality
// invariant") is enforced.

import {
  EVIDENCE_FIELDS,
  MAX_EVIDENCE_ARRAY_ITEMS,
  MAX_EVIDENCE_STRING_CHARS,
  STREAM_OUTCOMES,
  SUITE_IDS,
} from './types.ts';
import type { SuiteId } from './types.ts';

type Obj = Record<string, unknown>;

const isObj = (v: unknown): v is Obj =>
  typeof v === 'object' && v !== null && !Array.isArray(v);
const isBool = (v: unknown): v is boolean => typeof v === 'boolean';
const isNonNegInt = (v: unknown): v is number =>
  typeof v === 'number' && Number.isInteger(v) && v >= 0;
const isNonNegFinite = (v: unknown): v is number =>
  typeof v === 'number' && Number.isFinite(v) && v >= 0;
const isPrintableAscii = (v: string): boolean => /^[\x20-\x7E]*$/.test(v);

/// Which suites may carry a non-null value for each duplicated or
/// suite-scoped common `outcome` metric. Everything else must be
/// `null` there — the contract's "null when the fixture cannot
/// exercise it" made checkable.
export const OUTCOME_METRIC_SUITES: Record<string, readonly SuiteId[]> = {
  toolCallValid: ['tool-calling-agent-loop'],
  correctFileDiscovery: ['multi-file-navigation', 'tool-calling-agent-loop'],
  validDiff: ['single-file-bug-fix', 'multi-file-navigation', 'tool-calling-agent-loop'],
  patchApplySuccess: ['single-file-bug-fix', 'multi-file-navigation', 'tool-calling-agent-loop'],
  verificationSuccess: ['single-file-bug-fix', 'multi-file-navigation', 'tool-calling-agent-loop'],
  cancellationLatencyMs: ['cancellation-restart'],
  restartRecovery: ['cancellation-restart'],
};

interface EvidenceContext {
  suiteId: unknown;
  outcome: unknown;
  tokens: unknown;
  modelContext: unknown;
}

/// Validate `suiteEvidence` structure and its equality invariants
/// against the rest of the record. Returns error strings ("" means
/// none); structural failures suppress the contradiction checks that
/// depend on the broken part.
export function validateSuiteEvidence(evidence: unknown, ctx: EvidenceContext): string[] {
  const errors: string[] = [];
  if (!isObj(evidence)) {
    return ['suiteEvidence: must be an object'];
  }
  const kind = evidence['kind'];
  if (typeof kind !== 'string' || !(SUITE_IDS as readonly string[]).includes(kind)) {
    return [`suiteEvidence.kind: must be one of ${SUITE_IDS.join(', ')}`];
  }
  if (ctx.suiteId !== kind) {
    errors.push(
      `suiteEvidence.kind: must equal suite.id (kind "${kind}" vs suite.id ${JSON.stringify(ctx.suiteId)})`,
    );
  }

  // Exactly the documented fields for this kind — no more, no fewer.
  const allowed = EVIDENCE_FIELDS[kind as SuiteId];
  for (const key of Object.keys(evidence)) {
    if (!allowed.includes(key)) {
      errors.push(`suiteEvidence.${key}: not a documented field for kind "${kind}"`);
    }
  }
  for (const key of allowed) {
    if (!(key in evidence)) {
      errors.push(`suiteEvidence.${key}: required for kind "${kind}" and missing`);
    }
  }
  if (errors.length > 0) {
    return errors;
  }

  errors.push(...validateShape(kind as SuiteId, evidence));
  if (errors.length > 0) {
    return errors;
  }
  errors.push(...validateContradictions(kind as SuiteId, evidence, ctx));
  return errors;
}

// ---- Per-kind field shapes ------------------------------------------------

function checkStringArray(errors: string[], evidence: Obj, field: string): void {
  const value = evidence[field];
  if (!Array.isArray(value)) {
    errors.push(`suiteEvidence.${field}: must be an array`);
    return;
  }
  if (value.length > MAX_EVIDENCE_ARRAY_ITEMS) {
    errors.push(`suiteEvidence.${field}: at most ${MAX_EVIDENCE_ARRAY_ITEMS} items`);
  }
  value.forEach((item, i) => {
    if (typeof item !== 'string' || item.length === 0 || item.length > MAX_EVIDENCE_STRING_CHARS) {
      errors.push(`suiteEvidence.${field}[${i}]: must be a 1..${MAX_EVIDENCE_STRING_CHARS} char string`);
    } else if (!isPrintableAscii(item)) {
      errors.push(`suiteEvidence.${field}[${i}]: must be printable ASCII`);
    } else if (item.startsWith('/') || item.includes('\\') || item.split('/').some((c) => c === '..' || c === '.')) {
      errors.push(`suiteEvidence.${field}[${i}]: must be a clean repository-relative path`);
    }
  });
}

function checkNullableBool(errors: string[], evidence: Obj, field: string): void {
  const value = evidence[field];
  if (value !== null && !isBool(value)) {
    errors.push(`suiteEvidence.${field}: must be boolean or null`);
  }
}

function checkNullableNonNegInt(errors: string[], evidence: Obj, field: string): void {
  const value = evidence[field];
  if (value !== null && !isNonNegInt(value)) {
    errors.push(`suiteEvidence.${field}: must be a finite non-negative integer or null`);
  }
}

function checkNullableStreamOutcome(errors: string[], evidence: Obj, field: string): void {
  const value = evidence[field];
  if (value !== null && (typeof value !== 'string' || !(STREAM_OUTCOMES as readonly string[]).includes(value))) {
    errors.push(`suiteEvidence.${field}: must be one of ${STREAM_OUTCOMES.join(', ')} or null`);
  }
}

function checkNullableBoundedString(errors: string[], evidence: Obj, field: string): void {
  const value = evidence[field];
  if (value === null) return;
  if (typeof value !== 'string' || value.length === 0 || value.length > MAX_EVIDENCE_STRING_CHARS) {
    errors.push(`suiteEvidence.${field}: must be a 1..${MAX_EVIDENCE_STRING_CHARS} char string or null`);
  } else if (!isPrintableAscii(value)) {
    errors.push(`suiteEvidence.${field}: must be printable ASCII`);
  }
}

function validateShape(kind: SuiteId, evidence: Obj): string[] {
  const errors: string[] = [];
  switch (kind) {
    case 'short-chat':
      checkNullableBoundedString(errors, evidence, 'replyClassification');
      checkNullableStreamOutcome(errors, evidence, 'terminalStreamOutcome');
      break;
    case 'long-context-retrieval':
      checkNullableNonNegInt(errors, evidence, 'requestedContextTokens');
      checkNullableNonNegInt(errors, evidence, 'acceptedContextTokens');
      checkNullableNonNegInt(errors, evidence, 'finalAssembledPromptTokens');
      checkStringArray(errors, evidence, 'retrievedKeys');
      checkStringArray(errors, evidence, 'missingKeys');
      checkStringArray(errors, evidence, 'incorrectDecoyKeys');
      checkNullableBool(errors, evidence, 'truncated');
      break;
    case 'code-explanation': {
      const items = evidence['rubricItems'];
      if (!Array.isArray(items)) {
        errors.push('suiteEvidence.rubricItems: must be an array');
      } else {
        if (items.length > MAX_EVIDENCE_ARRAY_ITEMS) {
          errors.push(`suiteEvidence.rubricItems: at most ${MAX_EVIDENCE_ARRAY_ITEMS} items`);
        }
        items.forEach((item, i) => {
          if (
            !isObj(item) ||
            Object.keys(item).length !== 2 ||
            typeof item['id'] !== 'string' ||
            item['id'].length === 0 ||
            item['id'].length > MAX_EVIDENCE_STRING_CHARS ||
            !isPrintableAscii(item['id']) ||
            !isBool(item['passed'])
          ) {
            errors.push(`suiteEvidence.rubricItems[${i}]: must be { id: ascii string, passed: boolean }`);
          }
        });
      }
      checkNullableNonNegInt(errors, evidence, 'responseCharacters');
      break;
    }
    case 'single-file-bug-fix':
      checkNullableBoundedString(errors, evidence, 'targetFile');
      checkNullableBool(errors, evidence, 'diffValid');
      checkNullableBool(errors, evidence, 'applySucceeded');
      checkNullableBool(errors, evidence, 'verifierSucceeded');
      checkNullableBool(errors, evidence, 'rollbackSucceeded');
      break;
    case 'multi-file-navigation':
      checkStringArray(errors, evidence, 'discoveredPaths');
      checkStringArray(errors, evidence, 'missingRequiredPaths');
      checkStringArray(errors, evidence, 'claimedForbiddenPaths');
      checkNullableBool(errors, evidence, 'diffValid');
      checkNullableBool(errors, evidence, 'applySucceeded');
      checkNullableBool(errors, evidence, 'verifierSucceeded');
      break;
    case 'tool-calling-agent-loop': {
      checkNullableNonNegInt(errors, evidence, 'toolCallLimit');
      const calls = evidence['toolCalls'];
      if (!Array.isArray(calls)) {
        errors.push('suiteEvidence.toolCalls: must be an array');
      } else {
        if (calls.length > MAX_EVIDENCE_ARRAY_ITEMS) {
          errors.push(`suiteEvidence.toolCalls: at most ${MAX_EVIDENCE_ARRAY_ITEMS} items`);
        }
        calls.forEach((call, i) => {
          if (
            !isObj(call) ||
            Object.keys(call).length !== 4 ||
            !isNonNegInt(call['index']) ||
            typeof call['tool'] !== 'string' ||
            call['tool'].length === 0 ||
            call['tool'].length > MAX_EVIDENCE_STRING_CHARS ||
            !isPrintableAscii(call['tool']) ||
            !isBool(call['valid']) ||
            !isBool(call['allowed'])
          ) {
            errors.push(
              `suiteEvidence.toolCalls[${i}]: must be { index: int, tool: ascii string, valid: boolean, allowed: boolean }`,
            );
          }
        });
      }
      checkStringArray(errors, evidence, 'discoveredPaths');
      checkNullableBool(errors, evidence, 'diffValid');
      checkNullableBool(errors, evidence, 'applySucceeded');
      checkNullableBool(errors, evidence, 'verifierSucceeded');
      checkNullableBool(errors, evidence, 'taskSucceeded');
      break;
    }
    case 'cancellation-restart': {
      const latency = evidence['cancellationLatencyMs'];
      if (latency !== null && !isNonNegFinite(latency)) {
        errors.push('suiteEvidence.cancellationLatencyMs: must be a finite non-negative number or null');
      }
      checkNullableStreamOutcome(errors, evidence, 'terminalStreamOutcome');
      checkNullableBool(errors, evidence, 'runtimeCrashed');
      checkNullableBool(errors, evidence, 'restartHealthy');
      checkNullableBool(errors, evidence, 'followUpPassed');
      break;
    }
  }
  return errors;
}

// ---- Contradiction rules ---------------------------------------------------

function mirror(
  errors: string[],
  evidence: Obj,
  outcome: Obj,
  evidenceField: string,
  outcomeField: string,
): void {
  const ev = evidence[evidenceField];
  const oc = outcome[outcomeField];
  if (ev !== oc) {
    errors.push(
      `suiteEvidence.${evidenceField}: contradicts outcome.${outcomeField} (${JSON.stringify(ev)} vs ${JSON.stringify(oc)})`,
    );
  }
}

function validateContradictions(kind: SuiteId, evidence: Obj, ctx: EvidenceContext): string[] {
  const errors: string[] = [];
  const outcome = isObj(ctx.outcome) ? ctx.outcome : null;
  const tokens = isObj(ctx.tokens) ? ctx.tokens : null;
  const modelContext = isObj(ctx.modelContext) ? ctx.modelContext : null;

  if (outcome !== null) {
    if (kind === 'single-file-bug-fix' || kind === 'multi-file-navigation' || kind === 'tool-calling-agent-loop') {
      mirror(errors, evidence, outcome, 'diffValid', 'validDiff');
      mirror(errors, evidence, outcome, 'applySucceeded', 'patchApplySuccess');
      mirror(errors, evidence, outcome, 'verifierSucceeded', 'verificationSuccess');
    }
    if (kind === 'tool-calling-agent-loop') {
      mirror(errors, evidence, outcome, 'taskSucceeded', 'finalTaskSuccess');
      const calls = evidence['toolCalls'];
      const claimed = outcome['toolCallValid'];
      if (Array.isArray(calls)) {
        const derived = calls.every((c) => isObj(c) && c['valid'] === true && c['allowed'] === true);
        if (calls.length > 0 && claimed === null) {
          errors.push('outcome.toolCallValid: null although suiteEvidence.toolCalls records attempted calls');
        } else if (claimed !== null && claimed !== derived) {
          errors.push(
            `outcome.toolCallValid: contradicts suiteEvidence.toolCalls (${JSON.stringify(claimed)} vs derived ${derived})`,
          );
        }
      }
    }
    if (kind === 'multi-file-navigation') {
      const missing = evidence['missingRequiredPaths'];
      const forbidden = evidence['claimedForbiddenPaths'];
      const claimed = outcome['correctFileDiscovery'];
      if (Array.isArray(missing) && Array.isArray(forbidden) && claimed !== null) {
        const derived = missing.length === 0 && forbidden.length === 0;
        if (claimed !== derived) {
          errors.push(
            `outcome.correctFileDiscovery: contradicts suiteEvidence path verdicts (${JSON.stringify(claimed)} vs derived ${derived})`,
          );
        }
      }
    }
    if (kind === 'short-chat' || kind === 'cancellation-restart') {
      mirror(errors, evidence, outcome, 'terminalStreamOutcome', 'stream');
    }
    if (kind === 'cancellation-restart') {
      mirror(errors, evidence, outcome, 'cancellationLatencyMs', 'cancellationLatencyMs');
      mirror(errors, evidence, outcome, 'runtimeCrashed', 'crash');
      const healthy = evidence['restartHealthy'];
      const followUp = evidence['followUpPassed'];
      const recovery = outcome['restartRecovery'];
      if (isBool(healthy) && isBool(followUp)) {
        const derived = healthy && followUp;
        if (recovery !== derived) {
          errors.push(
            `outcome.restartRecovery: contradicts suiteEvidence restart fields (${JSON.stringify(recovery)} vs derived ${derived})`,
          );
        }
      } else if (recovery === true) {
        // "true only after a post-crash restart reaches health and
        // passes the follow-up" — unproven recovery cannot be claimed.
        errors.push('outcome.restartRecovery: true although suiteEvidence restart fields do not prove recovery');
      }
    }
  }

  if (kind === 'long-context-retrieval') {
    if (tokens !== null) {
      const ev = evidence['finalAssembledPromptTokens'];
      const tk = tokens['finalAssembledPromptTokens'];
      if (ev !== tk) {
        errors.push(
          `suiteEvidence.finalAssembledPromptTokens: contradicts tokens.finalAssembledPromptTokens (${JSON.stringify(ev)} vs ${JSON.stringify(tk)})`,
        );
      }
    }
    if (modelContext !== null) {
      const accepted = evidence['acceptedContextTokens'];
      const ctxAccepted = modelContext['acceptedTokens'];
      if (accepted !== ctxAccepted) {
        errors.push(
          `suiteEvidence.acceptedContextTokens: contradicts model.context.acceptedTokens (${JSON.stringify(accepted)} vs ${JSON.stringify(ctxAccepted)})`,
        );
      }
      // Mapping decision (documented in docs/BENCHMARK_HARNESS.md):
      // "requested" context is what the harness configured, i.e.
      // model.context.configuredTokens.
      const requested = evidence['requestedContextTokens'];
      const configured = modelContext['configuredTokens'];
      if (requested !== configured) {
        errors.push(
          `suiteEvidence.requestedContextTokens: contradicts model.context.configuredTokens (${JSON.stringify(requested)} vs ${JSON.stringify(configured)})`,
        );
      }
    }
  }

  // Suite-scoped outcome metrics must be null wherever the fixture
  // cannot exercise them.
  if (outcome !== null) {
    for (const [metric, suites] of Object.entries(OUTCOME_METRIC_SUITES)) {
      if (!suites.includes(kind) && metric in outcome && outcome[metric] !== null) {
        errors.push(`outcome.${metric}: must be null for suite "${kind}" (fixture cannot exercise it)`);
      }
    }
  }

  return errors;
}
