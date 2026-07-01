//! Chat module: read-only, streaming chat transport (Ollama + Plume-
//! managed MLX-LM as of D45).
//!
//! D7 shipped the smallest honest chat slice — one selected local
//! model, one user prompt, one assistant response, no streaming.
//! D7.1 layers token-by-token streaming + cooperative cancel on top.
//! Nothing about this surface reads files, runs commands, applies
//! patches, or carries a tool-call loop yet.
//!
//! The IPC `chat.send` verb now returns a `ChatStreamId` immediately
//! and emits Tauri events as tokens arrive — see
//! `docs/IPC_CONTRACT.md § chat`. `chat.cancel(streamId)` is the
//! companion verb; it sets a cancel flag the streaming loop checks
//! between line reads. Best-effort cancellation only: the underlying
//! blocking HTTP read can still buffer one more line in-flight
//! before the loop notices the flag. The terminal `chat.done` event
//! always fires, with `finish: 'cancelled'` in that case.
//!
//! Provider boundary today: `ollama` and `mlx-lm` (D45). The Rust
//! side enforces this with `BadArgument` so an external agent that
//! prompts `chat.send` against `lm-studio` or `llama-cpp` gets a
//! clean typed rejection instead of a connection-refused mess.
//!
//! Architectural note: `commands::chat::send` runs every prompt
//! through `prompts::assemble` (attachments, AGENTS.md, memory)
//! before either adapter sees it (see
//! `docs/ARCHITECTURE.md § Display reads vs prompt reads`) — the
//! secret redactor sits between `fs::read_for_prompt` and the
//! adapter. This module stays the transport layer only: message
//! types, the `mlx_lm` / `ollama` / `openai_sse` adapters, and the
//! stream registry; it does not itself assemble prompts.
//!
//! The synchronous `ollama::send_chat` from D7 is retained for tests
//! and as a reference implementation, but the shipping IPC path now
//! goes through `ollama::stream_chat`.

use serde::{Deserialize, Serialize};

pub mod mlx_lm;
pub mod ollama;
pub mod openai_sse;
pub mod stream;

/// Adapter-neutral chat message. The wire shape matches Ollama's
/// `/api/chat` message verbatim because that's the only adapter
/// D7 supports. When LM Studio / OpenAI-compatible chat ships, the
/// adapter is responsible for mapping our types onto its endpoint;
/// the contract type does not change.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
}

/// Roles we accept on the wire. `Tool` is reserved for the tool-call
/// loop that a later slice will land; today the handler rejects
/// `Tool` messages so prompts can't sneak tool history into a session
/// that has no tool runtime. `System` is allowed because it's
/// trivial-shaped and lets the frontend (or, later, the prompt
/// assembler) prepend a steering message.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    System,
    User,
    Assistant,
    Tool,
}

/// Why a chat exchange ended. Carried in `chat.done` events and in
/// the legacy synchronous `ChatResponse`.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ChatFinish {
    /// Model produced a natural stop. The Ollama adapter maps
    /// `done: true` here regardless of `done_reason` — D7 doesn't
    /// surface the granular reasons yet (`length`, `load`, …).
    Stop,
    /// Model returned but `done` was false. We surface this so the
    /// UI can flag a truncated reply.
    Length,
    /// User invoked `chat.cancel` before the stream finished.
    Cancelled,
    /// Transport / parse failure mid-stream. The error message is
    /// carried in the same `chat.done` payload.
    Error,
}

/// The full chat-send result the legacy non-streaming path returns.
/// Kept for tests and as a reference implementation; the shipping
/// IPC path goes through the streaming adapter and emits events
/// instead of returning this value. `#[cfg(test)]`-gated so the
/// production binary stays lean.
#[cfg(test)]
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChatResponse {
    /// The assistant message. `role` will always be
    /// `ChatRole::Assistant`; the frontend can treat this as a fact.
    pub message: ChatMessage,
    /// Echoes the provider id from the request for routing.
    pub provider_id: String,
    /// The model id the runtime actually reports it served.
    pub model_id: String,
    /// Wall-clock duration of the IPC call as measured on Plume's
    /// side, in milliseconds.
    pub duration_ms: u64,
    pub finish: ChatFinish,
}

