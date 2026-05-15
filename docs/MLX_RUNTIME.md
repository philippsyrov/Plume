# MLX Runtime Integration Plan

Implementation-ready notes for making Plume own an MLX-LM server
process. D38 is the **docs-only research spike** — no code, no
dependency installs. The next slice (call it D39 by convention)
turns this into a Plume-managed runtime per
`docs/MODEL_PROVIDERS.md § Runtime categories`.

The product target is "MLX-first on Apple Silicon" from
`docs/LOCAL_AGENT_NORTH_STAR.md`. D36 already verifies on-disk
that a folder is MLX (`mlx-folder` kind); this doc closes the
loop from inventory → managed runtime → chat.

## Ground truth: the `mlx_lm.server` surface

All claims below were verified by reading the source at
`https://raw.githubusercontent.com/ml-explore/mlx-lm/main/mlx_lm/server.py`
on 2026-05-15. If the upstream contract changes, this section is
the part that needs re-grounding.

### CLI

```text
python -m mlx_lm.server \
    --model <path or HF repo id> \
    --host 127.0.0.1 \
    --port 8080 \
    [--adapter-path PATH] \
    [--allowed-origins "*"] \
    [--draft-model PATH] \
    [--num-draft-tokens 3] \
    [--trust-remote-code] \
    [--log-level INFO] \
    [--chat-template TEMPLATE] \
    [--use-default-chat-template] \
    [--temp 0.0] [--top-p 1.0] [--top-k 0] [--min-p 0.0] \
    [--max-tokens 512] \
    [--chat-template-args '{}'] \
    [--decode-concurrency 32] \
    [--prompt-concurrency 8] \
    [--prefill-step-size 2048] \
    [--prompt-cache-size 10] \
    [--prompt-cache-bytes BYTES] \
    [--pipeline]
```

Plume passes only the small subset it needs (model, host, port,
log-level, possibly adapter-path). Everything else stays at the
upstream default; per-request overrides (`temperature`,
`max_tokens`, etc.) come in via the `/v1/chat/completions` body.

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
SSE parser (`providers/openai_compat.rs` for `/v1/models`)
doesn't yet parse SSE chat streams, but the shape is identical
to LM Studio's and llama-server's so the parser can be promoted
into a shared `openai_sse` helper when D39 lands the wiring.

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

## Plume integration plan

### Module placement

```text
src-tauri/src/providers/
  mlx_lm/
    mod.rs              registry entry + ProviderCapabilities
    process.rs          spawn / supervise / shutdown (D39)
    routes.rs           OpenAI-SSE chat routing (D39 / D40)
```

The split mirrors `chat/ollama/` (`blocking.rs` + `streaming.rs`
+ `http.rs`) — D25's decomposition pattern. Keep each child file
under 500 lines per `docs/DECOMPOSITION.md`.

### Spawn shape

`providers.startServer(id, modelId)` (already in
`docs/IPC_CONTRACT.md § providers`) is the trigger. The handler:

1. Resolve `modelId` against the local-model inventory. Accept
   only entries with `kind === 'mlx-folder'` (D36) or a verified
   HF repo id; reject anything else as `ProviderError::Unsupported`
   so the user isn't promised an MLX path that won't run.
2. Allocate a free port (§ Port allocation).
3. Compose the command line:
   ```text
   python -m mlx_lm.server
       --model <absolute path or repo id>
       --host 127.0.0.1
       --port <allocated>
       --log-level INFO
   ```
4. Spawn via `std::process::Command::new("python")` with stdout
   + stderr captured into a ring buffer Plume can surface for
   bring-up errors.
5. Poll `GET http://127.0.0.1:<port>/health` with a backoff
   (50 ms → 200 ms → 500 ms, give up after ~30 s). When the
   probe returns `{"status":"ok"}` the server is ready.
6. Return a `ServerHandle` keyed by `{pid, port, model_id}`.

Failure modes worth distinguishing in the response:
- `python` not on PATH → `ProviderError::NotInstalled("python interpreter not found")`.
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

**Strategy:** Plume picks a port in a private band (proposal:
**51500–51599**, well clear of common dev ports and the documented
provider defaults) by binding `127.0.0.1:0`, reading the chosen
port, closing the socket, and immediately spawning with `--port
<that>`. There is a tiny TOCTOU window where another process
could steal the port between Plume's `close` and mlx_lm's `bind`;
if `health` never comes up, retry once with a fresh port before
giving up.

A future slice can persist "Plume's MLX runtime is on port N" so
multiple windows on the same project don't double-launch.

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

`chat.done` stats (D9 — `outputTokens`, `evalMs`, `promptTokens`,
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

`providers.stopServer(handle)` triggers:

1. SIGINT (`kill -2 <pid>`) — the graceful path the server
   already handles via `KeyboardInterrupt`.
2. Wait up to ~2 s for exit.
3. SIGTERM if still alive.
4. SIGKILL as the floor.

Idempotent: a second `stopServer` on the same handle returns
`Ok(())` if the pid is gone. Drop the ring-buffer log on
successful shutdown so a re-spawn starts clean.

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

## Dependency-install posture

Plume MUST NOT auto-install `mlx-lm`. Per `docs/AGENTS.md § Hard
rules § 7`, dependency installs require an explicit ask. The
right experience is:

1. User clicks Start on an `mlx-folder` model.
2. Plume tries to spawn `python -m mlx_lm.server …`.
3. On `ModuleNotFoundError`, the panel shows a copy-pastable
   command (`./scripts/dev-env.sh pip install mlx-lm` per
   `docs/DEPENDENCY_ISOLATION.md`) and refuses to auto-run it.
4. The user runs it; Plume retries on next click.

Same posture as the rest of the dependency-honesty pattern.

## Open questions

These don't block D39. Resolve before they bite:

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
- **Python interpreter selection.** macOS often has multiple
  Pythons. Plume should resolve via `which python3` /
  `which python` at session start and surface the chosen
  interpreter in the panel so the user can audit it.
- **Concurrent windows / multiple projects.** D39's MVP is one
  MLX server per Plume process. A future slice can reuse a
  running server across project sessions on the same host —
  same model bind, multiple `chat.send`s.
- **Adapters.** `--adapter-path` is a real CLI flag; Plume's
  inventory doesn't yet recognize adapter folders. A later
  inventory pass should detect them so the panel can offer
  "load adapter on top of base model".

## What this slice does NOT do

- Spawn a process. (D39.)
- Add `mlx_lm` to any manifest. The dependency is on the user's
  machine, not in the Cargo / npm tree.
- Change `providers.list` / `providers.health` semantics. The
  existing MLX-LM entry in the static registry already declares
  `owned_process: true` (per `docs/MODEL_PROVIDERS.md § Built-in
  adapters`); this doc only commits how the spawn will work.
- Touch `chat.send`. The chat-routing changes land in the slice
  that wires the server in, not here.

## Pointer

- This doc: implementation contract.
- `docs/MODEL_PROVIDERS.md § MLX-LM` — short user-facing summary.
- `docs/LOCAL_AGENT_NORTH_STAR.md § MLX-first` — product rationale.
- `docs/IPC_CONTRACT.md § providers` — `startServer` /
  `stopServer` wire shape this slice will fill in.
- Upstream source of truth:
  `https://github.com/ml-explore/mlx-lm/blob/main/mlx_lm/server.py`.
