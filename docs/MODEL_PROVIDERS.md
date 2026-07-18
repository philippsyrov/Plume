# Model Providers

Plume treats local model runtimes as swappable engines behind a single
Rust trait. The UI never knows whether tokens came from MLX-LM, Ollama,
LM Studio, or llama.cpp.

## Runtime categories

Plume's relationship with a runtime sits on two axes that together
decide what the adapter is responsible for.

**Process ownership.** Either Plume spawns and supervises the runtime
(`owned_process: true`), or Plume connects to a long-running daemon
the user has already started (`owned_process: false`).

**Integration depth.** Either Plume drives the model directly through
this `Provider` trait — prompt assembly, tool-call loop, diff handling,
the whole agent stack on top — or Plume embeds an external *agent
engine* and steps back to a cockpit role.

This file is mostly about the first track. The second is sketched in
§ External agent engines below; nothing in the trait, nothing in the
IPC contract, and nothing in the registry is committed for it yet.

| Category               | Process owner | Integration | Examples                                  |
| ---------------------- | ------------- | ----------- | ----------------------------------------- |
| Plume-managed runtime  | Plume         | provider    | MLX-LM, llama.cpp                         |
| Connected runtime      | User          | provider    | Ollama daemon, LM Studio                  |
| External agent engine  | varies        | engine      | Codex CLI, Claude Code, OpenCode (future) |

Preferred direction: **MLX-first, Ollama-compatible**. On Apple Silicon,
the best Plume-native path should be Plume-managed MLX weights and a
runtime Plume starts/supervises itself. Ollama and LM Studio remain useful
connected runtimes, but they must not become mandatory dependencies for
the core local-agent experience.

Ollama can land in either of the first two: if `ollama serve` is
already running, Plume connects to it; otherwise the adapter offers
to start one and treats it as Plume-managed for the lifetime of that
session.

## Local model library

D27 adds a read-only local model inventory before any Plume-managed
runtime launches. `providers.localModels` scans `PLUME_MODEL_DIR` when
set, otherwise `plume-models/` under the current project root. It
recognizes:

- `.gguf` files
- `.safetensors` files
- `transformer-folder` — any folder shaped like a HuggingFace
  transformer checkpoint (`config.json` + a `tokenizer*` file + a
  `.safetensors` / `.gguf` / `.npz` weight file). This kind is the
  conservative default: a vanilla `huggingface-cli download` of a
  PyTorch or safetensors checkpoint lands here.
- `mlx-folder` — same shape as `transformer-folder` PLUS verified
  MLX evidence. D36 added this stricter classification with two
  signals, either sufficient:
  - a top-level `weights.npz` shard (legacy MLX-LM format), OR
  - a `config.json` carrying a top-level `quantization` object with
    both `group_size` and `bits` integer keys (the MLX-LM
    quantization shape).
  HuggingFace / bitsandbytes use the different key
  `quantization_config`, which is NOT sufficient evidence and keeps
  the folder in `transformer-folder`. Unquantized MLX safetensors
  uploads can be on-disk-identical to a vanilla HF safetensors
  upload; those also stay `transformer-folder` rather than risk a
  false-positive MLX claim.

Plume's runtime-honesty rule (the same rule that keeps Ollama
labeled `GGUF / Metal` instead of `MLX`) forbids claiming MLX
without verifying it. The two signals above are the verification
floor; future slices can layer richer parsing (architecture string,
model card heuristics) on top.

`PLUME_MODEL_DIR` is treated as **trusted operator input**. A relative
value with `..` components will resolve outside the project root —
the scanner does not normalize or reject it. The verb only surfaces
model filenames and byte sizes, so the blast radius is limited to
enumerating those, but anyone wiring CI or shared dev environments
should set the var to an absolute path.

Symlinks inside the model directory are not followed and never appear
in the inventory.

The walker enforces a defensive nesting cap with walkdir-style
semantics — the model directory itself is depth 0, immediate
children are depth 1, and entries strictly past depth 8 are
invisible (files, plain folders, and transformer folders alike).
Filesystem noise is also skipped: any entry whose final name
component starts with `.` (`.git`, `.DS_Store`, `.cache`, dotfile
configs) is ignored. An entry past the cap is silently invisible,
not an error.

This is library truth only: no downloads, no imports, no model
selection, and no server start happen in this path.

The follow-up model-management track should build from this inventory in
small steps:

1. ~~stricter `mlx-folder` detection for verified MLX-format checkpoints~~
   — **landed in D36** (this slice).
2. import/reference flows for existing local weights, including weights
   already downloaded by another local app,
3. memory/context fit estimates before load,
4. Plume-managed MLX-LM process start/stop,
5. chat/edit routing through that owned process,
6. only then model download UX.

