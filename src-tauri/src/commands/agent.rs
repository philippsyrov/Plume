//! D93 + D96: the `agent.*` IPC family.
//!
//! `agent.dryRun` (D93) returns the deterministic, dev-only event stream
//! from [`crate::agent::dry_run::scripted_dry_run`] — nothing real runs.
//!
//! `agent.singleStep` (D96) is the first *executing* slice: one agent step
//! against the selected, running local MLX model. It sends a propose-diff
//! prompt, takes the model's reply, classifies it
//! ([`crate::agent::single_step::classify_action`]), runs the one safe
//! action — `patch.validate_patch`, which writes nothing — through Plume's
//! real patch path, consults the D83 approval gate
//! ([`crate::agent::approval::decide`]) on whether *applying* would prompt,
//! and returns the typed D85 event stream for the existing `AgentEventLog`
//! to render.
//!
//! The trust gates this slice keeps, verbatim from the design: it never
//! applies a diff (applying is a write — always gated, always paused), it
//! never runs a shell command, it never recurses, and it requires a
//! trusted open project (validate needs a root). The only thing that
//! actually executes is read-only validation.

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tauri::State;

use crate::agent::approval::{decide, ApprovalDecision, ApprovalLedger, ToolRequest};
use crate::agent::dry_run::scripted_dry_run;
use crate::agent::events::AgentEventEnvelope;
use crate::agent::single_step::{
    build_single_step_events, classify_action, mode_allows_step, ProposedAction, ValidateSummary,
};
use crate::agent::AgentMode;
use crate::chat::mlx_lm as mlx_chat;
use crate::chat::{ChatMessage, ChatRole};
use crate::commands::chat::{attachment_to_request, validate_attachment, AttachmentPayload};
use crate::commands::project::AppState;
use crate::error::{IpcError, IpcRequest};
use crate::patch::{validate_patch, PatchValidateResponse};
use crate::project::OpenProject;
use crate::prompts::apply_attachment;
use crate::providers::mlx_lm::{self as mlx_supervisor, ServerHandleId};

/// TCP connect timeout for the MLX round-trip — same 5s the chat path uses.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Overall budget for one step's generation. Generous enough for a 3B/4-bit
/// model to emit a small diff, bounded so a hung server can't stall the
/// command forever (mirrors the chat path's `--max-time` discipline).
const SINGLE_STEP_BUDGET: Duration = Duration::from_secs(120);
/// Upper bound on the user prompt, in bytes. A step is one instruction, not
/// a pasted corpus.
const MAX_PROMPT_BYTES: usize = 8192;

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EmptyPayload {}

#[derive(Debug, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentDryRunResponse {
    pub events: Vec<AgentEventEnvelope>,
}

#[tauri::command]
pub async fn agent_dry_run(req: IpcRequest<EmptyPayload>) -> Result<AgentDryRunResponse, IpcError> {
    req.check_version()?;
    Ok(AgentDryRunResponse {
        events: scripted_dry_run(now_ms()),
    })
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentSingleStepPayload {
    /// The user's instruction for this one step.
    pub prompt: String,
    /// Must be `"mlx-lm"` — the only local provider wired for execution.
    pub provider_id: String,
    /// The pretty inventory id of the selected model. Accepted for parity
    /// with `chat.send` and to keep the wire payload honest about what the
    /// user picked, but unused server-side: the supervisor's launched-model
    /// label (resolved from `handleId`) is what the runtime actually
    /// serves. Hence `allow(dead_code)` — it's a contract field, not a bug.
    #[allow(dead_code)]
    pub model_id: String,
    /// Server handle from `providers.startServer` — the running MLX server
    /// to talk to. Required; a stale/missing handle is a `NotFound`.
    pub handle_id: String,
    /// D99 (optional): a single read-only project-file attachment to fold
    /// into the propose-diff prompt as context, identical in shape and
    /// guards to the chat panel's `chat.send` attachment (`prompts::read`
    /// path: size cap, secret redaction, binary / hardlink / `.git`
    /// blocks, optional D10 line range). Reading it requires the same
    /// trusted project the step already needs, so an attachment without
    /// trust is the same `NeedsApproval` the step returns anyway.
    #[serde(default)]
    pub attachment: Option<AttachmentPayload>,
}

#[derive(Debug, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentSingleStepResponse {
    pub events: Vec<AgentEventEnvelope>,
    /// D100: the model's diff, returned ONLY when it classified as a
    /// propose-diff that PASSED read-only validation — i.e. the diff the
    /// user may now apply. `None` for an invalid diff, a blocked tool
    /// request, or no diff at all (nothing to apply). The single-step
    /// command itself never writes; this hands the validated diff to the
    /// frontend so an explicit user Apply can run it through the existing
    /// `patch.apply` (checkpoint + atomic write + revert) path. The
    /// `MessageChunk` event truncates the reply for the transcript, so the
    /// apply needs the full diff carried here.
    pub applicable_diff: Option<String>,
    /// D126: the tool name from a blocked `TOOL_REQUEST:` reply, so the
    /// frontend can explain the block in its own words instead of parsing
    /// event-log copy. `None` for every other outcome. Carrying it here
    /// changes nothing about the block itself — the request was already
    /// refused (`toolFailed` + `done` in `events`) and nothing ran.
    pub blocked_tool: Option<String>,
}