/// `chat.token` event payload. Emitted for each NDJSON frame the
/// runtime produces. `delta` is exactly what the runtime sent —
/// Ollama's protocol guarantees per-frame deltas, not cumulative
/// content, so the frontend just concatenates.
///
/// `seq` is monotonic per stream id, starting at 0. Frontend uses
/// it to detect dropped or reordered events per
/// `docs/IPC_CONTRACT.md § Event sequencing`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChatTokenEvent {
    pub id: String,
    pub seq: u64,
    pub delta: String,
}

/// `chat.done` event payload — terminal event for a stream. Exactly
/// one of these fires per stream id, after which the id is invalid
/// and any further `chat.cancel(id)` is a no-op.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChatDoneEvent {
    pub id: String,
    pub seq: u64,
    pub finish: ChatFinish,
    /// Model id the runtime reported it served. `None` when the
    /// stream errored before reading any frame (e.g. 404 on the
    /// initial HTTP response — we don't know what Ollama would have
    /// labelled the call). Echoes the request id otherwise.
    pub model_id: Option<String>,
    /// Wall-clock duration of the call on Plume's side, in
    /// milliseconds. Even on `error` / `cancelled` this is the
    /// time spent before the stream terminated.
    pub duration_ms: u64,
    /// Human-readable error string when `finish == Error`; empty
    /// otherwise. Frontend renders this verbatim in the transcript.
    pub error: Option<String>,
    /// D9: generation telemetry from the runtime's final frame —
    /// model-reported token counts and per-phase durations.
    /// Populated only on `finish == 'stop'` for Ollama, where the
    /// `done: true` frame carries `eval_count`, `eval_duration`,
    /// `prompt_eval_count`, and `prompt_eval_duration`. `None` on
    /// every other finish reason (cancelled, length, error) —
    /// those paths terminate before the runtime sends metrics.
    pub stats: Option<ChatStats>,
}

/// Provider-neutral generation telemetry surfaced through
/// `chat.done`. Every field is `Option<...>` so a runtime that
/// reports only a subset (or a future provider whose protocol
/// names them differently) can still fill what it has without
/// forcing the others to lie about zero.
///
/// Today the Ollama adapter populates this; the field names are
/// deliberately not Ollama-specific so the LM Studio / llama.cpp
/// chat paths can map their own telemetry onto the same shape when
/// they land. Durations are surfaced in milliseconds (the same
/// unit as `duration_ms` above) rather than the nanosecond
/// integers Ollama uses on the wire — millisecond resolution is
/// what the UI renders and what tests assert on without floating
/// past sub-tick precision.
///
/// `f32` is intentional for `tokens_per_second`: the value is for
/// display, and `f32` matches typical "18.4 tok/s" precision
/// without buying floating-point baggage that `f64` doesn't earn
/// for telemetry like this.
#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChatStats {
    /// Tokens in the assistant reply (`eval_count` on Ollama).
    pub output_tokens: Option<u64>,
    /// Generation phase wall-clock duration in milliseconds —
    /// derived from `eval_duration` for Ollama.
    pub eval_ms: Option<u64>,
    /// Generation throughput, computed by the backend from
    /// `output_tokens / eval_ms` so the frontend doesn't have to
    /// duplicate the formula. `None` when either input is missing
    /// or zero.
    pub tokens_per_second: Option<f32>,
    /// Tokens in the input prompt as evaluated
    /// (`prompt_eval_count` on Ollama). Useful for explaining why
    /// first-token latency is large on long prompts.
    pub prompt_tokens: Option<u64>,
    /// Prompt-evaluation duration in milliseconds — derived from
    /// `prompt_eval_duration` for Ollama.
    pub prompt_ms: Option<u64>,
}
