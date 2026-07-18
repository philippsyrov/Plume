# MLX Runtime Contract

Current implementation and upstream notes for Plume-owned MLX-LM. Provider
shape and lifecycle are defined in `docs/MODEL_PROVIDERS.md`; wire types live
in `docs/IPC_CONTRACT.md § providers`.

The product target is "MLX-first on Apple Silicon" from
`docs/LOCAL_AGENT_NORTH_STAR.md`. D36 already verifies on-disk
that a folder is MLX (`mlx-folder` kind); this contract keeps "we found
weights", "the release contains a runtime", and "the model started" separate.

## Current shipped boundary

- Release packaging generates a relocatable arm64 runtime with pinned CPython
  3.12.13, `mlx-lm` 0.31.3, `mlx` 0.32.0, and `mlx-metal` 0.32.0. Its lock,
  executable digest, package identity, relocation cleanup, and absence of model
  weights are verified before bundling. Packaging fails closed on identity
  drift; release start fails closed if its regular non-symlink interpreter is
  missing, and PATH Python is never used.
- The runtime is not a model. Qwen Coder 1.5B weights are a separate explicit,
  pinned, verified, resumable download into Application Support.
- Fixed catalog Qwen starts without a project. Arbitrary discovered folders
  retain the trusted-project gate. Both share the same supervisor and SSE chat
  adapter.
- Catalog chat does not ship broad tools, a multi-step coding loop, semantic
  retrieval, Browser authority, or computer actions.

## Ground truth: the `mlx_lm.server` surface

All claims below were verified by reading the source at
`https://raw.githubusercontent.com/ml-explore/mlx-lm/main/mlx_lm/server.py`
on 2026-05-15. If the upstream contract changes, this section is
the part that needs re-grounding.

### CLI

The upstream `main()` deprecates the `python -m mlx_lm.server …`
form and points at two non-deprecated launchers:

```python
print(
    "Calling `python -m mlx_lm.server...` directly is deprecated."
    " Use `mlx_lm.server...` or `python -m mlx_lm server ...` instead."
)
```

So Plume must spawn one of these two:

```text
# Console-script entry point (verified by the upstream print
# message; lives in the package's `console_scripts` once
# `pip install mlx-lm` runs):
mlx_lm.server --model <path> --host 127.0.0.1 --port <N> [...]

# Subcommand form — same effect, doesn't depend on the console
# script being on PATH:
python -m mlx_lm server --model <path> --host 127.0.0.1 --port <N> [...]
```

D39 should spawn the **subcommand form** (`python -m mlx_lm
server …`). Rationale: "we resolved a python interpreter" is a
stronger guarantee than "the console script is on PATH" — a user
who did `pip install --user mlx-lm` without `~/.local/bin` on
PATH still gets a working `python -m mlx_lm server` invocation.

Supported flags Plume cares about. The full upstream list is
longer; read it directly from `mlx_lm/server.py::main()` if a
new flag becomes relevant.

| Flag                          | Default     | Plume use                                      |
| ----------------------------- | ----------- | ---------------------------------------------- |
| `--model`                     | (required)  | Absolute path of an `mlx-folder` from D36.     |
| `--host`                      | `127.0.0.1` | Pass `127.0.0.1` explicitly; never bind 0.0.0.0. |
| `--port`                      | `8080`      | Plume-allocated; see § Port allocation.         |
| `--adapter-path`              | (none)      | Reserved for a later inventory pass.            |
| `--trust-remote-code`         | off         | Stay off — unsafe-by-default opt-in.            |
| `--log-level`                 | `INFO`      | Pass `INFO`; bump to `DEBUG` from a hidden setting later. |

Everything else stays at the upstream default. Per-request
overrides (`temperature`, `max_tokens`, etc.) flow through the
`/v1/chat/completions` request body, not the CLI.

### Defaults

- **Host:** `127.0.0.1`
- **Port:** `8080` — **collides with llama.cpp's documented
  default** (`docs/IPC_CONTRACT.md § providers`). Plume must
  pick a non-conflicting port at spawn time. See § Port
  allocation below.
