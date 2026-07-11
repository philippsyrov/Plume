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

## What is deliberately not implemented yet

Recorded here so nobody mistakes harness coverage for measurement
coverage:

- **No Ollama / llama.cpp adapters.** D129A ships the MLX-LM adapter
  (below); other runtime rows remain future slices.
- **No `plumeOrchestration` path** (reserved for D129C). Measuring
  Plume's own overhead means driving the real app; the config loader
  rejects that measurement path rather than faking it.
- **No resource probes** (reserved for D129B). `resources.*` is
  `null` (the contract's "unsupported probe" value), never `0`.
- **`validDiff` is measured with `git apply --check`** (after a
  lexical path screen) inside a disposable fixture copy, not with
  Plume's Rust patch validator — wiring the Rust validator in is
  reserved for D129C. Until that lands, agent-suite results must not
  be published as Plume results. Records from the fake runtime never
  qualify for publication anyway.

## The MLX-LM adapter (D129A)

`transport: "openai-sse"` selects the real runtime adapter
(`scripts/benchmark/mlx-runtime.ts` + `runtime-factory.ts`). It owns
one `python -m mlx_lm server --model <dir> --host 127.0.0.1 --port
<ephemeral>` process per session, exactly like Plume's supervisor:
spawn, poll `GET /health` (startup budget in the config), serve,
SIGINT with a 3 s grace, then SIGKILL. Warm/cold semantics carry over
unchanged: a warm group is one primed live server; a cold attempt is
a fresh server per invocation (`processRestart` — the model load
happens before the request, so timings still start at request send).

**Verified identity, or no run.** Before any session starts, the
factory re-digests the model directory (sha256 over every file;
symlinks refused) and requires it to equal the declared
`model.artifact.sha256`; it probes `mlx_lm.__version__` through the
configured interpreter and requires it to match a declared version
(or fills a null one). A mismatch refuses the run — records never
carry an unverified identity.

**Client-observed timing** (`timing.method: "clientObserved"`,
monotonic): `timeToFirstTokenMs` = request write → first non-empty
content delta; `generationDurationMs` = first content delta →
terminal `[DONE]`; `endToEndMs` = request write → terminal;
`promptEvaluationMs` is not client-observable and stays null (so the
prompt rate stays null too). Token counts come only from the server's
reported `usage` (`stream_options.include_usage`); SSE deltas are
never counted as tokens. Deliberate cancellation aborts the HTTP
stream; latency runs from abort to conclusive close.

**Smoke matrix**: `scripts/benchmark-mlx-smoke.sh` discovers the
interpreter (`PLUME_MLX_PYTHON` → `~/.venvs/mlx-env/bin/python` →
`python3`) and a local checkpoint (`PLUME_MODEL_DIR` →
`<repo>/plume-models` → `~/plume-models`), builds a verified config
(quantization read from the checkpoint's own `config.json`), runs a
3-warm + 3-cold short-chat matrix, and prints the summary. Mechanics
validation only — single machine, tiny counts, results stay in
gitignored `benchmark-artifacts/`, never a performance claim. The
adapter's protocol handling is CI-tested against a scripted local
fake SSE server (`mlx-runtime.test.ts`); nothing in `npm run test`
needs mlx-lm or a model.

## The fake runtime

`benchmarks/fake-runtime/fake-runtime.mjs` is a local Node subprocess
speaking line-delimited JSON on stdio — no ports, no network, no
model. The process is a **session**: after a completed or cancelled
request it stays alive and serves the next generate on the same stdin.
A case script (`benchmarks/fake-runtime/cases/*.json`) scripts its
behavior byte-for-byte: reply tokens (optionally varied per request
index via `replyByRequest`, which is how tests prove population
honesty), tool calls, **reported** timing/token numbers
(`timing.method: "runtimeReported"`), and failure modes (`malformed`,
`crash`, `hang`, `cancellable` — these end the process; that is the
behavior they script). `--health` is the restart probe; `--follow-up`
selects the post-restart behavior.

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

Repetitions are bounded 3..30 (below three is incomplete evidence).

**Population honesty.** A **warm** group is one live runtime session:
the process is spawned once, primed with one unrecorded request, and
every measured repetition runs in that same loaded process. A
standalone warm `benchmark-model.sh` run does the same in miniature
(spawn, prime, measure, close). A **cold** attempt is a fresh
subprocess per invocation (`coldMethod: "processRestart"` — literally
true). A record may say `population: "warm"` only because its request
genuinely ran in a loaded, primed process — pinned by tests whose
fixture answers correctly only from request ≥ 1 of a process.

**Cancellation latency is harness-measured**: a monotonic clock
(`performance.now()`) starts when the client writes the cancel request
and stops at the terminal cancelled acknowledgement (or conclusive
stream close). The protocol's cancelled frame carries no report and
nothing a runtime says about its own cancel latency is read.

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

The summarizer **refuses statistics** for any inconsistent group —
mixed configurations, duplicate run ids, duplicate repetitions, or
disagreeing planned-repetition counts render as
`refused (inconsistent group)` with the errors listed; reliability
totals still count every attempt. A joint median over mixed records is
never produced.
