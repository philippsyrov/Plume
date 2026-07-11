# Local Model Benchmark Contract

This document is Plume's durable contract for reproducible local-model
benchmarking. It is intentionally a specification, not a result set and not a
runner. D67 adds no benchmark scripts, fixtures, model downloads, inference
runs, dependencies, or performance claims.

D68 will implement the harness described here when the 128 GB Apple Silicon
machine is available. D69 will use recorded evidence to rewrite README and
product-launch performance language. Until then, do not present a proposed
benchmark configuration or an isolated manual run as a Plume result.

## Goals and boundaries

The harness must answer five separate questions. A fast token stream is not,
by itself, evidence of a good agent or a responsive Plume app.

| Layer | Question | Required measurements or verdicts |
| --- | --- | --- |
| Raw inference | How does the runtime serve this exact model? | Time to first token (TTFT), prompt tokens per second, generation tokens per second |
| Resource use | What does that run cost the machine? | Peak unified memory, swap delta, thermal state, wall energy when available |
| Context scaling | Does behavior change as prompt size grows? | 4 KiB, 16 KiB, 32 KiB, 64 KiB, then only supported larger contexts |
| Agent quality | Can the model complete the bounded coding task? | Tool-call validity, file discovery, diff, apply, verification, and task verdicts |
| Reliability | Does the path remain usable when it fails or is interrupted? | Crashes, malformed streams, timeouts, cancellation latency, restart/recovery |

Raw inference uses a direct client to one runtime endpoint. Product performance
uses Plume's normal orchestration path. A result must identify which path it
measures; it must never fold their timings together and label the result
"inference." Agent suites are functional measurements, not a replacement for
the raw inference or resource layers.

## Comparison contract

### Fixed comparison group

A comparison group is one model and one fixture configuration measured across
the supported runtime paths:

- Plume-managed MLX-LM.
- Ollama MLX.
- llama.cpp.

Each runtime path is a distinct result even when it exposes a similar API. A
headline comparison is valid only when every included row has the same model
weights and exact revision, quantization, prompt fixture revision, context
configuration, sampling configuration, seed where the runtime supports one,
and output-token cap. A missing seed is recorded as `null` with a capability
note; it is not silently treated as a shared seed.

Do not compare different weights, revisions, or quantizations as though they
were identical. They may be published as separate configurations, with the
difference named beside every result. Do not substitute an Ollama-managed
model or an unspecified checkpoint for the MLX-first proof path.

Every run records the Plume git SHA, runtime name and version, model identity,
hardware/OS manifest, power mode, and starting thermal state. A dirty Plume
checkout is recorded as `dirty: true`; results from it cannot support a public
performance claim.

### Run populations and summaries

Cold and warm runs are different populations and must never share a median.

- A cold run starts after the measured runtime process has stopped. If the OS
  cannot clear file or model caches without unsupported intervention, record
  `bestEffortCold`; do not claim a hardware-cold measurement.
- A warm run begins with the runtime loaded and one non-recorded priming
  request using the same configuration. The priming request is not a result.
- Each group uses 3 to 30 recorded repetitions. The selected count, completed
  count, and excluded-run reason are recorded. Fewer than three completed
  repetitions are incomplete evidence, not a summary.
- Report the median and spread (minimum, maximum, and interquartile range
  when at least four values exist). Never publish a fastest or best run as the
  group result.

Failures remain in the group's reliability count. A run may be excluded from a
latency summary only for a recorded mechanical reason such as a cancelled
measurement; exclusion never deletes the JSONL record.

### Timing boundaries and units

All durations use a monotonic clock and are stored in milliseconds as decimal
numbers. Token rates use tokens per second. Token counts use the tokenizer or
runtime count named in the record; an unavailable count is `null`.