Do not skip straight from inventory to "download anything". The product
promise is reliable local coding, so the runtime and resource-honesty path
must land before a large model catalog.

## Provider trait

```rust
#[async_trait]
pub trait Provider: Send + Sync {
    fn id(&self) -> ProviderId;
    fn display_name(&self) -> &str;
    fn capabilities(&self) -> ProviderCapabilities;

    async fn is_installed(&self) -> bool;
    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError>;

    async fn start_server(&self, model: &ModelId)
        -> Result<ServerHandle, ProviderError>;
    async fn stop_server(&self, handle: &ServerHandle)
        -> Result<(), ProviderError>;

    async fn chat(
        &self,
        req: ChatRequest,
        sink: ChatTokenSink,
        cancel: CancellationToken,
    ) -> Result<ChatSummary, ProviderError>;
}
```

`ChatTokenSink` forwards tokens onto a Tauri event channel with a
monotonic `seq` (see `docs/IPC_CONTRACT.md`). `CancellationToken` is the
backend half of `chat.cancel`; adapters must propagate it to the
underlying HTTP/process so a cancelled call also cancels the upstream
generation. Returning `ChatSummary` lets the provider report finish
reason, token counts, and timings without forcing the caller to inspect
events.

### `ProviderCapabilities`

```rust
pub struct ProviderCapabilities {
    pub streaming: bool,
    pub tool_calls: ToolCallSupport,
    pub vision: bool,
    pub embeddings: bool,
    pub max_context: u32,         // tokens; 0 means "unknown"
    pub owned_process: bool,      // true if Plume spawns the server
}

/// Provisional. Local-model tool calling is a spectrum, not a flag.
/// MVP only uses `None` and `PromptOnly`; the richer variants are
/// reserved so the field shape doesn't churn when capable adapters land.
pub enum ToolCallSupport {
    None,        // model has no tool-call concept
    PromptOnly,  // model emits text; the runtime parses tool intents
    JsonMode,    // model emits JSON; the runtime validates against the tool schema
    Native,      // OpenAI-style function calling end-to-end
}
```

The model picker uses these to decide which UI affordances to show
(tool-call inspector, image attachment, etc.) and to surface honest
capability badges.

### Tool-call slot in `ChatRequest`

```rust
pub struct ChatRequest {
    pub provider_id: ProviderId,
    pub model_id: ModelId,
    pub mode: ChatMode,
    pub instruction: String,
    pub attachments: Vec<Attachment>,
    pub tools: Vec<ToolDescriptor>,        // empty when mode != Agent
}

pub struct ToolDescriptor {
    pub name: String,
    pub description: String,
    pub schema: serde_json::Value,         // JSON Schema for args
}
```

Adapters that report `capabilities().tool_calls == ToolCallSupport::None`
must reject a non-empty `tools` field with `ProviderError::Unsupported`.
MVP only uses `None` and `PromptOnly`; `JsonMode` and `Native` land
when an adapter genuinely supports them.

## Built-in adapters

### Apple Foundation Models

- The only Apple route is `providerId: 'apple-foundation'`,
  `modelId: 'system'`; it has no server handle and never falls back to Qwen.
- Rust resolves only the bundled `apple-model/plume-apple-model` helper. The
  helper receives the already assembled/redacted chat transcript over bounded
  stdin JSON and returns bounded JSON-lines tokens. It has no project paths,
  tool interface, localhost port, or filesystem-browsing authority.
- `providers.appleAvailability` is app-level and needs no project trust. On
  non-macOS or macOS below 26 it returns `os-unsupported` before any helper
  spawn. On supported macOS, the helper's typed availability result is the
  source of truth; its safe short detail may reach the catalog, but stderr and
  local paths never do.
- The stream loop receives through a capacity-64 channel and checks cancel and
  deadline at least every 50 ms. Cancel, deadline, malformed helper output,
  consumer loss, and process error kill and reap the child before one terminal
  `chat/done` event.
- Release packaging builds the arm64 helper and places it at
  `apple-model/plume-apple-model`. The top-bar catalog exposes the adapter before
  a project is open, but availability remains a runtime fact reported by the
  host. Shipped adapter code does not mean every judge Mac can use the model.
- The helper uses only `SystemLanguageModel.default`. Plume has no Private Cloud
  Compute route, no arbitrary Apple model id, no filesystem/tool authority, and
  no computer-use emission.

### MLX-LM

- Primary on Apple Silicon. Best perf-per-watt for the models we care
  about.
- Communicates via the MLX-LM HTTP server (`mlx_lm.server`). The Rust
  adapter spawns it under Plume control and tears it down via the
  process lifecycle rules below.
- Tokenizer / chat-template quirks live inside the adapter, not the
  prompt layer.