- **Trust remote code:** off. Plume keeps it off; turning it on
  is an unsafe-by-default opt-in we will not auto-enable.

### Routes

| Method | Path                    | Notes                                       |
| ------ | ----------------------- | ------------------------------------------- |
| GET    | `/health`               | Returns `{"status": "ok"}` — ready probe.   |
| GET    | `/v1/models`            | OpenAI-style: scans HF cache + the loaded model. |
| POST   | `/v1/completions`       | Bare completions (no chat formatting).      |
| POST   | `/v1/chat/completions`  | OpenAI chat completions.                    |
| POST   | `/chat/completions`     | Alias of the above.                         |

Anything else returns 404. There is no `/api/show`-equivalent
metadata endpoint — model details (parameter count, quantization,
context length) have to come from the on-disk `config.json`
Plume already reads in `providers/local_models.rs`.

### Streaming shape

`/v1/chat/completions` with `stream: true` returns
`Content-Type: text/event-stream`. Each chunk is

```text
data: {"choices":[{"delta":{"content":"..."}, "index":0}], ...}\n\n
```

Stream terminator:

```text
data: [DONE]\n\n
```

If the request body sets `stream_options.include_usage = true`,
a final usage chunk lands just before `[DONE]`. Plume's existing
`providers/openai_compat.rs` parses the JSON shape of `/v1/models`
ONLY — it is not an SSE parser. D39 landed `chat/openai_sse.rs`:
a pure line-driven parser that classifies each wire line into
`SseEvent::Delta { content, finish_reason }`, `SseEvent::Usage(...)`,
or `SseEvent::Done`. It tolerates CRLF, ignores comments and
non-`data:` fields, and emits both events on a single frame when a
server inlines `usage` alongside the stop chunk. The chat runtime
slice (D40+) drives this from the HTTP read loop; nothing from
`openai_compat.rs` is reused.

### Process model

- `ThreadingHTTPServer` — one Python thread per HTTP request.
- One background `ResponseGenerator` thread does the actual MLX
  inference; requests queue against it via a `Queue()`.
- `KeyboardInterrupt` (SIGINT) triggers `httpd.shutdown()` plus
  `response_generator.stop_and_join()` — a graceful exit path.
- Optional distributed mode (`mx.distributed.init()`) is
  reserved; Plume's single-host MVP ignores it.

The implication for supervision: the server starts in a single
OS process, accepts SIGINT for graceful shutdown, and falls back
to SIGKILL on timeout. No PID files, no Unix socket, no admin
endpoint — pure HTTP.

## Plume integration

### Module placement

```text
src-tauri/src/providers/
  mlx_runtime.rs         bundled/debug interpreter resolution
  catalog.rs             fixed Qwen identity + receipt validation
  catalog_download*.rs   verified download, publication, removal
  mlx_lm/
    mod.rs              registry entry + ProviderCapabilities
    process.rs          spawn / supervise / shutdown registry core
    process_*.rs        launch, health, start, stop, diagnostics helpers
```

The split mirrors `chat/ollama/` (`blocking.rs` + `streaming.rs`
+ `http.rs`) — D25's decomposition pattern. Keep each child file
under 500 lines per `docs/DECOMPOSITION.md`.

### Spawn shape

`providers.startServer(id, modelId)` launches an inventory model from a
trusted project. `providers.catalogStart({ catalogId })` launches only the
fixed receipt-backed Qwen catalog model and has no project input. Both enter
the same supervisor after their backend-only path and command resolution.
The generic handler:

1. Resolve `modelId` against the local-model inventory. Accept
   only entries with `kind === 'mlx-folder'` (D36) or a verified
   HF repo id; reject anything else as `ProviderError::Unsupported`
   so the user isn't promised an MLX path that won't run.
2. Allocate a free port (§ Port allocation).
3. Resolve the interpreter before entering the supervisor, then compose the
   non-deprecated command line:
   ```text
   python -m mlx_lm server
       --model <absolute path or repo id>
       --host 127.0.0.1
       --port <allocated>
       --log-level INFO
   ```
4. Spawn via the resolved command with stdout + stderr captured
   into a ring buffer Plume can surface for bring-up errors.