| Field | Meaning |
| --- | --- |
| `timeToFirstTokenMs` | Elapsed time from sending the complete request to receiving the first non-empty generated-token event. A non-token status frame does not qualify. |
| `promptTokensPerSecond` | Prompt token count divided by prompt evaluation duration. It is `null` unless both values are authoritative for that runtime. |
| `generationTokensPerSecond` | Generated token count divided by generation duration from first generated token through the terminal event. It is `null` unless both values are authoritative. |
| `rawRuntime` | Direct-client measurement at the runtime endpoint, without Plume UI, IPC, prompt assembly, or event rendering. Runtime-reported values and client-observed values are labeled separately. |
| `orchestration` | Measurement through Plume's normal path: prompt assembly, IPC, provider routing, event handling, and UI-facing completion. |
| `extraOverheadMs` | `orchestration.endToEndMs - rawRuntime.endToEndMs` only for a paired direct/Plume measurement with the same fixture, output cap, and completed-token count. Otherwise it is `null`. |

Do not fabricate prompt or generation rates from wall time when the required
token count or phase duration is unavailable. The raw and orchestration paths
must be recorded independently rather than inferred by subtracting unrelated
requests.

### Resource and context rules

Peak unified memory is the maximum machine unified-memory usage observed from
request start through terminal completion, in bytes. `swapDeltaBytes` is
`swapAtEndBytes - swapAtStartBytes`; it may be negative and is never clamped
to zero. Thermal state records the platform's named state at start and end
(`nominal`, `fair`, `serious`, `critical`, or `unknown`); unsupported platforms
use `unknown`, not a made-up normal state. `wallEnergyJoules` is energy at the
wall for the measured interval only when a supported meter supplies it.

For every context point, record requested prompt bytes, prompt tokens when
available, configured context window, accepted context window, and any
truncation outcome. The required points are 4 KiB, 16 KiB, 32 KiB, and 64 KiB
of fixture content. Larger points are attempted only when the exact runtime
and model configuration report support. An unsupported point is a recorded
`unsupported` result with `null` metrics, not a zero-value row.

## Deterministic local suites

D68 will create a versioned, deterministic, local-only fixture pack. Fixtures
must contain synthetic or publicly distributable code and text only: no user
repositories, private prompts, copied project text, credentials, or network
dependencies. A fixture manifest fixes its prompt, expected files, verifier,
timeout, and content digest. The suite records the manifest revision and
digest, not prompt or source contents in the result JSONL.

| Suite | Deterministic fixture | Functional pass criterion | Record in addition to common fields |
| --- | --- | --- | --- |
| `short-chat` | A bounded factual local prompt with an exact normalized answer. | The normalized reply equals the fixture answer before timeout. | Reply classification and terminal stream outcome. |
| `long-context-retrieval` | Padded local text with planted keyed facts at fixed locations. | Required keys are returned and decoys are not asserted as facts. | Requested/accepted context, retrieved-key verdicts, truncation. |
| `code-explanation` | Small synthetic source file plus a question with a rubric. | Required rubric facts appear and prohibited claims do not. | Rubric item verdicts and response length. |
| `single-file-bug-fix` | One broken synthetic file and an allowlisted fixture verifier. | Proposed diff validates, applies in a disposable fixture copy, and the verifier passes. | Target file, diff validity, apply, verifier, rollback outcome. |
| `multi-file-navigation` | Small synthetic repository with required and decoy files. | Required files are discovered, a valid scoped diff is proposed, and the verifier passes after apply. | Discovered paths, required/forbidden path verdicts, diff/apply/verifier outcomes. |
| `tool-calling-agent-loop` | Fixed tool catalog and isolated synthetic repository. | Every call is schema-valid and allowed, required evidence is found, then the fixture verifier and final oracle pass. | Per-call validity, tool limit, discovery, diff/apply/verifier/task verdicts. |
| `cancellation-restart` | A deterministic long response plus a restartable local runtime. | Cancellation reaches a terminal cancelled outcome within the fixture timeout; after restart, health and a follow-up fixture pass. | Cancellation acknowledgement latency, stream terminal kind, crash/restart/recovery outcomes. |

No suite sets a model-specific prompt, expected speed, quality score, or
performance threshold in this specification. Pass/fail is only the fixture's
functional oracle. A latency, memory, or rate result is measured evidence to
compare, not a pass merely because it looks fast.

### Agent metric definitions

For an agent case, record each metric as `true`, `false`, or `null` when the
fixture cannot exercise it. `null` means unavailable or not applicable; it
does not count as success or failure.

- `toolCallValid`: every attempted call names an allowlisted tool and has
  schema-valid, in-bounds arguments. A blocked or malformed call is false.
