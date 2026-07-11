# Local Model Benchmark Contract

This document is Plume's durable contract for reproducible local-model
benchmarking. It is intentionally a specification, not a result set and not a
runner. D128 adds no benchmark scripts, fixtures, model downloads, inference
runs, dependencies, or performance claims.

D129 may implement the harness described here before or independently of the
128 GB Apple Silicon machine. That machine is the intended place to run the
full matrix, not a prerequisite for writing the harness. D130 will use
recorded evidence to rewrite README and product-launch performance language.
Until then, do not present a proposed benchmark configuration or an isolated
manual run as a Plume result.

## Goals and boundaries

The harness must answer five separate questions. A fast token stream is not,
by itself, evidence of a good agent or a responsive Plume app.

| Layer | Question | Required measurements or verdicts |
| --- | --- | --- |
| Raw inference | How does the runtime serve this exact model? | Time to first token (TTFT), prompt tokens per second, generation tokens per second |
| Resource use | What does that run cost the machine? | Peak unified memory, swap delta, thermal state, wall energy when available |
| Context scaling | Does behavior change as prompt size grows? | 4096, 16384, 32768, 65536-token points, then only verified supported larger windows |
| Agent quality | Can the model complete the bounded coding task? | Tool-call validity, file discovery, diff, apply, verification, and task verdicts |
| Reliability | Does the path remain usable when it fails or is interrupted? | Crashes, malformed streams, timeouts, cancellation latency, restart/recovery |

Raw inference uses a direct client to one runtime endpoint. Product performance
uses Plume's normal orchestration path. One JSONL record measures exactly one
of those paths; it must never fold both timings together and label the result
"inference." Agent suites are functional measurements, not a replacement for
the raw inference or resource layers.

## Comparison contract

### Fixed comparison group

A comparison group is one model, one fixture configuration, one generation
configuration, one runtime configuration, and one measurement path. Supported
runtime rows are:

- Plume-managed MLX-LM.
- `ollama-mlx`, only when the recorded Ollama engine identity proves that MLX
  is the actual backend.
- llama.cpp.

Plume's current verified Ollama label is `GGUF / Metal (Ollama)`. Do not imply
that every Ollama-on-Mac run is MLX. If an `ollama-mlx` row cannot record
verified engine identity, it is `unsupported` or unavailable; it is never
silently substituted with an ordinary Ollama result.

Each runtime path is a distinct result even when it exposes a similar API. A
headline comparison is valid only when every included row has the same source
model revision, fixture revision, context configuration, sampling
configuration, runtime settings, seed where the runtime supports one, and
output-token cap. A missing seed is recorded as `null` with a capability note;
it is not silently treated as a shared seed. Materially different supported
generation controls or runtime settings form different comparison groups.

Strict artifact-parity comparisons require the same runtime-consumed artifact
format and SHA-256 digest. Practical equivalent-source deployment comparisons
may compare MLX and GGUF artifacts converted from one exact source checkpoint,
but must label themselves `equivalentSource`, never identical weights. Do not
compare different source revisions or quantizations as though they were the
same. Do not substitute an Ollama-managed model or an unspecified checkpoint
for the MLX-first proof path.

Every run records the Plume git SHA, runtime engine and backend identity,
runtime configuration, model/artifact identity, full hardware/OS manifest,
power mode/source when discoverable, and starting thermal state. A dirty Plume
checkout is recorded as `dirty: true`; results from it cannot support a public
performance claim.

### Run populations and summaries

Cold and warm runs are different populations and must never share a median.

- A cold run starts after the measured runtime process has stopped. If the OS
  cannot clear file or model caches without unsupported intervention, record
  `bestEffortCold`; do not claim a hardware-cold measurement.
- A warm run begins with the runtime loaded and one non-recorded priming
  request using the same configuration. The priming request is not a result.
- Each group selects 3 to 30 recorded repetitions. Fewer than three completed
  repetitions are incomplete evidence, not a summary.
- Every attempt carries `includeInSummary` and `exclusionReason`. The
  summarizer derives selected, completed, included, and excluded counts from
  those attempt records; it must not create a second handwritten summary
  record.
- Report the median and spread (minimum, maximum, and interquartile range
  when at least four values exist). Never publish a fastest or best run as the
  group result.

Failures remain in reliability totals even when excluded from a latency
summary. An attempt may be excluded only for a recorded bounded mechanical
reason, such as cancellation; exclusion never deletes its JSONL record.

### Timing boundaries and units

