//! Chat module: one-shot, non-streaming, read-only.
//!
//! D7 ships the smallest honest chat slice — one selected local model,
//! one user prompt, one assistant response. Nothing about this surface
//! reads files, runs commands, applies patches, or carries a tool-call
//! loop. The IPC contract reserves a streaming `chat.send` (returns a
//! `ChatStreamId`, emits `chat.token` events); D7 deliberately ships
//! the SYNC subset of that and the streaming version is queued as
//! D7.1 — see `docs/IPC_ROADMAP.md § Chat streaming`.
//!
//! Provider boundary today: Ollama only. The Rust side enforces this
//! with `BadArgument` so an external agent that prompts `chat.send`
//! against `lm-studio` or `llama-cpp` gets a clean typed rejection
//! instead of a connection-refused mess.
//!
//! Architectural note: the docs sketch a richer `chat.send` that
//! takes a `ChatRequest` with attachments and a mode, runs through
//! `prompts::assemble`, and reads files via `fs::read_for_prompt`
//! (see `docs/ARCHITECTURE.md § Display reads vs prompt reads`). D7
//! does NOT do that. The user instruction is the entire prompt
//! today — no file content, no template, no system message. When the
//! prompt-assembly path lands, the secret redactor sits between
//! `fs::read_for_prompt` and the adapter; this module's transport
//! layer (`ollama::send_chat`) does not change.

use serde::{Deserialize, Serialize};

pub mod ollama;

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

/// Why a non-streaming chat exchange ended. Mirrors the streaming
/// roadmap's `finish` field (`docs/IPC_CONTRACT.md § chat`) so the
/// frontend types do not have to migrate when streaming lands.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ChatFinish {
    /// Model produced a natural stop. The Ollama adapter maps
    /// `done: true` here regardless of `done_reason` — D7 doesn't
    /// surface the granular reasons yet (`length`, `load`, …).
    Stop,
    /// Model returned but `done` was false. We surface this so the
    /// UI can flag a truncated reply; today only Ollama can produce
    /// it and only in pathological cases.
    Length,
}

/// The full chat-send result Plume returns to the frontend. Carries
/// the assistant message plus enough provenance for the UI to render
/// "model X said Y in N ms" without a second round-trip.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChatResponse {
    /// The assistant message. `role` will always be
    /// `ChatRole::Assistant`; the frontend can treat this as a fact.
    pub message: ChatMessage,
    /// Echoes the provider id from the request for routing.
    pub provider_id: String,
    /// The model id the runtime actually reports it served. Usually
    /// matches the request id verbatim; Ollama can return a slightly
    /// different value (`llama3` → `llama3:latest`) so the UI uses
    /// this to display what was actually used.
    pub model_id: String,
    /// Wall-clock duration of the IPC call as measured on Plume's
    /// side, in milliseconds. Not the same as the model's reported
    /// `eval_duration` — that's an internal nanosecond counter we
    /// don't surface here.
    pub duration_ms: u64,
    pub finish: ChatFinish,
}