- Packaged releases resolve only the generated, identity-checked Python 3.12.13
  runtime containing pinned `mlx-lm` 0.31.3, `mlx` 0.32.0, and `mlx-metal`
  0.32.0. Release never falls back to PATH Python. Debug builds retain an
  explicit override and contributor fallback.
- The fixed catalog entry is **Qwen Coder 1.5B**
  (`mlx-community/Qwen2.5-Coder-1.5B-Instruct-4bit`) at revision
  `b3252a2f97102b1fb1571fec2c9b27219a8536be`, Apache-2.0. The runtime ships in
  the app; the weights do not. A user click starts the pinned, verified,
  resumable download into Application Support.
- Catalog Qwen start is app-level and accepts only its opaque catalog id.
  Arbitrary inventory-model starts remain trusted-project scoped. Both use the
  same bounded MLX supervisor and exact-handle chat route.
- This makes local chat reachable without Ollama or external Python. It does
  not make Qwen a broad tool executor or ship the deeper read/edit/test loop.
- Runtime, model-path, SSE, cancellation, and lifecycle details live in
  [`MLX_RUNTIME.md`](MLX_RUNTIME.md).

### Ollama

- Useful because many users already have it. Speaks the Ollama HTTP API
  (`/api/chat`, `/api/generate`, `/api/tags`).
- Plume does not own the Ollama process — if the user already has the
  daemon running, Plume connects; otherwise Plume offers to start
  `ollama serve` (offer not implemented yet).
- On Mac, Ollama is currently a GGUF/Metal path for most models. The UI
  must say so honestly. Do not present it as MLX.
- Probes shipped in D2: TCP connect to `127.0.0.1:11434` followed by
  `GET /api/tags` over a tiny hand-rolled HTTP/1.1 client (no new
  crate deps; one localhost JSON GET does not justify pulling in
  `reqwest`). The result feeds `ProviderHealth.models`. Failures fall
  back to `models: null` so the panel never claims "0 models" when
  the probe couldn't read a list.
- Probes shipped in D3: lazy per-model `POST /api/show` reading
  `details.{format, family, parameter_size, quantization_level}`,
  `model_info["general.parameter_count"]`, the family-prefixed
  `*.context_length`, and `capabilities`. Surfaces through the new
  `providers.modelDetails` IPC verb plus a cautious fit estimator
  (see `src-tauri/src/providers/fit.rs`).
- Runtime-path label: on macOS the panel says `GGUF / Metal (Ollama)`.
  Ollama serves GGUF through Metal on Mac today, not MLX, and the UI
  must say so honestly — if Ollama's MLX preview becomes default we
  revisit the label in `commands::providers::runtime_path_for`.
- Chat shipped in D7 (sync) and was reshaped to streaming in D7.1:
  `POST /api/chat` with `stream:true`, parsed line-by-line by
  `src-tauri/src/chat/ollama.rs::stream_chat`. Each NDJSON frame's
  `message.content` is a DELTA (not cumulative) — Plume forwards
  it on the `chat/token` Tauri event and the frontend
  concatenates. Cooperative cancel via `chat.cancel(streamId)`
  flips an `AtomicBool` the streaming loop polls every ~200 ms.
  404 maps to a terminal `chat/done { finish: 'error' }`; 5xx and
  transport failures the same. The non-streaming `send_chat`
  adapter is retained `#[cfg(test)]`-only as a reference
  implementation of the protocol.

### LM Studio

- Treated as both a model browser and a local server. Plume connects to
  its OpenAI-compatible HTTP API at `127.0.0.1:1234`.
- Plume does not control LM Studio's process lifecycle.
- Probes shipped in D4: TCP connect to `127.0.0.1:1234` followed by
  `GET /v1/models`. Parser lives in
  `src-tauri/src/providers/openai_compat.rs` and only treats
  `data[].id` as stable. Per-model size and parameter info are not in
  `/v1/models`, so `ProviderModel.size_bytes` stays `None` and the
  panel renders no model-detail expand for LM Studio rows.

### llama.cpp

- Backstop for cross-platform GGUF support.
- Adapter speaks `llama-server`'s HTTP API at `127.0.0.1:8080`
  (the `--host` / `--port` defaults).
- Probes shipped in D4: TCP connect to `127.0.0.1:8080` followed by
  `GET /v1/models`. Shares the OpenAI-compat parser with LM Studio
  (`src-tauri/src/providers/openai_compat.rs`). Shape verified
  against `tools/server/server-models.cpp` in the upstream repo:
  `{ "object": "list", "data": [{ "id", "object": "model",
  "owned_by", "created", "aliases", "tags", "status",
  "architecture" }] }`. We only treat `data[].id` as stable.