- `correctFileDiscovery`: all manifest-required paths were discovered before
  proposing the diff, and no manifest-forbidden decoy was claimed as the
  target.
- `validDiff`: the proposed unified diff passes Plume's current validator for
  the disposable fixture root.
- `patchApplySuccess`: the validated diff applies cleanly to the pristine
  disposable fixture and its cleanup/rollback succeeds where applicable.
- `verificationSuccess`: the fixture's named, local, allowlisted verifier
  exits successfully after the applied patch.
- `finalTaskSuccess`: the fixture's final oracle passes. It may be false even
  when the patch applies or the verifier exits successfully.

## JSONL result contract

One UTF-8 JSON object occupies one line. A record is a single attempt, never
a hand-written summary. Summary tables are derived from these records.

```json
{
  "schemaVersion": 1,
  "run": {
    "id": "bench_01J00000000000000000000000",
    "groupId": "grp_01J00000000000000000000000",
    "timestampUtc": "2026-07-11T12:00:00Z",
    "population": "warm",
    "coldMethod": null,
    "repetition": 1,
    "plannedRepetitions": 5
  },
  "plume": { "gitSha": "0123456789abcdef0123456789abcdef01234567", "dirty": false },
  "host": {
    "machine": "Apple Silicon",
    "unifiedMemoryBytes": 137438953472,
    "os": "macOS 26.0",
    "powerMode": "automatic",
    "thermalStart": "nominal"
  },
  "runtime": {
    "path": "plume-mlx-lm",
    "name": "mlx-lm",
    "version": "0.0.0",
    "transport": "openai-sse"
  },
  "model": {
    "id": "publisher/model-name",
    "revision": "immutable-revision",
    "quantization": "4-bit",
    "context": { "configuredTokens": 32768, "acceptedTokens": 32768 },
    "sampling": { "temperature": 0.0, "topP": 1.0, "seed": 42, "maxOutputTokens": 512 }
  },
  "suite": { "id": "single-file-bug-fix", "caseId": "bug-001", "fixtureRevision": "v1", "fixtureDigest": "sha256:..." },
  "tokens": { "prompt": 1200, "output": 84 },
  "timing": {
    "rawRuntime": { "method": "client-observed", "timeToFirstTokenMs": 0.0, "promptEvaluationMs": null, "generationDurationMs": 0.0, "promptTokensPerSecond": null, "generationTokensPerSecond": 0.0, "endToEndMs": 0.0 },
    "orchestration": { "timeToFirstTokenMs": 0.0, "endToEndMs": 0.0, "extraOverheadMs": null }
  },
  "resources": { "peakUnifiedMemoryBytes": 0, "swapDeltaBytes": 0, "thermalEnd": "nominal", "wallEnergyJoules": null },
  "outcome": {
    "status": "passed",
    "toolCallValid": null,
    "correctFileDiscovery": null,
    "validDiff": true,
    "patchApplySuccess": true,
    "verificationSuccess": true,
    "finalTaskSuccess": true,
    "stream": "completed",
    "timeout": false,
    "timeoutLimitMs": 30000,
    "cancellationLatencyMs": null,
    "crash": false,
    "restartRecovery": null,
    "errorClass": null
  },
  "artifacts": []
}
```

The example values demonstrate shape only. They are not measured results.

### Field rules, bounds, and null semantics

- `schemaVersion` is the positive integer major version. D68 writes version
  `1`. A reader refuses a file whose major version is newer than it supports;
  it must not guess a mapping. Older versions require an explicit migrator.
- All documented top-level fields are required. A producer must not add
  unversioned fields. A reader receiving an unknown field at its supported
  version preserves it if possible, ignores it for analysis, and emits a
  warning; it must not turn it into a zero or an inferred metric.
- IDs are ASCII `[A-Za-z0-9_-]`, at most 64 characters. Timestamps are UTC
  RFC 3339 strings, at most 32 characters. Runtime, model, suite, and case
  identifiers are at most 256 characters. Version, digest, and error-class
  strings are at most 512 characters.
- `plannedRepetitions` is 3 through 30 and `repetition` is 1 through that
  value. A malformed count is rejected rather than truncated.