#[tauri::command]
pub async fn agent_single_step(
    req: IpcRequest<AgentSingleStepPayload>,
    state: State<'_, AppState>,
) -> Result<AgentSingleStepResponse, IpcError> {
    req.check_version()?;
    let payload = req.payload;

    let prompt = payload.prompt.trim().to_string();
    if prompt.is_empty() {
        return Err(IpcError::BadArgument(
            "agent.singleStep: prompt is empty".into(),
        ));
    }
    if prompt.len() > MAX_PROMPT_BYTES {
        return Err(IpcError::BadArgument(format!(
            "agent.singleStep: prompt exceeds {MAX_PROMPT_BYTES} bytes"
        )));
    }
    // Only the local MLX path executes. Ollama is compatibility-only and
    // has no role in the executing agent slice.
    if payload.provider_id != "mlx-lm" {
        return Err(IpcError::BadArgument(format!(
            "agent.singleStep: only the local 'mlx-lm' provider is wired (got '{}')",
            payload.provider_id
        )));
    }
    let handle_raw = payload.handle_id.trim();
    if handle_raw.is_empty() {
        return Err(IpcError::BadArgument(
            "agent.singleStep: handleId is required — start the model first".into(),
        ));
    }
    let info = mlx_supervisor::lookup_handle_info(&ServerHandleId(handle_raw.to_string()))
        .ok_or_else(|| {
            IpcError::NotFound(format!(
                "agent.singleStep: no live MLX server with handleId '{handle_raw}'; start the model and retry"
            ))
        })?;

    // Single-step operates on the open project — `validate_patch` needs a
    // root for path safety. No trusted project ⇒ NeedsApproval, same as
    // the patch commands.
    let project = trusted_open(&state).ok_or(IpcError::NeedsApproval)?;
    let (mode, policy) = {
        let cfg = state
            .agent_config
            .lock()
            .expect("agent_config mutex poisoned");
        (cfg.mode, cfg.approval_policy)
    };

    // The agentMode axis gates *what the model may do*, independently of the
    // approval policy. `chat` is talk-only; asking the model to propose a
    // diff requires `propose-diff` or higher. Refuse before touching the
    // model so the gear selector and the engine can't disagree.
    if !mode_allows_step(mode) {
        return Err(IpcError::BadArgument(format!(
            "agent.singleStep needs agentMode 'propose-diff' or higher; the session is in '{}'. \
             Switch the Agent mode to run a step.",
            agent_mode_wire(mode)
        )));
    }

    // Build the propose-diff prompt, then fold the optional read-only file
    // attachment into the final user message. The fold validates + reads on
    // the SAME path chat.send uses; a blocked / oversize / malformed
    // attachment rejects with the same typed `IpcError`, before the model is
    // ever called. The trusted `project` was resolved above, so the read is
    // scoped to its root.
    let base_messages = build_propose_diff_messages(&prompt);
    let messages = fold_attachment(
        project.root.as_path(),
        base_messages,
        payload.attachment.as_ref(),
    )?;

    // The real model round-trip. Synchronous TCP reader, so it runs on the
    // blocking pool to keep the executor free (same as the chat path).
    let port = info.port;
    let label = info.model_label;
    let reply =
        tauri::async_runtime::spawn_blocking(move || collect_mlx_reply(port, &label, &messages))
            .await
            .map_err(|join_err| {
                IpcError::Internal(format!(
                    "agent.singleStep model task join failed: {join_err}"
                ))
            })?
            .map_err(map_chat_error)?;

    let now_ms = now_ms();
    let action = classify_action(&reply);
    // D100: capture the diff to hand to the frontend for an explicit apply,
    // but ONLY when it validated. The command still writes nothing here.
    let mut applicable = None;
    let events = match &action {
        ProposedAction::ProposeDiff { diff } => {
            // The one safe action: validate (writes nothing). Run it through
            // Plume's real patch path.
            let summary = summarize_validate(validate_patch(project.root.as_path(), diff));
            applicable = applicable_diff(&action, summary.valid);
            // Reuse the D83 gate: applying the diff is a write, which always
            // prompts. Single-step never auto-applies regardless of verdict.
            let apply_decision = decide(
                policy,
                &ToolRequest::Write {
                    path: summary.paths.first().cloned().unwrap_or_default(),
                },
                &ApprovalLedger::new(),
                None,
            );
            build_single_step_events(now_ms, &reply, &action, Some(&summary), apply_decision)
        }
        _ => build_single_step_events(now_ms, &reply, &action, None, ApprovalDecision::Prompt),
    };

    Ok(AgentSingleStepResponse {
        events,
        applicable_diff: applicable,
        blocked_tool: blocked_tool_of(&action),
    })
}

