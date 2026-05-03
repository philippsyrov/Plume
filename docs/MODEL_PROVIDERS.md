# Model Providers

Plume treats local model runtimes as swappable engines behind a single
Rust trait. The UI never knows whether tokens came from MLX-LM, Ollama,
LM Studio, or llama.cpp.

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
    pub tool_calls: bool,
    pub vision: bool,
    pub embeddings: bool,
    pub max_context: u32,         // tokens; 0 means "unknown"
    pub owned_process: bool,      // true if Plume spawns the server
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

Adapters that report `capabilities().tool_calls == false` must reject a
non-empty `tools` field with `ProviderError::Unsupported`.

## Built-in adapters

### MLX-LM

- Primary on Apple Silicon. Best perf-per-watt for the models we care
  about.
- Communicates via the MLX-LM HTTP server (`mlx_lm.server`). The Rust
  adapter spawns it under Plume control and tears it down via the
  process lifecycle rules below.
- Tokenizer / chat-template quirks live inside the adapter, not the
  prompt layer.
- Tested model targets: Gemma 4 E2B / E4B MLX, Qwen 2.5 / 3 family MLX,
  MLX community conversions of small DeepSeek-style coder models.

### Ollama

- Useful because many users already have it. Speaks the Ollama HTTP API
  (`/api/chat`, `/api/generate`, `/api/tags`).
- Plume does not own the Ollama process — if the user already has the
  daemon running, Plume connects; otherwise Plume offers to start
  `ollama serve`.
- On Mac, Ollama is currently a GGUF/Metal path for most models. The UI
  must say so honestly. Do not present it as MLX.

### LM Studio

- Treated as both a model browser and a local server. Plume connects to
  its OpenAI-compatible HTTP API.
- Plume does not control LM Studio's process lifecycle.

### llama.cpp

- Backstop for cross-platform GGUF support.
- Adapter speaks `llama-server`'s HTTP API.

## Process lifecycle for owned providers

When `capabilities().owned_process` is true (MLX-LM today), the adapter
must:

1. **Lockfile.** Write `<project>/.plume/providers/<id>.lock` containing
   `{ pid, started_at, model_id }` immediately after spawn.
2. **Adopt-or-kill on startup.** Before spawning a new server, check the
   lockfile. If the PID is still alive and was started by Plume (verify
   via the recorded start time matching `/proc` or `ps`), adopt it.
   Otherwise stale-PID: remove and spawn fresh.
3. **Best-effort teardown.** On normal Plume exit, send SIGTERM, wait
   up to 5 s, then SIGKILL. On crash this won't run, which is exactly
   why step 2 exists.
4. **Single-server-per-(project, provider).** Switching models stops
   the old server before starting the new one; the lockfile blocks
   concurrent spawns from a second Plume window on the same project.

Adapters that don't own the process skip lockfile work and rely on the
underlying daemon's own state.

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

The model picker classifies models so the UI can recommend a default
agent mode (see `docs/SAFETY.md` for what each stage allows).

| Tier                 | Example          | Default mode            |
| -------------------- | ---------------- | ----------------------- |
| Tiny / Fast          | 1-3 B            | Stage 1 chat            |
| Small / Useful       | 4-8 B coder      | Stage 2 propose diff    |
| Medium / Capable     | 14-32 B Q4       | Stage 3 scoped edit     |
| Large / Workstation  | 35 B+            | Stage 4 manual approval |

These tiers are heuristics, not promises. If a benchmark shows
otherwise, the registry entry overrides the tier.

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