5. Poll `GET http://127.0.0.1:<port>/health` with a backoff
   (50 ms → 200 ms → 500 ms, give up after ~30 s). When the
   probe returns `{"status":"ok"}` the server is ready.
6. Return a `ServerHandle` keyed by `{pid, port, model_id}`.

Failure modes worth distinguishing in the response:
- the bundled release interpreter is absent or not a regular file → fail
  closed before spawn; release never falls back to PATH Python.
- `mlx_lm` not importable → server exits ~immediately with a
  Python `ModuleNotFoundError`. Read 200 ms of stderr after spawn
  and surface as `ProviderError::NotInstalled("mlx_lm package missing; install with `pip install mlx-lm`")`.
- Health probe timeout → kill the process, surface
  `ProviderError::Internal("mlx_lm.server did not become ready within 30 s")`.

### Port allocation

The collision matters: llama-server defaults to 8080, mlx_lm.server
defaults to 8080, Plume probes 8080 for llama-cpp. Plume must
not blindly start mlx_lm.server on 8080 — it would either fail
to bind (if llama-server is running) or shadow llama-server from
Plume's own llama-cpp adapter.

**Strategy (one approach, not two):** bind a TCP socket to
`127.0.0.1:0`, read the OS-assigned ephemeral port, close the
socket, and spawn `mlx_lm.server` with `--port <that>`. The OS
picks the port from its own ephemeral range (Darwin:
`net.inet.ip.portrange.first..last`, typically 49152–65535), not
from a Plume-curated band — there is no value in pretending we
have an opinion about which port number is "ours". An earlier
revision of this doc said "pick 51500–51599" alongside the
bind-`:0` trick; that was incoherent. Drop the band and trust
the OS.

