# Benchmark Harness (D129)

Implementation guide for the harness that
[`docs/MODEL_BENCHMARKS.md`](MODEL_BENCHMARKS.md) reserves. That
document is the binding evidence contract; this one records the D129
implementation decisions the contract delegates — exact flags, fixture
location, ignore rules, and tests. When the two disagree, the contract
wins.

## What D129 implements

- Schema-v1 record validation (`scripts/benchmark/validate.ts` +
  `evidence.ts`) in two modes: **producer** (the harness itself; unknown
  fields are errors) and **reader** (the summarizer; unknown fields are
  warnings, newer schema versions are refused for the whole input).
  The validator enforces the **contradiction rules** between duplicated
  common fields and `suiteEvidence`: `diffValid` /
  `applySucceeded` / `verifierSucceeded` must equal
  `outcome.validDiff` / `patchApplySuccess` / `verificationSuccess`;
  `taskSucceeded` must equal `outcome.finalTaskSuccess`;
  `cancellationLatencyMs`, `runtimeCrashed`, and
  `terminalStreamOutcome` must equal their `outcome` counterparts;
  `finalAssembledPromptTokens` must equal
  `tokens.finalAssembledPromptTokens`; `acceptedContextTokens` must
  equal `model.context.acceptedTokens`; `requestedContextTokens` must
  equal `model.context.configuredTokens` (**mapping decision**:
  "requested" is what the harness configured); `outcome.toolCallValid`
  must equal the conjunction of the recorded per-call verdicts;
  `outcome.correctFileDiscovery` must equal "no missing required, no
  claimed forbidden"; `outcome.restartRecovery` must equal
  `restartHealthy && followUpPassed` and may never be `true` unproven.
  Suite-scoped outcome metrics must be `null` for suites that cannot
  exercise them.
- The deterministic local fixture pack under `benchmarks/fixtures/`
  (one case per contract suite) and the scripted **fake runtime** under
  `benchmarks/fake-runtime/`.
- The three reserved commands (below), wired end-to-end against the
  fake runtime and covered by `scripts/benchmark/*.test.ts`.

## What D129 deliberately does not implement

Recorded here so nobody mistakes harness coverage for measurement
coverage:

- **No real runtime adapters.** The only shipped runtime is the fake.
  Real MLX-LM / Ollama / llama.cpp invocation lands with a future
  slice; nothing stops the config's `runtime.command` from pointing at
  a real local server client once one exists.
- **No `plumeOrchestration` path.** Measuring Plume's own overhead
  means driving the real app; the config loader rejects that
  measurement path rather than faking it.
- **No resource probes.** `resources.*` is `null` (the contract's
  "unsupported probe" value), never `0`.
- **`validDiff` is measured with `git apply --check`** (after a
  lexical path screen) inside a disposable fixture copy, not with
  Plume's Rust patch validator — wiring the Rust validator in would
  mean product-code changes this slice excludes. Until that lands,
  agent-suite results must not be published as Plume results. Records
  from the fake runtime never qualify for publication anyway.

## The fake runtime

`benchmarks/fake-runtime/fake-runtime.mjs` is a local Node subprocess
speaking line-delimited JSON on stdio — no ports, no network, no
model. A case script (`benchmarks/fake-runtime/cases/*.json`) scripts
its behavior byte-for-byte: reply tokens, tool calls, **reported**
timing/token numbers (`timing.method: "runtimeReported"`), and failure
modes (`malformed`, `crash`, `hang`, `cancellable`). `--health` is the
restart probe; `--follow-up` selects the post-restart behavior.

Records produced against it carry `engine: "plume-fake-runtime"` /
`backend: "scripted"`; the summarizer banners any output containing
such records as **HARNESS TEST DATA**. They exist to prove harness
mechanics, never model performance.

## Commands