- Process supervision (spawning `llama-server` ourselves with a
  lockfile per § Process lifecycle for owned providers) has not
  landed yet; the registry category stays `PlumeManaged` to mark
  the intent, but today a user must have `llama-server` running
  themselves for the probe to find it.

## Process lifecycle for owned providers

MLX-LM is owned by an in-process supervisor. Start reserves one of eight slots
under the registry lock before spawn, launches in a new process group, allocates
an ephemeral loopback port, and returns an opaque handle only after health is
ready. Stop uses SIGINT with a three-second grace and then SIGKILL. Normal app
exit latches the registry closed and sweeps running and mid-start children;
webview reload can re-adopt healthy handles from the live registry.

This ownership is process-local. A Plume hard crash, SIGKILL, or power loss
runs no sweep; persisted-PID adoption across application restarts is not
implemented. Connected adapters such as Ollama remain user-owned.

## Adding a new provider

1. Create `src-tauri/src/providers/<name>.rs` implementing `Provider`.
2. Register it in `providers::registry::default_providers()`.
3. Add a `<Name>Card` in the model picker UI.
4. Document model fit in this file with a short paragraph and a tested
   model example.
5. Add unit tests for the parser/HTTP layer with mocked responses.
6. If the adapter owns its process, add a lockfile teardown integration
   test.

Every call path needs a timeout, a structured error, and a clear UI
state for "provider not running".

## Capability tiers

The model picker classifies models so the UI can recommend defaults on
the two-axis autonomy model from `docs/SAFETY.md` (`agentMode` plus
`approvalPolicy`). Allowlists for `scoped-edit` and `agent-loop` are
always per-task and never inferred from the tier.

| Tier                | Example     | Default `agentMode` | Default `approvalPolicy` |
| ------------------- | ----------- | ------------------- | ------------------------ |
| Tiny / Fast         | 1-3 B       | `chat`              | `ask-each`               |
| Small / Useful      | 4-8 B coder | `propose-diff`      | `ask-each`               |
| Medium / Capable    | 14-32 B Q4  | `scoped-edit`       | `ask-on-write`           |
| Large / Workstation | 35 B+       | `agent-loop`        | `ask-on-fail`            |

These tiers are heuristics, not promises. If a benchmark shows
otherwise, the registry entry overrides the tier. The user can also
re-cross any combination from the picker; defaults exist so a tiny
model is not handed `agent-loop` by accident.

## External agent engines

Codex CLI, Claude Code, and OpenCode are not LLM providers in the
sense the trait above describes — they are full agent runtimes that
already own a planning loop, tool dispatch, diff generation, and
their own model client. Forcing them through `Provider` would fight
their grain.

The plan when this lands is to embed them as *engines*: the engine
owns the agent loop, Plume owns the cockpit. Concretely:

Plume keeps:

- the editor, file tree, and diff viewer,
- the project trust prompt,
- the path / command / patch safety gates — every engine call still
  flows through `safety::guard`,
- the approval ledger,
- the visible UI both humans and computer-use agents drive.

The engine gets:

- the agent loop,
- prompt construction,
- tool-call dispatch and the model client.

**Safety precondition.** External agent runtimes default to raw
filesystem and process access. Codex CLI is essentially `cd <project>
&& work`. Embedding one of them naively would route around
`safety::guard` entirely. The engine track therefore lands only with
one of:

- a brokered tool protocol where Plume intercepts every tool call the
  engine emits, runs the same `fs.* / commands.* / patch.*` checks
  the rest of the app uses, and returns brokered results;
- or an OS-level sandbox (macOS Seatbelt, Linux user namespaces, and
  similar) that gives the engine no direct project-root access.

Engines that require raw cwd access are unsupported until that
isolation exists. The engine track is reserved, not licensed.

Architecturally this is a separate Rust module track from
`providers/`. No `Provider` trait change. No `ChatRequest` overload.
A future `engines/` module will sit alongside with its own trait and
its own IPC verbs (placeholders in `docs/IPC_ROADMAP.md`). Until that
lands, no schema, no IPC, no registry entry is committed for engines,
and the user-visible model picker shows providers only.

Why name this track now: when external agent runtimes are commodity,
Plume's value is not "one more LLM client". It is the calm,
hand-drawn, safety-aware desktop cockpit that can drive any of them
on local files. Keeping the engines split visible in the docs keeps
the provider trait honest about its scope.

## Memory honesty rules

The UI must not show "load model" without first showing the user:

- Estimated memory cost for the chosen quantization and context length.
- Whether the user's machine is likely to fit it (green / amber / red).
- KV cache impact at the chosen context length.

If estimation is unavailable, say so. Do not silently load.

## Speculative decoding (DFlash etc.)

Treated as an optimization layer. The big model still has to fit in
memory. The UI must communicate this and never imply that DFlash makes a
35 B model run on 16 GB of unified memory.