- `promptEvaluationMs`, `generationDurationMs`, and `timeoutLimitMs` use the
  same millisecond rules as the other timing fields. `timeoutLimitMs` is the
  configured limit for the attempt; it is `null` only when the fixture has no
  timeout. Numeric metrics are finite non-negative numbers except `swapDeltaBytes`,
  which is a finite signed integer. Counts are non-negative integers. A
  missing, unsupported, or unmeasurable value is `null`; zero means a measured
  zero.
- `status` is `passed`, `failed`, `unsupported`, `cancelled`, `timedOut`, or
  `error`. `stream` is `completed`, `malformed`, `cancelled`, `timedOut`,
  `crashed`, or `unavailable`. `errorClass` is a bounded stable category, not
  a raw exception, stack trace, prompt, or model output.
- Each serialized record is at most 64 KiB. It may contain at most 16 artifact
  references, each at most 512 ASCII characters. An artifact reference is a
  sanitized repository-relative path plus optional digest; it never embeds
  source, prompt text, model output, an absolute path, or a home directory.
- Inline logs, private fixture text, source contents, credentials, tokens,
  and environment dumps are prohibited. A runner rejects a record that would
  exceed a bound rather than silently cutting text and changing evidence.

The `rawRuntime.method` field is `runtime-reported`, `client-observed`, or
`unavailable`. The method applies to the timing values in that object. The
orchestration object is `null` only when the run intentionally measures the
direct runtime path alone. Unsupported resource probes and unavailable energy
meters use `null`; they never use `0`, `false`, or `nominal` as a stand-in.

## Reliability collection

Every run records its configured `outcome.timeoutLimitMs` and terminal outcome.
A timeout means the harness reached that
configured limit; it is not a guessed slow result. A malformed stream is any
response that violates the selected runtime protocol before a valid terminal
event. A crash means the managed runtime process exits unexpectedly during the
attempt. The harness counts both per-run events and group totals.

Cancellation latency starts when the harness issues the cancellation request
and ends only when the path reports a terminal cancelled event or the stream
is conclusively closed. If neither happens before its cancellation timeout,
the result is `timedOut` and `cancellationLatencyMs` is `null`; a client-side
button state is not acknowledgement. Restart/recovery is true only after a
post-crash restart reaches health and passes the fixture's follow-up request.

## Publishing and artifact hygiene

- Commit raw JSONL only when it is small, sanitized, and within the record
  bounds above. Keep large logs, model outputs, traces, energy captures, and
  disposable fixture artifacts local and ignored.
- Generate README tables from recorded results. No benchmark table is typed
  by hand.
- Every public performance claim links to the hardware manifest,
  configuration, fixture revision, raw result record, and Plume commit SHA.
- Label text as one of: **measured fact** (directly supported by a linked
  record), **inference** (a stated interpretation of measured facts), or
  **marketing copy** (a user-facing claim with linked evidence). Marketing
  copy must not masquerade as a measurement.
- Do not publish a cross-runtime conclusion when weights, revisions, or
  quantizations differ. Publish the configurations separately instead.

## Reserved D68 command shapes

D67 creates none of these files. Their names and responsibilities are
reserved so D68 can add one implementation without changing the evidence
contract.

| Reserved file | Responsibility | Inputs | Outputs |
| --- | --- | --- | --- |
| `scripts/benchmark-model.sh` | Run one direct-runtime or Plume-orchestration measurement group for one exact model/runtime/configuration. | Sanitized config, fixture manifest, runtime path, model identity, repetition and population selection. | Bounded attempt JSONL records and local artifact references. |
| `scripts/benchmark-suite.sh` | Select deterministic fixture cases, coordinate warm/cold groups, reliability cases, and the selected runtime paths. | Suite manifest, fixture revision, matrix of model/runtime/context/configuration values. | Ordered calls to the model benchmark command and one sanitized JSONL collection. |
| `scripts/summarize-benchmarks.ts` | Validate records, refuse unsupported schema versions, group like-for-like attempts, and render derived summaries. | Sanitized JSONL records only. | Median/spread summaries and generated README-ready tables with evidence links. |

The scripts must not download models, substitute weights, invent unavailable
metrics, or place model output in committed result records. D68 is responsible
for their exact flags, local ignore rules, fixture location, and tests; D67
only reserves the contract.