All durations use a monotonic clock and are stored in milliseconds as decimal
numbers. Token rates use tokens per second. One record contains one `timing`
object for its one invocation and one `measurementPath`: `rawRuntime` for a
direct client or `plumeOrchestration` for Plume's normal prompt assembly, IPC,
provider routing, event handling, and UI-facing completion.

| Field | Meaning |
| --- | --- |
| `timeToFirstTokenMs` | Elapsed time from sending the complete request to receiving the first non-empty generated-token event. A non-token status frame does not qualify. |
| `promptTokensPerSecond` | Prompt token count divided by prompt evaluation duration. It is `null` unless both values are authoritative for that runtime. |
| `generationTokensPerSecond` | Generated token count divided by generation duration from first generated token through the terminal event. It is `null` unless both values are authoritative. |
| `pairId` | Nullable bounded id joining deliberately paired raw/Plume invocations. Unpaired attempts remain valid. |
| `extraOverheadMs` | A derived summary value only: paired `plumeOrchestration.endToEndMs - rawRuntime.endToEndMs`. It is never stored in an attempt record. |

A pair is valid only when it has exactly one completed `rawRuntime` attempt and
one completed `plumeOrchestration` attempt with the same fixture revision and
digest, source model/artifact identity, context, sampling and runtime-relevant
configuration, population, output-token cap, and completed output-token count.
Any mismatch makes the pair invalid for overhead calculation, but leaves each
attempt valid on its own. Do not fabricate prompt or generation rates from wall
time when the required token count or phase duration is unavailable.

### Resource and context rules

Peak unified memory is the maximum machine unified-memory usage observed from
request start through terminal completion, in bytes. `swapDeltaBytes` is
`swapAtEndBytes - swapAtStartBytes`; it may be negative and is never clamped
to zero. `thermalStart` and `thermalEnd` are nullable enums: use `null` when
no supported probe exists or a measurement fails, and use `unknown` only when
a supported probe succeeds but reports an unclassified state. `wallEnergyJoules`
is energy at the wall for the measured interval only when a supported meter
supplies it.

The required context points are exactly 4096, 16384, 32768, and 65536 tokens.
Larger points are attempted only when the exact runtime/model configuration
verifiably supports them. Each point reserves the configured `maxOutputTokens`:
the final assembled prompt token count plus that reserve must not exceed the
accepted context window. A path with an unverifiable tokenizer or chat-template
count is `unsupported`, with null token metrics, rather than guessed. Record
prompt bytes only as supplementary evidence, never as a context point or
comparison key. An unsupported point is a recorded `unsupported` result with
null metrics, not a zero-value row.

The record identifies the tokenizer and chat template used for prompt counting,
their immutable revision/digest when available, and whether counts are
`runtimeReported`, `harnessTokenizer`, or `unavailable`. The final assembled
prompt count is the count after the exact template is applied.

## Deterministic local suites

D129 will create a versioned, deterministic, local-only fixture pack. Fixtures
must contain synthetic or publicly distributable code and text only: no user
repositories, private prompts, copied project text, credentials, or network
dependencies. A fixture manifest fixes its prompt, expected files, verifier,
timeout, and content digest. The suite records the manifest revision and
digest, not prompt or source contents in the result JSONL.

| Suite | Deterministic fixture | Functional pass criterion | Record in addition to common fields |
| --- | --- | --- | --- |
| `short-chat` | A bounded factual local prompt with an exact normalized answer. | The normalized reply equals the fixture answer before timeout. | Reply classification and terminal stream outcome. |
| `long-context-retrieval` | Padded local text with planted keyed facts at fixed locations. | Required keys are returned and decoys are not asserted as facts. | Requested/accepted context, final assembled prompt tokens, retrieved-key verdicts, truncation. |
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

One UTF-8 JSON object occupies one line. One record is exactly one invocation
on one `measurementPath`, never a combined-path attempt or a handwritten
summary. Summary tables are derived from these records.