```
scripts/benchmark-model.sh --config <config.json> --fixture <fixture-dir> \
  --out <records.jsonl> [--population warm|cold] [--repetition N] \
  [--planned N] [--run-id ID] [--group-id ID] [--pair-id ID] \
  [--timestamp RFC3339]

scripts/benchmark-suite.sh <plan.json>

npx --no-install vite-node scripts/summarize-benchmarks.ts -- <records.jsonl>...
```

All three are thin wrappers over `scripts/benchmark/*.ts`, run with
the repo's existing `vite-node` (no new dependencies). One
`benchmark-model.sh` run = one invocation = one JSONL record, appended
to `--out`. The record is producer-validated before writing; an
invalid record is a harness bug and the command fails instead of
writing it.

`--timestamp` and the `PLUME_BENCH_GIT_SHA` / `PLUME_BENCH_DIRTY`
environment overrides exist for deterministic tests; without them the
harness stamps the current time and the real `git rev-parse HEAD` /
dirty state.

### Plan format (`benchmark-suite.sh`)

```json
{
  "config": "benchmarks/plans/fake-config.json",
  "outFile": "benchmark-artifacts/fake-smoke.jsonl",
  "groups": [
    { "groupId": "grp_x", "fixture": "benchmarks/fixtures/short-chat/fact-001",
      "population": "warm", "repetitions": 3 }
  ]
}
```

Repetitions are bounded 3..30 (below three is incomplete evidence). A
**warm** group runs one unrecorded priming request first. A **cold**
group records `coldMethod: "processRestart"` — literally true here,
since every invocation is a fresh subprocess; real persistent-server
runtimes will need a genuine stop/start between cold attempts.
`benchmarks/plans/` holds a working example config and plan.

### Config format (`--config`)

The sanitized config carries the `runtime` identity block (including
the subprocess `command` array) and the full declared `model` block
(artifact identity, context, sampling) exactly as they will be
recorded. The harness never infers model identity — what you declare
is what is recorded, and the fake config's artifact digest is the real
sha256 of the case script it runs.

## Fixtures

`benchmarks/fixtures/<suite-id>/<case-id>/manifest.json` fixes the
prompt, oracle configuration, timeout, file list, and `contentDigest`
(sha256 over the listed files). The recorded `suite.fixtureDigest` is
the sha256 of the manifest bytes, so one value pins everything. Loads
recompute the content digest and refuse a drifted fixture. Fixture
content is synthetic only — the integrity test rejects network
references.

Suite-specific manifest fields: `expectedAnswer` (short-chat);
`paddingFile` / `requiredKeys` / `decoyKeys` (long-context-retrieval);
`rubric` with required/prohibited patterns (code-explanation);
`fixtureRoot` / `targetFile` / `verifier` (single-file-bug-fix, and
with `requiredPaths` / `forbiddenPaths` for multi-file-navigation);
plus `tools` / `toolCallLimit` (tool-calling-agent-loop);
`cancelAfterTokens` / `followUpPrompt` (cancellation-restart).

Verifiers are allowlisted: the named script must be one of the
manifest's own listed files and runs inside the disposable copy with a
minimal environment. Discovery is observed through `read_file` tool
calls; "claimed as the target" means appearing as a `+++ b/` path in
the proposed diff.

## Artifact hygiene

`benchmark-artifacts/` is gitignored; it is the only root the record
`artifacts` grammar accepts. The D129 runner writes no artifact files
(`artifacts: []`); the grammar and its validator are in place for when
real runs produce local logs.

## Tests

`scripts/benchmark/*.test.ts` (vitest, part of `npm run test`):
`validate.test.ts` (schema battery + the contradiction battery),
`harness.test.ts` (every suite end-to-end against the fake runtime,
including malformed / hang / cancel / crash-restart), `summarize.test.ts`
(grouping, populations, spread math, pairs, rendering posture),
`cli.test.ts` (the three reserved commands exactly as a user runs
them), `fixtures.test.ts` (digest integrity, suite coverage, no
network references).
