# Apple And Qwen Model Onboarding Design

**Date:** 2026-07-17
**Status:** approved for implementation

## Goal

A fresh Plume installation on an Apple Silicon Mac must offer two useful
model choices from the model selector itself:

1. Apple's on-device Foundation Model when the host can actually use it.
2. A coding-focused Qwen MLX model that Plume can download, verify, start,
   select, and reuse without requiring Ollama, a user-managed Python
   installation, or an open project.

The selector is the ordinary setup path. Settings remains the home for
advanced diagnostics and storage controls, not a prerequisite for chat.

## Product Boundaries

- Models and their runtimes are app-level resources. Starting or downloading
  a catalog model does not require project trust.
- Project files, project memory, topics, skills, patches, and tools keep their
  existing trust and explicit-context boundaries.
- Apple generation uses only the on-device system model. This slice does not
  use Private Cloud Compute.
- Qwen weights download only after an explicit user action. Plume never
  silently downloads or updates a model.
- Existing user-provided local-model starts keep their current trusted-project
  gate. Only the fixed Plume catalog model gains the app-level start path.
- Ollama remains a compatibility provider and is not required for first-run
  success.
- The shipped-vs-candidate firewalls remain unchanged: this work adds two chat
  providers, not broad tools, agent Browser authority, semantic retrieval, or
  automatic prompt authority.

## Chosen Model

The initial downloadable catalog contains exactly one model:

- Repository: `mlx-community/Qwen2.5-Coder-1.5B-Instruct-4bit`
- Revision: `b3252a2f97102b1fb1571fec2c9b27219a8536be`
- Reported repository storage: `868,628,559` bytes
- License: Apache-2.0
- Product label: **Qwen Coder 1.5B**
- Product description: **Recommended for coding**

The catalog manifest pins the repository revision, required file list, byte
sizes, and SHA-256 digests. A mutable branch name such as `main` is never a
download authority. The Qwen 2.5 Coder 3B checkpoint used during exploratory
smoke testing is excluded because its Qwen Research License is noncommercial.

## Architecture

### App-level model catalog

Rust owns a small static catalog and exposes typed state to the frontend. Each
entry reports identity, provider, installation state, availability, fit copy,
license/source metadata, and the one action currently allowed. The frontend
never supplies a repository URL, destination path, executable path, or model
checkpoint path.

The catalog initially has two entries:

- `apple-system`: installed by macOS, never downloaded by Plume.
- `qwen-coder-1.5b-mlx-4bit`: downloadable into Plume's Application Support
  model directory.

Downloaded weights are shared by local and project chats and survive Plume
updates. Temporary downloads live under the same app-data volume so verified
installation can finish with an atomic rename.

### Apple Foundation Models bridge

A small bundled Swift executable links Apple's `FoundationModels` framework.
Plume does not depend on the macOS 27 `fm` CLI.

The helper has two bounded modes:

1. `availability` emits one JSON result describing `available`,
   `os-unsupported`, `device-ineligible`, `apple-intelligence-disabled`,
   `model-not-ready`, or `failed` with safe ordinary-language detail.
2. `generate` accepts one bounded JSON request on stdin and emits JSON-lines
   token and terminal records on stdout.

Rust performs prompt assembly, trust checks, exact-context resolution,
redaction, size enforcement, event sequencing, cancellation, and transcript
persistence before the helper sees content. The helper receives only the
already-approved chat messages. It has no filesystem browsing or project-tool
interface.

The helper runs per generation rather than as a persistent network server.
Rust owns the child process, closes stdin after the request, bounds stdout and
stderr, and terminates the child on cancellation or deadline. No localhost
port or additional network surface is introduced.

On macOS versions below 26, Rust reports `os-unsupported` without launching
the helper. On supported systems, the helper's real framework result is the
source of truth. A nominally available model may still fail at generation;
that error remains visible and does not remove the Qwen fallback.

### Bundled MLX-LM runtime

Release packaging creates a relocatable, Apple-Silicon-only Python runtime
containing pinned versions of Python, `mlx-lm`, `mlx`, and `mlx-metal`. The
runtime build inputs, source URLs, versions, licenses, and SHA-256 hashes live
in a checked-in lock manifest. Release packaging fails closed when the
runtime is absent or its recorded identity does not match.

The packaged app resolves the bundled interpreter from its resources and
prefers it for catalog Qwen starts. Development builds retain
`PLUME_MLX_PYTHON` as an explicit override and the existing external-runtime
fallback so contributors are not forced to assemble the release payload for
ordinary tests.

The existing MLX supervisor remains the owner of loopback binding, health
probing, startup deadlines, process limits, diagnostics, cancellation, and
shutdown sweeping. The new catalog start path passes a backend-resolved model
path and bundled interpreter to that supervisor; it does not add a second MLX
process manager.

### Qwen download and installation

Rust performs the catalog download. It resolves only the fixed manifest,
follows HTTPS redirects only to the approved Hugging Face delivery hosts,
enforces per-file and total byte limits, and writes `.part` files inside a
staging directory. Partial files can resume only when their manifest identity
and local length remain valid.

Every required file is hashed before installation. Unexpected files,
oversized responses, digest mismatches, symlinks, path traversal, redirect
violations, cancellation, and network failures leave no installed model.
Successful verification atomically renames the staged snapshot into the
catalog model directory and writes a small receipt containing the catalog id,
revision, manifest digest, installed size, and completion time.

The download surface supports progress events, cancellation, retry, and
removal. Removing Qwen is an explicit action available only when its managed
server is stopped; removal never touches arbitrary local models or shared
third-party caches.

## User Experience

### Model selector