```json
{
  "schemaVersion": 1,
  "run": {
    "id": "bench_01J00000000000000000000000",
    "groupId": "grp_01J00000000000000000000000",
    "pairId": "pair_01J0000000000000000000000",
    "timestampUtc": "2026-07-11T12:00:00Z",
    "population": "warm",
    "coldMethod": null,
    "repetition": 1,
    "plannedRepetitions": 5,
    "measurementPath": "plumeOrchestration"
  },
  "plume": { "gitSha": "0123456789abcdef0123456789abcdef01234567", "dirty": false },
  "host": {
    "machine": "Mac Studio (M3 Ultra)",
    "appleChip": "Apple M3 Ultra",
    "unifiedMemoryBytes": 137438953472,
    "cpuCoreCount": 32,
    "gpuCoreCount": 80,
    "os": "macOS 26.0",
    "osBuild": "25A000",
    "powerMode": "automatic",
    "powerSource": "ac",
    "thermalStart": "nominal"
  },
  "runtime": {
    "path": "plume-mlx-lm",
    "name": "mlx-lm",
    "version": "0.0.0",
    "engine": "mlx-lm",
    "backend": "MLX",
    "configuration": {
      "digest": "sha256:...",
      "mtp": null,
      "speculativeDecoding": null,
      "promptCache": null,
      "kvCacheQuantization": null,
      "contextTokens": 32768,
      "batchSize": null,
      "threads": null,
      "gpuLayers": null
    },
    "transport": "openai-sse"
  },
  "model": {
    "sourceId": "publisher/model-name",
    "sourceRevision": "immutable-revision",
    "artifact": {
      "format": "mlx",
      "sha256": "sha256:...",
      "quantizationMethod": "grouped",
      "quantizationBits": 4,
      "quantizationGroupSize": 64,
      "conversionProvenance": "publisher-mlx-release",
      "conversionConfigDigest": "sha256:..."
    },
    "comparisonParity": "strictArtifact",
    "context": { "pointTokens": 32768, "configuredTokens": 32768, "acceptedTokens": 32768, "maxOutputTokens": 512 },
    "sampling": {
      "temperature": 0.0,
      "topP": 1.0,
      "topK": null,
      "minP": null,
      "repeatPenalty": 1.0,
      "seed": 42,
      "maxOutputTokens": 512,
      "stopSequences": []
    }
  },
  "suite": { "id": "single-file-bug-fix", "caseId": "bug-001", "fixtureRevision": "v1", "fixtureDigest": "sha256:..." },
  "tokens": {
    "tokenizer": { "identity": "publisher/tokenizer", "revision": "immutable-revision", "digest": "sha256:...", "chatTemplate": "default", "chatTemplateDigest": "sha256:..." },
    "countSource": "harnessTokenizer",
    "finalAssembledPromptTokens": 1200,
    "promptBytes": 4800,
    "outputTokens": 84
  },
  "timing": {
    "method": "clientObserved",
    "timeToFirstTokenMs": 0.0,
    "promptEvaluationMs": null,
    "generationDurationMs": 0.0,
    "promptTokensPerSecond": null,
    "generationTokensPerSecond": 0.0,
    "endToEndMs": 0.0
  },
  "resources": { "peakUnifiedMemoryBytes": 0, "swapDeltaBytes": 0, "thermalEnd": "nominal", "wallEnergyJoules": null },
  "includeInSummary": true,
  "exclusionReason": null,
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

- `schemaVersion` is the positive integer major version. D129 writes version
  `1`. A reader refuses a file whose major version is newer than it supports;
  it must not guess a mapping. Older versions require an explicit migrator.
- All documented top-level fields are required. A producer must not add
  unversioned fields. A reader receiving an unknown field at its supported
  version preserves it if possible, ignores it for analysis, and emits a
  warning; it must not turn it into a zero or an inferred metric.
- IDs, including nullable `pairId`, are ASCII `[A-Za-z0-9_-]`, at most 64
  characters. Timestamps are UTC RFC 3339 strings, at most 32 characters.
  Runtime, model, suite, and case identifiers are at most 256 characters.
  Version, digest, configuration, conversion-provenance, and error-class
  strings are at most 512 characters. `exclusionReason` is a stable category
  at most 256 characters, or `null` only when `includeInSummary` is true.
  `includeInSummary` is always boolean; an excluded attempt has a non-null
  reason and remains present for reliability totals.
- `plannedRepetitions` is 3 through 30 and `repetition` is 1 through that
  value. A malformed count is rejected rather than truncated.
- `population` is `cold` or `warm`. `coldMethod` is `processRestart`,
  `bestEffortCold`, or `null`; it is required for cold attempts and `null`
  for warm attempts. `measurementPath` is `rawRuntime` or
  `plumeOrchestration`. `comparisonParity` is `strictArtifact` or
  `equivalentSource` and follows the comparison rules above.
- `temperature`, `topP`, `topK`, `minP`, `repeatPenalty`, `seed`, and
  `maxOutputTokens` record every generation-affecting control supported by the
  harness. Unsupported settings are `null`; omitted and zero are distinct.
  `stopSequences` is an ordered list of at most 16 strings, each at most 256
  characters. `maxOutputTokens` is required and must match the reserved value
  in `model.context`.
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
- `countSource` is `runtimeReported`, `harnessTokenizer`, or `unavailable`.
  When it is `unavailable`, final prompt/output token counts and token-derived
  rates are `null`, and a context-scaling attempt is `unsupported`.
- `thermalStart` and `thermalEnd` are `nominal`, `fair`, `serious`,
  `critical`, `unknown`, or `null`, with the thermal semantics above.
- `host.machine`, `host.appleChip`, `host.unifiedMemoryBytes`,
  `host.cpuCoreCount`, `host.gpuCoreCount`, `host.os`, `host.osBuild`,
  `host.powerMode`, and `host.powerSource` identify the host; any unsupported
  or undiscoverable field is `null`, never inferred.
- `artifacts` contains at most 16 references, each at most 512 ASCII
  characters. The only allowed local root, reserved for D129, is
  `benchmark-artifacts/`. Its path grammar is
  `benchmark-artifacts/<component>(/<component>)*`, with `/` separators only
  and each component matching `[A-Za-z0-9][A-Za-z0-9._-]{0,127}`. A reference
  rejects absolute paths, empty, `.` or `..` components, backslashes, NUL
  bytes, and any existing symlink that resolves outside the repository or the
  allowed root. It never embeds source, prompt text, model output, an absolute
  path, or a home directory; evidence digests belong in their typed record
  fields instead of being encoded into paths.
- Each serialized record is at most 64 KiB.
- Inline logs, private fixture text, source contents, credentials, tokens,
  and environment dumps are prohibited. A runner rejects a record that would
  exceed a bound rather than silently cutting text and changing evidence.

`timing.method` is `runtimeReported`, `clientObserved`, or `unavailable` and
applies only to that attempt's timing values. Unsupported resource probes and
unavailable energy meters use `null`; they never use `0`, `false`, or
`nominal` as a stand-in.

## Reliability collection

Every run records its configured `outcome.timeoutLimitMs` and terminal outcome.
A timeout means the harness reached that
configured limit; it is not a guessed slow result. A malformed stream is any
response that violates the selected runtime protocol before a valid terminal
event. A crash means the managed runtime process exits unexpectedly during the
attempt. The summarizer counts both per-run events and group reliability totals
from all attempt records, including excluded attempts.

Cancellation latency starts when the harness issues the cancellation request
and ends only when the path reports a terminal cancelled event or the stream
is conclusively closed. If neither happens before its cancellation timeout,
the result is `timedOut` and `cancellationLatencyMs` is `null`; a client-side
button state is not acknowledgement. Restart/recovery is true only after a
post-crash restart reaches health and passes the fixture's follow-up request.

## Publishing and artifact hygiene

- Commit raw JSONL only when it is small, sanitized, and within the record
  bounds above. Keep large logs, model outputs, traces, energy captures, and
  disposable fixture artifacts local and ignored under `benchmark-artifacts/`.
- Generate README tables from recorded results. No benchmark table is typed
  by hand.
- Every public performance claim links to the hardware manifest,
  configuration, fixture revision, raw result record, and Plume commit SHA.
- Label text as one of: **measured fact** (directly supported by a linked
  record), **inference** (a stated interpretation of measured facts), or
  **marketing copy** (a user-facing claim with linked evidence). Marketing
  copy must not masquerade as a measurement.
- Do not publish a cross-runtime conclusion when source revisions or required
  comparison-group settings differ. Use the strict-artifact or
  equivalent-source label required above.

## Reserved D129 command shapes

D128 creates none of these files. Their names and responsibilities are
reserved so D129 can add one implementation without changing the evidence
contract.

| Reserved file | Responsibility | Inputs | Outputs |
| --- | --- | --- | --- |
| `scripts/benchmark-model.sh` | Run one direct-runtime or Plume-orchestration invocation for one exact model/runtime/configuration. | Sanitized config, fixture manifest, runtime path, model identity, repetition and population selection. | One bounded attempt JSONL record and local artifact references. |
| `scripts/benchmark-suite.sh` | Select deterministic fixture cases and coordinate warm/cold groups, reliability cases, and selected runtime paths. | Suite manifest, fixture revision, matrix of model/runtime/context/configuration values. | Ordered single-invocation calls and one sanitized JSONL collection. |
| `scripts/summarize-benchmarks.ts` | Validate records, refuse unsupported schema versions, group like-for-like attempts, validate pairs, and render derived summaries. | Sanitized JSONL records only. | Derived selected/completed/included/excluded counts, reliability totals, median/spread summaries, and README-ready tables with evidence links. |

The scripts must not download models, substitute weights, invent unavailable
metrics, or place model output in committed result records. D129 is responsible
for their exact flags, local ignore rules, fixture location, and tests; D128
only reserves the contract.