TOCTOU window between Plume's `close` and mlx_lm's `bind` is
small but real. If the spawned server's health probe never
returns ready inside the 30 s budget, kill it, allocate a fresh
ephemeral port, and retry once. Surface
`ProviderError::Internal("…")` after the second failure so the
user can investigate (almost always the answer is "another
process holds it" — they can see that in `lsof -i`).

The live registry can re-adopt exact handles after a webview reload. It does
not persist ownership across a full Plume process restart.

### Model path semantics

`--model` accepts two forms:

1. **Local folder** — the path must contain `config.json`,
   tokenizer files, and weights. Plume's `local_models.rs`
   already identifies these as `mlx-folder` (D36). Pass the
   absolute path.
2. **HuggingFace repo id** — string like
   `mlx-community/Qwen2.5-Coder-7B-Instruct-4bit`. mlx_lm.server
   resolves these against the HF cache (`~/.cache/huggingface/`).
   On first use, mlx_lm will download the repo — Plume MUST NOT
   trigger network downloads from the runtime path. Reject
   non-cached repo ids upfront by checking the HF cache before
   spawn. (Or, in the conservative MVP, accept only local
   folders.)

D39's smallest-useful scope: **local folders only**. HF repo
support is additive and gates on a hf-cache scan, which adds
roughly one file-read pass.

### Chat routing

The reply shape on `/v1/chat/completions` is OpenAI-compatible.
Plume's existing chat path already understands NDJSON (Ollama),
so adding SSE parsing for mlx_lm is the new piece:

1. POST JSON body matching `OpenAIChatRequest` shape (the
   `messages`, `model`, `stream`, `max_tokens`, `temperature`
   subset Plume already constructs internally for D7-D14).
2. Read body as a line stream, parse each `data: ...` line as
   JSON, route `choices[0].delta.content` into the existing
   `ChatTokenSink`.
3. Treat `data: [DONE]\n\n` as end-of-stream; treat any
   `{"error": {...}}` payload as a transport error.

`chat/done` stats (D9 — `outputTokens`, `evalMs`, `promptTokens`,
`promptMs`) come from the optional `usage` chunk if
`stream_options.include_usage` is set in the request body.
Plume should always set it for MLX (Ollama returns the same data
without an opt-in; we just have to ask for it).

### Cancellation

mlx_lm.server has no per-request cancel endpoint. The graceful
path is **drop the HTTP connection** — closing the SSE response
body should cause the server's response handler to break out of
its write loop. Plume's existing `chat.cancel` already drops the
upstream read at the client side, so the work is "wire the same
drop-the-body pattern Ollama uses." If the server ignores the
disconnect and keeps generating (possible — depends on the
ResponseGenerator's poll cadence), Plume's per-stream
`CancellationToken` still flips the frontend state to
`cancelled` and stops emitting tokens, matching the existing
behavior for Ollama edge cases.

### Shutdown

`providers.stopServer(handle)` triggers (as shipped in
`process.rs::stop_child`):

1. SIGINT to the child's process group (`kill -2 -<pid>`; the
   child is a session leader via `setsid`) — the graceful path the
   server already handles via `KeyboardInterrupt`.
2. Wait up to 3 s (`STOP_SIGINT_GRACE`) for exit.
3. SIGKILL to the whole process group as the floor, then reap.

A second `stopServer` on the same handle returns `NotFound` (the
registry entry is gone). The ring-buffer log is dropped with the
registry entry so a re-spawn starts clean.

**Normal-exit sweep (Thermos I1, hardened in Codex #154).** Quitting
Plume runs a `RunEvent::Exit` hook that latches the registry shut,
drains it, and stops every managed child: running children go through
the same SIGINT-grace → SIGKILL escalation, one stopper thread per
child, joined concurrently — the whole sweep is bounded by roughly
one grace period. Children still in their startup window (spawned,
`/health` not yet 200) are covered too: the supervisor reserves a
registry slot carrying the child's pid *under the same lock as the
spawn*, so the sweep SIGKILLs mid-startup children by pid and the
latch guarantees their start thread can never register them
afterwards. The registry is capped at `MAX_MANAGED_SERVERS` (8)
concurrent servers, enforced atomically at that same reservation —
concurrent starts cannot overshoot it, and a full supervisor refuses
before spawning anything. Children that exited on their own are
reaped out of the count first, so a crashed server never pins a cap
slot.

**Handle-loss recovery (Thermos I1).** `providers.listServers`
returns every *healthy* server this Plume process currently manages
(`handleId`, `port`, `pid`, the inventory `modelId` recorded at
start, `modelLabel`, uptime); children that exited on their own are
reaped before listing so a reloaded webview never re-adopts a dead
pid as running. The webview re-adopts running servers from it instead
of stranding children whose handles lived only in the old page's
memory.

**Hard-crash limitation — explicitly NOT covered.** The exit sweep
runs only on a normal event-loop exit. If Plume is SIGKILLed,
crashes, or loses power, no sweep runs — and because children are
deliberately detached into their own sessions, nothing else signals
them either. Orphans from a hard crash keep running until the user
kills them (Activity Monitor / `kill <pid>`; the PID is surfaced in
the panel and diagnostics). Persisted-PID adoption across Plume
restarts is unimplemented and would need its own reviewed slice
(PID-reuse checks before any claim of ownership).

### ProviderCapabilities

```rust
ProviderCapabilities {
    streaming: true,
    tool_calls: ToolCallSupport::None,     // until mlx_lm exposes a tool API
    vision: false,                          // text-only for the MVP
    embeddings: false,
    max_context: 0,                         // unknown until per-model probe
    owned_process: true,                    // Plume-managed
}
```

`max_context` stays `0` ("unknown") until a follow-up parses
each model's `config.json` for `max_position_embeddings`. The
honesty rule (`docs/AGENTS.md`) requires `0` over a guess.

## Model architecture support {#model-architecture-support}

A folder that Plume classifies as `mlx-folder` is not guaranteed to
**run** under any given `mlx-lm` version. Plume's scanner (D36)
only confirms the folder *looks* like an MLX-quantized transformer
on disk (`config.json` with `quantization: {bits, group_size}`).
Whether `mlx_lm` can actually load it depends on per-architecture
python code shipped in `mlx_lm/models/*.py` upstream. The model's
`config.json` `model_type` (e.g. `gemma2`, `llama`, `qwen2`,
`gemma4`) selects which module loads the weights; new architectures
land in `mlx_lm` releases on their own cadence.

**The dominant failure mode** is a weight-namespace mismatch when
`mlx_lm` dispatches to the wrong model class. We've seen this in
the wild (D56) against `mlx-community/gemma-4-e4b-it-4bit`, a
`Gemma4ForConditionalGeneration` (vision-language) variant — it
classifies as `mlx-folder` on disk, `/health` answers 200 after
spawn, then the first chat request triggers weight loading and
fails with:

```
ValueError: Received 126 parameters not in model:
language_model.model.layers.24.self_attn.k_norm.weight, ...
```

mlx_lm 0.31.3's `gemma4` module reads a different weight namespace
than the conditional-generation variant uses, so the dispatch
mismatches and load fails. **This is not a Plume bug.**

### What Plume does about it

* **D52's "Logs & diagnostics" disclosure** on the row shows the
  full ring-buffer tail (16 KiB of mlx-lm stdout/stderr). The
  traceback above lands there verbatim, so the operator can read
  what mlx-lm complained about.
* **D57's hint detector** (frontend, `src/features/providers/
  mlxLogPatterns.ts`) classifies the most common failure shapes
  into a one-line label + suggestion above the log:
  - "Received N parameters not in model" / "Missing N parameters
    from model" → `unsupported-architecture` hint.
  - `KeyError: '<type>'` from `mlx_lm.utils` / `mlx_lm.models` →
    `unknown-model-type` hint.
  - `ImportError` from `mlx_lm.models.*` → `import-error` hint
    (usually version skew; `pip install -U mlx-lm` is the fix).
  - `RuntimeError: ... CUDA` → `cuda-missing` hint (wrong venv).
  The detector is heuristic and returns `null` when nothing fires;
  the raw log remains the source of truth.

### What this means for users picking a model

* **Text-only chat models** — Gemma 2 (`mlx-community/gemma-2-2b-it`,
  `mlx-community/gemma-2-9b-it`), Llama 3 / 3.1 / 3.2, Qwen 2.5
  (including Qwen2.5-Coder), Mistral, DeepSeek-Coder — generally
  load cleanly against current mlx-lm releases. Pick one of these
  for first runs.
* **Conditional generation / vision-language models** — Gemma-3-VL,
  Gemma-4-VL (`*ForConditionalGeneration`), Llava-style models —
  may fail to load even when the folder shape is correct. mlx-lm
  catches up over time; check upstream release notes if a specific
  architecture you want isn't loading.
* **Brand-new model families** (released within the last week) —
  may not have an mlx_lm module yet. The fix is upstream-side; try
  `pip install -U mlx-lm` and re-attempt. If the error persists,
  pick a different model.

### How to verify outside Plume

`scripts/smoke-mlx-runtime.sh <folder>` (D53) runs the same load
path mlx-lm uses, outside the full app. If the smoke fails with
the same traceback Plume's diagnostics surface, it's an mlx-lm
support issue, not a Plume wiring issue. If the smoke succeeds
and Plume still fails, file an issue with both logs.

## Dependency and download posture

The packaged release includes its own pinned MLX-LM runtime. That build-time
resource is generated by the explicit release/smoke preparation command; Plume
does not install Python packages at runtime. The fixed Qwen weights remain a
separate user-triggered network action with visible progress, cancel, resume,
verification, retry, and removal. No mutable revision or caller-supplied model
URL is accepted.

The advanced arbitrary-folder path remains dependency-honest in debug builds:
Plume never runs an installer. Contributors may choose an explicit
`PLUME_MLX_PYTHON`; missing packages or incompatible architectures surface as
start errors.

### Picking the interpreter: bundled release + `PLUME_MLX_PYTHON` development override {#plume-mlx-python}

App-level starts resolve their interpreter before entering the supervisor:

- **Release:** only `<app resources>/mlx-runtime/bin/python3` is accepted.
  The resolver requires a regular, non-symlink file; an absent or malformed
  bundle fails closed before a child process is spawned. It ignores both
  `PLUME_MLX_PYTHON` and PATH, so Finder / LaunchServices cannot silently
  select a different Python.
- **Debug:** a non-empty `PLUME_MLX_PYTHON` wins, then a valid bundled runtime
  is used when present, then the existing bare `python` fallback remains for
  ordinary contributor tests before the release payload is assembled.

The explicit debug override names which interpreter to spawn:

```bash
export PLUME_MLX_PYTHON="$HOME/.venvs/mlx-env/bin/python"
open -a "Plume (dev)"   # or however you launch Plume
```

With the override set in a debug build, Plume's supervisor spawns
`/Users/<you>/.venvs/mlx-env/bin/python -m mlx_lm server …`
directly. **No shell activation required** — the venv interpreter
finds `mlx_lm` via its own site-packages because the binary's path
includes it. This sidesteps the LaunchServices-PATH gotcha (when
Plume launches from Finder / Spotlight / the Dock, it inherits a
bare PATH that doesn't include an activated venv).

The override value is trimmed but otherwise taken verbatim; Plume does not
expand `~` or environment variables inside it. Empty or whitespace-only
values are ignored.

We do NOT pre-check that the resolved path is executable, exists,
or has `mlx_lm` importable. `Command::spawn` will surface those as
`StartError::Spawn(io::Error)` with the OS message ("No such file
or directory", "permission denied") — the IPC layer maps it to a
clear "is python installed? mlx_lm package installed?" string and
the D52 diagnostics disclosure shows it inline. Pre-checking would
be racy and duplicate work.

### Recommended contributor setup

For debug development with arbitrary local model folders:

```bash
python -m venv ~/.venvs/mlx-env
source ~/.venvs/mlx-env/bin/activate
pip install --upgrade pip mlx-lm
deactivate
# In your shell rc (or a launchd plist, or a Plume launcher):
export PLUME_MLX_PYTHON="$HOME/.venvs/mlx-env/bin/python"
```

Use this setup for debug development. Packaged release builds instead require
their verified bundled runtime resource.

## Remaining limitations and follow-ups

- **Streaming-cancel server behavior.** Plume can drop the body,
  but mlx_lm's actual response-loop cadence determines how
  quickly generation stops. Worth one timing probe before
  shipping cancel claims.
- **Per-model context length.** mlx_lm.server doesn't expose a
  per-model `context_length` endpoint. The on-disk `config.json`
  has `max_position_embeddings`; Plume can read it during model
  inventory and surface it in `providers.modelDetails`.
- **HF cache discovery.** When (later) accepting HF repo ids,
  Plume needs to know which repos are already cached without
  triggering a network call. mlx_lm imports
  `huggingface_hub.scan_cache_dir`; Plume can mirror the cache
  layout (`<HF_HOME>/hub/models--<org>--<name>/snapshots/...`)
  in a small inventory pass.
- **Concurrent windows / multiple projects.** The process-wide registry can
  supervise up to eight servers and a webview reload can re-adopt them. Durable
  ownership across separate Plume processes or app restarts remains absent.
- **Adapters.** `--adapter-path` is a real CLI flag; Plume's
  inventory doesn't yet recognize adapter folders. A later
  inventory pass should detect them so the panel can offer
  "load adapter on top of base model".

## What the shipped runtime does not do

- It does not contain Qwen or any other checkpoint weights.
- It does not accept arbitrary catalog URLs, revisions, Python programs, or
  process arguments from the frontend.
- It does not guarantee every folder classified as `mlx-folder` is supported
  by the pinned upstream runtime.
- It does not survive or adopt owned children after a hard Plume crash.
- It does not add broad tools, a production multi-iteration agent, or automatic
  retrieval.

## Pointer

- This doc: implementation contract.
- `docs/MODEL_PROVIDERS.md § MLX-LM` — short user-facing summary.
- `docs/LOCAL_AGENT_NORTH_STAR.md § MLX-first` — product rationale.
- `docs/IPC_CONTRACT.md § providers` — `startServer` /
  `stopServer` wire shape this slice will fill in.
- Upstream source of truth:
  `https://github.com/ml-explore/mlx-lm/blob/main/mlx_lm/server.py`.