The top selector reads **Choose model** until a model is selected. It opens a
compact popover with two spacious cards and no diagnostic wall of text.

**Apple On-Device**

- Subtitle: **Built into this Mac**
- Available action: **Use Apple Model**
- Unavailable state remains visible and disabled with one short reason.
- Technical framework errors live behind **Details**.

**Qwen Coder 1.5B**

- Subtitle: **Recommended for coding**
- Absent action: **Download** with the approximate download size.
- Downloading state: progress, downloaded bytes, and **Cancel**.
- Verifying state: a determinate label without pretending installation is
  complete.
- Ready action: **Use Qwen**; selecting it starts the managed server and then
  selects the returned exact handle.
- Running state: **Selected** or **Use** when another model is selected.
- Failed state: one short message plus **Retry**; logs stay under **Details**.

The empty chat surface contains one **Choose a model** action that opens this
same popover. No duplicate onboarding flow exists in Settings.

### Settings

Settings retains advanced provider reachability, local model inventory,
runtime logs, model source/license details, storage usage, and managed-model
removal. The primary catalog cards do not show paths, ports, PIDs, environment
variables, or installation commands.

### Accessibility

- The selector keeps the stable accessible name **Model** while its value
  changes to the selected model.
- Each card has one clear heading, status announcement, and primary action.
- Download progress uses a labelled progressbar and does not announce every
  byte update.
- Start, cancel, retry, use, details, and remove actions are reachable by
  keyboard with visible focus.
- Disabled Apple state exposes the short reason in its accessible
  description.

## IPC And State

New provider/catalog IPC is additive and typed. The frontend deals only in
catalog ids and opaque running handles. The backend owns all paths and URLs.

Required operations:

- list catalog entries and current state;
- refresh Apple availability;
- begin, cancel, and observe catalog download;
- start/select catalog Qwen through the existing MLX supervisor;
- stop catalog Qwen through the existing handle path;
- remove installed catalog Qwen when stopped;
- stream Apple chat through the existing `chat/token` and `chat/done` event
  contract.

Download events carry a stable operation id and monotonic sequence number.
Late events from cancelled or superseded operations cannot repaint current
state. Model selection remains window-local. Installed model state and the
download receipt are app-global.

## Error Handling

- Apple framework unavailability is normal state, not a generic provider
  failure.
- Apple generation failures keep the transcript retryable and surface a short
  message; they never silently fall back to Qwen for a sent prompt.
- Qwen download failures preserve valid resumable bytes only when the pinned
  manifest still matches. Corrupt bytes are deleted before retry.
- A runtime packaging or identity mismatch disables Qwen start with an honest
  packaged-runtime error; Plume never falls back silently to an arbitrary
  `python` in a release build.
- Start failure leaves Qwen installed and retryable. Stop and application quit
  retain the existing bounded supervisor cleanup guarantees.
- The UI never claims Apple or Qwen is ready until the backend has established
  the corresponding usable state.

## Verification

### Automated

- Rust tests pin catalog identity, fixed-host download policy, byte caps,
  digest verification, resumable staging, cancellation, atomic installation,
  receipt validation, removal boundaries, and app-level-vs-project trust.
- Rust chat tests use a fake Apple helper to pin JSON-lines streaming,
  sequencing, cancellation, deadlines, malformed output, stderr bounds, and
  terminal-event behavior.
- Swift helper tests pin framework availability mapping and request/output
  protocol using a fake model session boundary; the real framework remains a
  packaged smoke requirement.
- Existing MLX supervisor tests pin that catalog starts reuse its process
  ownership, cap, recovery, shutdown, and exact-handle behavior.
- Frontend tests cover the two cards, terse unavailable reasons, download
  progress/cancel/retry, verification, start/select, persistent selector value,
  empty-chat entrypoint, keyboard operation, and accessible names.
- Packaging tests require the helper, relocatable runtime, runtime identity
  manifest, and third-party notices inside the final app, while excluding
  model weights from the DMG.

### Packaged app

At the exact release-candidate head:

1. Launch from Finder with no shell environment and no Ollama.
2. Confirm both catalog cards appear before a project is opened.
3. Exercise every honest Apple availability state possible on the host; when
   available, send and cancel a real response. Record any OS beta/framework
   failure exactly rather than converting it into a pass.
4. Download Qwen from the selector, cancel once, resume, verify, start, select,
   chat, quit, relaunch, and reuse without another download.
5. Confirm local chat works without a project and project context remains
   unavailable until a project is trusted.
6. Exercise corrupt/interrupted download recovery with the controlled smoke
   harness, not by altering a user's real installed model.
7. Verify app and DMG signatures, bundle identity, architecture, runtime and
   helper presence, third-party notices, final DMG hash, and Git ancestry.

## Documentation And Status

Update the feature inventory, roadmap, provider/runtime docs, IPC contract,
architecture, safety, user guide, domain maps, decomposition records, release
testing path, and third-party notices at the implementation head. Claims must
distinguish:

- Apple adapter shipped versus Apple model available on this particular Mac;
- MLX runtime bundled versus Qwen weights downloaded;
- Qwen catalog chat shipped versus deeper coding-agent execution still not
  shipped;
- on-device Apple generation versus Private Cloud Compute, which remains out
  of scope.

## Non-Goals

- Bundling model weights in the DMG.
- Downloading arbitrary Hugging Face repositories.
- Automatic model updates or catalog expansion.
- Private Cloud Compute or any cloud API.
- Replacing the existing arbitrary local-model inventory.
- Removing Ollama, LM Studio, or llama.cpp compatibility.
- The broader visual overhaul beyond the focused selector and empty-chat
  onboarding surfaces.
- New prompt authority, retrieval, tools, patches, Browser actions, or host
  control.
