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
- **The `plumeOrchestration` boundary excludes the webview hop**
  (D129C, below): the sidecar measures through Plume's real modules to
  the UI-facing emission point, with stdout standing in for the Tauri
  event bridge. Webview transport and render are not in the number.
- **Resource probes are real-transport only** (D129B). Records from
  the scripted fake runtime keep `resources.*` and `host.thermalStart`
  null — probing the machine around a deterministic fixture would make
  its records depend on whatever else the machine is doing.
- **`git apply` diff mechanics remain only for the fake path.** With a
  configured `plumeBench.binary` (D129C), diff mechanics run through
  Plume's real Rust patch modules; without it, the documented
  `git apply --check` / `git apply` mechanics apply and those records
  must not be published as Plume results (fake-runtime records never
  qualify for publication anyway).

## The MLX-LM adapter (D129A)

`transport: "openai-sse"` selects the real runtime adapter
(`scripts/benchmark/mlx-runtime.ts` + `runtime-factory.ts`). It owns
one `python -m mlx_lm server --model <dir> --host 127.0.0.1 --port
<ephemeral>` process per session, exactly like Plume's supervisor:
spawn (as its own process-group leader), poll `GET /health` (startup
budget in the config), serve, SIGINT with a 3 s grace, then SIGKILL.
Shutdown signals the whole process group with an unconditional final
SIGKILL sweep — a forked worker never outlives the session, even if
it ignores SIGINT, even when the leader crashed on its own or exited
during startup. Warm/cold semantics
carry over unchanged: a warm group is one primed live server; a cold
attempt is a fresh server per invocation (`processRestart` — the
model load happens before the request, so timings still start at
request send).

**Verified identity, or no run.** Before any session starts — at
resolve time AND again at every server launch, so a checkpoint
changed mid-suite refuses instead of running under a stale digest —
the factory re-digests the model directory IN FULL (sha256 over every
file; symlinks refused; deliberately no cache — a stat-level
fingerprint cannot see a same-size rewrite with a restored mtime, so
only hashing the actual bytes counts as verification) and requires it
to equal the declared `model.artifact.sha256`. The server command must pass a
single two-token `--model server.modelDir` (duplicates and the
`--model=` form are refused — argparse would let a later duplicate
load different bytes). The engine must be `mlx-lm`, whose
`mlx_lm.__version__` is probed through the configured interpreter and
must match a declared version (or fill a null one); other engine
declarations over `openai-sse` cannot be verified and are refused.
The engine probe is likewise never cached: every launch re-probes the
interpreter and refuses if the served version drifted from the one
the records carry (a venv upgraded mid-suite refuses instead of
recording stale identity). A mismatch refuses the run — records never
carry an unverified identity.

**Client-observed timing** (`timing.method: "clientObserved"`,
monotonic): `timeToFirstTokenMs` = request write → first non-empty
content delta; `generationDurationMs` = first content delta →
terminal `[DONE]` (timestamped when `[DONE]` is parsed — a server
lingering with the connection open adds nothing); `endToEndMs` =
request write → terminal;
`promptEvaluationMs` is not client-observable and stays null (so the
prompt rate stays null too). Token counts come only from the server's
reported `usage` (`stream_options.include_usage`); SSE deltas are
never counted as tokens. Deliberate cancellation aborts the HTTP
stream; latency runs from abort to conclusive close.

**Resource probes (D129B)** (`scripts/benchmark/resource-probes.ts`):
real-transport runs sample machine resources around exactly the
MEASURED request — not priming, not session/model load — matching the
contract's "request start through terminal completion" window.
`peakUnifiedMemoryBytes` is machine memory-used via `vm_stat`
((active + wired down + occupied-by-compressor) pages × page size), a
documented proxy sampled on a 100 ms interval (so even the shortest
real requests get in-window samples) plus one sample at each window
edge; an in-flight sample is drained before the peak is read. `swapDeltaBytes` is `sysctl vm.swapusage` "used" at end
minus start — signed, never clamped. `host.thermalStart` /
`resources.thermalEnd` read `NSProcessInfo.thermalState` through
`osascript` JXA (a genuine 4-level macOS probe); an integer outside
0..3 records `unknown`, a failed probe records null.
`wallEnergyJoules` stays null: no supported wall meter exists here,
and a package-power estimate is not wall energy. Failure posture: any
broken probe records null — never `0`, never a guessed enum — and
never fails, delays, or times the model run (start probes complete
before the request is sent; end probes run after the terminal event;
only the memory sampler ticks concurrently, which is what observing a
peak means). Records from the scripted fake runtime never carry probe
values — the transport gate keeps them deterministic.

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

## The plumeOrchestration path (D129C)

`measurementPath: "plumeOrchestration"` measures the same verified
mlx server THROUGH Plume's own code. The driver is the `plume_bench`
sidecar (`src-tauri/src/bin/plume_bench.rs`), a thin shell over the
real product modules — `src-tauri` gained a library target so the
sidecar links the actual `prompts::assemble`, `chat::mlx_lm`
(Plume's TCP + SSE client, product request body, product connect
timeout and overall budget), and `patch` modules instead of a
reimplementation. One session = one verified server + one sidecar
process; warm/cold semantics carry over (warm reuses both, cold
restarts both). Identity verification is identical to `rawRuntime`
and runs before every launch.

**Measurement boundary.** Each generate is timed monotonically inside
the sidecar from request receipt (prompt assembly is INSIDE the
window — assembly is exactly the overhead this path exists to
observe) to the UI-facing emission of each token and the terminal
event, with stdout standing in for the Tauri event bridge. The
webview transport/render hop is NOT included. Timings therefore come
from the measured system itself: `timing.method: "runtimeReported"`.

**Declared-equals-wired posture.** Plume's chat path sends NO client
sampling controls and its own explicit `max_tokens` cap (D129C made
that cap explicit in the product — previously the app silently
inherited mlx-lm's version-dependent server default). The factory
verifies via the sidecar's `--health` handshake that the config
declares exactly that posture — all sampling controls null, empty
stop sequences, `maxOutputTokens` equal to the cap actually on the
wire — and refuses anything else. Contradictory or unsupported
configs never become records.

**Pairing.** Raw and Plume attempts sharing a `pairId` let the
summarizer derive `extraOverheadMs` (Plume minus raw end-to-end) —
derived only, never stored in attempt records, and only for pairs
that satisfy the contract's strict validity rules (one completed
attempt per path; equal fixture digest, model identity, context,
sampling, runtime configuration, population, output cap, and
completed output-token count). The raw side of a pair uses the same
null-sampling posture, so both paths put the SAME request shape on
the wire. Groups never mix measurement paths — the group key includes
the path, so a mixed group refuses statistics.

**Diff mechanics through Plume.** `plume_bench patch-check` runs the
real `validate_patch` / `apply_patch` against the disposable fixture
copy and returns Plume's own response taxonomy; a configured
`plumeBench.binary` routes the agent-suite oracles through it (any
measurement path may opt in). A broken bridge records null mechanics,
never a false verdict.

**Paired smoke**: `scripts/benchmark-plume-smoke.sh` builds the
sidecar, then runs the same checkpoint on both paths (3 warm + 3 cold
pairs, shared pairIds) and prints the group and pair tables.
Mechanics validation only — records stay in gitignored
`benchmark-artifacts/`, never a performance claim. CI tests drive the
path with a protocol-faithful fake sidecar
(`plume-orchestration.test.ts`); `npm run test` needs no cargo build.

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