/// D100: the diff the user may apply after this step — `Some` only for a
/// propose-diff action whose diff PASSED validation, so the frontend never
/// offers Apply on an invalid diff, a blocked tool request, or a no-action
/// reply. `patch.apply` re-validates server-side regardless; this is the UI
/// gate, not the security boundary.
fn applicable_diff(action: &ProposedAction, valid: bool) -> Option<String> {
    match action {
        ProposedAction::ProposeDiff { diff } if valid => Some(diff.clone()),
        _ => None,
    }
}

/// D126: the blocked tool's name for the response — `Some` only when the
/// reply was an unsupported `TOOL_REQUEST:`. The `applicable_diff` sibling:
/// a pure action → wire-field mapper the tests can pin without a model.
fn blocked_tool_of(action: &ProposedAction) -> Option<String> {
    match action {
        ProposedAction::UnsupportedTool { name } => Some(name.clone()),
        _ => None,
    }
}

/// The propose-diff steering prompt. Mirrors the contract the D91 smoke
/// uses: reply with ONLY a unified diff, or the documented `TOOL_REQUEST:`
/// sentinel if the model needs a tool it can't express as a diff (which the
/// classifier then surfaces as a blocked event).
fn build_propose_diff_messages(prompt: &str) -> Vec<ChatMessage> {
    let system = "You are Plume's single-step coding agent. The user wants one change to the \
open project. Reply with ONLY a unified diff that makes the change: lines starting with \
'--- ', '+++ ', '@@', a single space for context, '-' for removals, and '+' for additions. \
To change an existing file, use that file's path in both the '--- ' and '+++ ' headers. \
Only when the change requires a brand-new file that does not exist yet, use '--- /dev/null' \
as the old-file header and '+++ b/<path>' with a single all-'+' hunk. \
No prose and no explanation. If you cannot express the change as a diff and need a different \
tool, reply with exactly one line: 'TOOL_REQUEST: <tool_name>'. Only propose-diff is available \
right now; any other tool request will be blocked.";
    vec![
        ChatMessage {
            role: ChatRole::System,
            content: system.to_string(),
        },
        ChatMessage {
            role: ChatRole::User,
            content: prompt.to_string(),
        },
    ]
}

/// D99: fold the optional read-only file attachment into the single-step
/// messages. Runs the SAME shape validation `chat.send` runs
/// (`validate_attachment`: half-range, `startLine: 0`, inverted range,
/// absolute / `..` / oversize path) BEFORE `attachment_to_request` — that
/// converter, and `slice_lines` downstream, both assume validation already
/// happened (a half-range would silently become whole-file; a zero start
/// would underflow the slice). Then reuses `prompts::apply_attachment`
/// (resolve → redact → optional line-range slice on the redacted text →
/// wrap into the final user message). `None` returns the base messages
/// unchanged. Pure over (root, messages, attachment), so the validation +
/// fold are unit-testable without a model or the IPC layer.
fn fold_attachment(
    root: &Path,
    base_messages: Vec<ChatMessage>,
    attachment: Option<&AttachmentPayload>,
) -> Result<Vec<ChatMessage>, IpcError> {
    let Some(att) = attachment else {
        return Ok(base_messages);
    };
    validate_attachment(att)?;
    let (folded, _summary) = apply_attachment(root, &base_messages, attachment_to_request(att))?;
    Ok(folded)
}

/// Drive the MLX chat adapter to completion, accumulating the streamed
/// deltas into the full assistant reply. We don't stream tokens to the UI
/// in this slice — the step returns the assembled event list once — so a
/// simple accumulator is all we need.
fn collect_mlx_reply(
    port: u16,
    model: &str,
    messages: &[ChatMessage],
) -> Result<String, mlx_chat::ChatError> {
    let cancel = Arc::new(AtomicBool::new(false));
    let deadline = Instant::now() + SINGLE_STEP_BUDGET;
    let mut buf = String::new();
    // `Done` and `EofBeforeDone` both leave `buf` holding the text we got;
    // we never set the cancel flag, so `Cancelled` can't happen here.
    mlx_chat::stream_chat(
        port,
        model,
        messages,
        cancel,
        |delta| buf.push_str(delta),
        CONNECT_TIMEOUT,
        deadline,
    )?;
    Ok(buf)
}

/// Flatten a `PatchValidateResponse` into the [`ValidateSummary`] the
/// transcript builder needs. The patch types stay on this (I/O) side so
/// `agent::single_step` remains pure.
fn summarize_validate(resp: PatchValidateResponse) -> ValidateSummary {
    match resp {
        PatchValidateResponse::Ok(ok) => {
            let paths: Vec<String> = ok.touches.iter().map(|t| t.path.clone()).collect();
            let detail = format!(
                "{} file{}, {} hunk{}",
                paths.len(),
                if paths.len() == 1 { "" } else { "s" },
                ok.hunks,
                if ok.hunks == 1 { "" } else { "s" },
            );
            ValidateSummary {
                valid: true,
                paths,
                detail,
            }
        }
        PatchValidateResponse::Err(err) => {
            let detail = err
                .errors
                .first()
                .map(|e| e.message.clone())
                .unwrap_or_else(|| "invalid diff".to_string());
            ValidateSummary {
                valid: false,
                paths: Vec::new(),
                detail,
            }
        }
    }
}

/// Map a chat-adapter transport/protocol error to the IPC contract. A
/// failed round-trip means the local runtime is unreachable or misbehaving,
/// which is exactly `ProviderDown`.
fn map_chat_error(err: mlx_chat::ChatError) -> IpcError {
    IpcError::ProviderDown {
        provider: "mlx-lm".to_string(),
        reason: err.to_string(),
    }
}

/// Returns the currently-open project iff it is also trusted. A local copy
/// of the patch commands' helper (same rationale: avoid sharing a private
/// sibling helper across command modules).
fn trusted_open(state: &AppState) -> Option<OpenProject> {
    let open = state.session.current()?;
    let trusted = {
        let store = state.trust.lock().expect("trust mutex poisoned");
        store.is_trusted(&open.root)
    };
    if trusted {
        Some(open)
    } else {
        None
    }
}

/// The kebab-case wire spelling of an `agentMode`, for error messages
/// (mirrors the serde `rename_all = "kebab-case"` on the enum).
fn agent_mode_wire(mode: AgentMode) -> &'static str {
    match mode {
        AgentMode::Chat => "chat",
        AgentMode::ProposeDiff => "propose-diff",
        AgentMode::ScopedEdit => "scoped-edit",
        AgentMode::AgentLoop => "agent-loop",
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
#[path = "agent_command_tests.rs"]
mod tests;
