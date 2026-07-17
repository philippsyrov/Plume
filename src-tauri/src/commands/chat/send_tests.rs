use super::*;

// D120: the outcome/stats helpers below are only re-imported into
// `send` where production code calls them (`*_outcome_to_done`);
// the rest are reached through the sibling module directly, along
// with the wire types their tests construct.
use super::outcome::{
    compute_tokens_per_second, format_chat_error, format_mlx_chat_error, ns_to_ms, translate_stats,
};
use crate::chat::ollama::{ChatError, OllamaFrameStats};
use crate::chat::stream::ChatStreamRegistry;
use crate::chat::{ChatFinish, ChatMessage, ChatRole};
use crate::commands::sessions::SessionScope;
use crate::memory::{self, MemoryRememberResponse, UserMemoryRememberResponse};
use crate::project::trust::TrustStore;
use crate::project::ProjectSession;
use crate::sessions;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

struct CommandTempDir(PathBuf);

impl CommandTempDir {
    fn new(label: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "plume-chat-send-command-{label}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for CommandTempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn command_state(base: &Path) -> AppState {
    AppState {
        session: ProjectSession::default(),
        trust: Mutex::new(TrustStore::load(base.join("trust.json"))),
        chat_streams: Arc::new(ChatStreamRegistry::default()),
        agent_config: Mutex::new(crate::agent::AgentConfig::default()),
        local_sessions_dir: base.join("sessions"),
        user_memory_dir: base.join("memory"),
        catalog_store: Arc::new(crate::providers::catalog::CatalogStore::new(
            base.to_path_buf(),
        )),
        catalog_downloads: Arc::new(
            crate::providers::catalog_download::CatalogDownloadRegistry::default(),
        ),
    }
}

fn user_memory_id(dir: &Path, text: &str) -> String {
    match memory::remember_user_memory(dir, text) {
        UserMemoryRememberResponse::Ok(ok) => ok.entry.id,
        UserMemoryRememberResponse::Err(error) => panic!("remember failed: {}", error.message),
    }
}

fn project_memory_id(root: &Path, text: &str) -> String {
    match memory::remember(root, text) {
        MemoryRememberResponse::Ok(ok) => ok.entry.id,
        MemoryRememberResponse::Err(error) => panic!("remember failed: {}", error.message),
    }
}

fn send_payload(
    include_project_context: bool,
    owner: ChatContextOwner,
    context_sources: Vec<ContextSourceRef>,
) -> ChatSendPayload {
    ChatSendPayload {
        stream_id: "stream-test".into(),
        provider_id: "ollama".into(),
        model_id: "test-model".into(),
        messages: vec![ChatMessage {
            role: ChatRole::User,
            content: "hello".into(),
        }],
        handle_id: None,
        attachment: None,
        context_sources,
        context_owner: Some(owner),
        mode: ChatMode::Chat,
        include_project_context,
    }
}

#[test]
fn real_send_preflight_resolves_mixed_user_and_project_memory_exactly() {
    let td = CommandTempDir::new("mixed-memory");
    let state = command_state(&td.0);
    let project = td.0.join("project");
    fs::create_dir_all(&project).unwrap();
    let project = fs::canonicalize(project).unwrap();
    state.session.open(project.clone());
    state.trust.lock().unwrap().mark_trusted(&project).unwrap();
    let project_session =
        sessions::create(&sessions::project_sessions_dir(&project).unwrap(), None).unwrap();
    let user_id = user_memory_id(&state.user_memory_dir, "user preference");
    let project_id = project_memory_id(&project, "project decision");
    let payload = send_payload(
        true,
        ChatContextOwner {
            scope: SessionScope::Project,
            session_id: project_session.id,
        },
        vec![
            ContextSourceRef::UserMemoryEntry {
                entry_id: user_id.clone(),
            },
            ContextSourceRef::MemoryEntry {
                entry_id: project_id.clone(),
            },
        ],
    );

    let assembled = prepare_chat_send_context(&payload, &state).unwrap();
    assert!(matches!(
        assembled.explicit_context.as_slice(),
        [
            ContextSourceManifestItem::UserMemoryEntry { entry_id: first, .. },
            ContextSourceManifestItem::MemoryEntry { entry_id: second, .. }
        ] if first.as_str() == user_id.as_str() && second.as_str() == project_id.as_str()
    ));
}

#[test]
fn real_send_preflight_rejects_project_only_memory_from_local_chat() {
    let td = CommandTempDir::new("local-project-memory");
    let state = command_state(&td.0);
    let local = sessions::create(&state.local_sessions_dir, None).unwrap();
    let payload = send_payload(
        false,
        ChatContextOwner {
            scope: SessionScope::Local,
            session_id: local.id,
        },
        vec![ContextSourceRef::MemoryEntry {
            entry_id: "m_0123456789abcdef0123456789abcdef".into(),
        }],
    );

    assert!(matches!(
        prepare_chat_send_context(&payload, &state),
        Err(IpcError::NeedsApproval)
    ));
}

#[test]
fn real_send_preflight_resolves_local_user_memory_for_the_exact_owner() {
    let td = CommandTempDir::new("local-user-memory");
    let state = command_state(&td.0);
    let owner = sessions::create(&state.local_sessions_dir, None).unwrap();
    let entry_id = user_memory_id(&state.user_memory_dir, "user preference");
    let payload = send_payload(
        false,
        ChatContextOwner {
            scope: SessionScope::Local,
            session_id: owner.id,
        },
        vec![ContextSourceRef::UserMemoryEntry {
            entry_id: entry_id.clone(),
        }],
    );

    let assembled = prepare_chat_send_context(&payload, &state).unwrap();
    assert!(matches!(
        assembled.explicit_context.as_slice(),
        [ContextSourceManifestItem::UserMemoryEntry { entry_id: id, .. }]
            if id.as_str() == entry_id.as_str()
    ));
}

#[test]
fn real_send_preflight_rejects_missing_or_wrong_local_owner() {
    let td = CommandTempDir::new("owner-errors");
    let state = command_state(&td.0);
    let entry_id = user_memory_id(&state.user_memory_dir, "user preference");
    let sources = vec![ContextSourceRef::UserMemoryEntry { entry_id }];
    let mut missing = send_payload(
        false,
        ChatContextOwner {
            scope: SessionScope::Local,
            session_id: "ignored".into(),
        },
        sources.clone(),
    );
    missing.context_owner = None;
    assert!(matches!(
        prepare_chat_send_context(&missing, &state),
        Err(IpcError::BadArgument(_))
    ));

    let wrong = send_payload(
        false,
        ChatContextOwner {
            scope: SessionScope::Local,
            session_id: "s00000000000000000000000000000000".into(),
        },
        sources,
    );
    assert!(matches!(
        prepare_chat_send_context(&wrong, &state),
        Err(IpcError::NotFound(_))
    ));
}

#[test]
fn chat_send_summaries_serialize_exact_context_manifests() {
    let memory = ChatSendMemorySummary {
        entry_count: 1,
        bytes: 5,
        byte_cap: 4096,
        truncated: false,
        entries: vec![ChatMemoryContextEntry {
            id: "m_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            created_at_ms: 7,
            text_bytes: 5,
            preview: "hello".into(),
        }],
    };
    let topics = ChatSendTopicsSummary {
        file_count: 1,
        bytes: 9,
        byte_cap: 6144,
        truncated: false,
        files: vec![ChatTopicContextFile {
            name: "USER.md".into(),
            bytes: 9,
        }],
    };
    let memory_json = serde_json::to_value(memory).expect("memory must serialize");
    let topics_json = serde_json::to_value(topics).expect("topics must serialize");
    assert_eq!(memory_json["entries"][0]["createdAtMs"], 7);
    assert_eq!(memory_json["entries"][0]["textBytes"], 5);
    assert_eq!(topics_json["files"][0]["name"], "USER.md");
    assert_eq!(topics_json["files"][0]["bytes"], 9);
}

#[test]
fn format_chat_error_carries_through_messages() {
    let e = ChatError::ModelNotFound {
        model: "ghost".into(),
        message: "not pulled".into(),
    };
    let s = format_chat_error(&e);
    assert!(s.contains("ghost"));
    assert!(s.contains("not pulled"));
}

// ---- D9 generation telemetry ----

#[test]
fn translate_stats_passes_counts_and_converts_durations_to_ms() {
    // 18 output tokens generated in exactly 1 s → 18 tok/s.
    // 12 prompt tokens evaluated in 100 ms → prompt_ms == 100.
    let raw = OllamaFrameStats {
        eval_count: Some(18),
        eval_duration_ns: Some(1_000_000_000),
        prompt_eval_count: Some(12),
        prompt_eval_duration_ns: Some(100_000_000),
    };
    let stats = translate_stats(&raw);
    assert_eq!(stats.output_tokens, Some(18));
    assert_eq!(stats.eval_ms, Some(1_000));
    assert_eq!(stats.prompt_tokens, Some(12));
    assert_eq!(stats.prompt_ms, Some(100));
    assert_eq!(stats.tokens_per_second, Some(18.0));
}

#[test]
fn translate_stats_returns_none_fields_when_inputs_absent() {
    // A frame with no telemetry fields produces a stats value
    // where every output is None — the UI hides the footer in
    // that case.
    let stats = translate_stats(&OllamaFrameStats::default());
    assert_eq!(stats.output_tokens, None);
    assert_eq!(stats.eval_ms, None);
    assert_eq!(stats.tokens_per_second, None);
    assert_eq!(stats.prompt_tokens, None);
    assert_eq!(stats.prompt_ms, None);
}

#[test]
fn tokens_per_second_is_none_when_eval_duration_is_zero() {
    // Division by zero would produce inf; we prefer honest
    // "throughput not measurable" by returning None.
    assert_eq!(
        compute_tokens_per_second(Some(10), Some(0)),
        None,
        "zero eval_duration must not produce infinity"
    );
}

#[test]
fn tokens_per_second_is_none_when_either_input_is_none() {
    assert_eq!(compute_tokens_per_second(None, Some(1_000_000)), None);
    assert_eq!(compute_tokens_per_second(Some(5), None), None);
}

#[test]
fn ns_to_ms_floors_sub_millisecond_durations() {
    // 999 µs rounds down to 0 ms; the UI surfaces that as
    // "0 ms" rather than fabricating a 1 ms reading.
    assert_eq!(ns_to_ms(999_000), 0);
    assert_eq!(ns_to_ms(1_000_000), 1);
    assert_eq!(ns_to_ms(1_500_000), 1);
}

// ---- D15: chat.send mode wire shape ----
//
// Pin both directions of the new `mode` field on the wire so
// a future refactor that drops `#[serde(default)]` (= D7.1
// payloads break) or `rename_all = "camelCase"` on `ChatMode`
// (= `proposeDiff` stops parsing) fires a test instead of
// a Codex smoke flag. The `ChatMode` enum itself is unit-
// variant so `rename_all` does cascade — D8's struct-variant
// trap doesn't apply here, but the explicit tests keep the
// contract auditable.

#[test]
fn chat_send_payload_defaults_mode_to_chat_when_omitted() {
    // The wire compatibility win of D15: an existing D7.1
    // frontend that sends no `mode` field still deserialises
    // to a payload where `mode == ChatMode::Chat`. Without
    // the `#[serde(default)]` on the field this would reject
    // and break every older send.
    let json = r#"{
        "streamId": "s",
        "providerId": "ollama",
        "modelId": "llama3",
        "messages": [{"role":"user","content":"hi"}]
    }"#;
    let p: ChatSendPayload = serde_json::from_str(json).expect("omitted mode must default to chat");
    assert!(matches!(p.mode, ChatMode::Chat));
}

#[test]
fn chat_send_payload_accepts_explicit_chat_mode() {
    let json = r#"{
        "streamId": "s",
        "providerId": "ollama",
        "modelId": "llama3",
        "messages": [{"role":"user","content":"hi"}],
        "mode": "chat"
    }"#;
    let p: ChatSendPayload = serde_json::from_str(json).expect("must parse");
    assert!(matches!(p.mode, ChatMode::Chat));
}

#[test]
fn chat_send_payload_accepts_propose_diff_mode_in_camel_case() {
    // The exact wire shape `chat.send` sees when the user
    // flips the chat panel to "Propose diff" mode.
    let json = r#"{
        "streamId": "s",
        "providerId": "ollama",
        "modelId": "llama3",
        "messages": [{"role":"user","content":"rename foo"}],
        "mode": "proposeDiff"
    }"#;
    let p: ChatSendPayload = serde_json::from_str(json).expect("camelCase proposeDiff must parse");
    assert!(matches!(p.mode, ChatMode::ProposeDiff));
}

#[test]
fn chat_send_payload_defaults_project_context_on() {
    let json = r#"{
        "streamId": "s",
        "providerId": "ollama",
        "modelId": "llama3",
        "messages": [{"role":"user","content":"hi"}]
    }"#;
    let p: ChatSendPayload =
        serde_json::from_str(json).expect("missing flag must preserve project chat");
    assert!(p.include_project_context);
}

#[test]
fn chat_send_payload_accepts_project_context_off() {
    let json = r#"{
        "streamId": "s",
        "providerId": "ollama",
        "modelId": "llama3",
        "messages": [{"role":"user","content":"hi"}],
        "includeProjectContext": false
    }"#;
    let p: ChatSendPayload = serde_json::from_str(json).expect("no-project chat flag must parse");
    assert!(!p.include_project_context);
}

#[test]
fn chat_send_payload_defaults_context_sources_empty_and_parses_typed_refs() {
    let legacy: ChatSendPayload = serde_json::from_str(
        r#"{
          "streamId":"s","providerId":"ollama","modelId":"llama3",
          "messages":[{"role":"user","content":"hi"}]
        }"#,
    )
    .unwrap();
    assert!(legacy.context_sources.is_empty());

    let typed: ChatSendPayload = serde_json::from_str(
        r#"{
          "streamId":"s","providerId":"ollama","modelId":"llama3",
          "messages":[{"role":"user","content":"hi"}],
          "contextSources":[
            {"kind":"projectFile","relPath":"src/lib.rs","startLine":2,"endLine":4},
            {"kind":"memoryEntry","entryId":"m_0123456789abcdef0123456789abcdef"},
            {"kind":"userMemoryEntry","entryId":"m_11111111111111111111111111111111"},
            {"kind":"topicFile","name":"topics/testing.md"}
          ]
        }"#,
    )
    .unwrap();
    assert_eq!(typed.context_sources.len(), 4);
    assert!(matches!(
        &typed.context_sources[0],
        ContextSourceRef::ProjectFile {
            rel_path,
            start_line: Some(2),
            end_line: Some(4)
        } if rel_path == "src/lib.rs"
    ));
}

#[test]
fn chat_send_payload_rejects_unknown_mode_variant() {
    // Serde rejects on unknown variant before the handler
    // runs, which surfaces as `IpcError::BadArgument` at the
    // Tauri envelope level. A future mode (`'scopedEdit'`,
    // `'agentLoop'`) is opt-in: until the backend knows
    // about it, the frontend gets a clean rejection rather
    // than a silent "mode dropped" send.
    let json = r#"{
        "streamId": "s",
        "providerId": "ollama",
        "modelId": "llama3",
        "messages": [{"role":"user","content":"hi"}],
        "mode": "scopedEdit"
    }"#;
    let err = serde_json::from_str::<ChatSendPayload>(json).expect_err("unknown mode must reject");
    let msg = err.to_string();
    assert!(
        msg.contains("variant") || msg.contains("scopedEdit"),
        "expected unknown-variant error, got: {msg}"
    );
}

// ---- D45: routing dispatch ----

fn payload_for_route(provider: &str, handle_id: Option<&str>) -> ChatSendPayload {
    ChatSendPayload {
        stream_id: "s".into(),
        provider_id: provider.into(),
        model_id: "m".into(),
        messages: vec![ChatMessage {
            role: crate::chat::ChatRole::User,
            content: "hi".into(),
        }],
        handle_id: handle_id.map(str::to_string),
        attachment: None,
        context_sources: Vec::new(),
        context_owner: None,
        mode: ChatMode::Chat,
        include_project_context: true,
    }
}

#[test]
fn resolve_route_picks_ollama_for_ollama_provider() {
    let route = resolve_route(&payload_for_route("ollama", None)).expect("ollama route ok");
    assert!(matches!(route, ChatRoute::Ollama));
}

#[test]
fn resolve_route_for_ollama_ignores_handle_id_even_when_present() {
    // An over-eager frontend that always sends `handleId` should
    // not break the Ollama path. The id is silently ignored
    // there.
    let route = resolve_route(&payload_for_route("ollama", Some("srv_0000000000000001")))
        .expect("ollama with stray handleId");
    assert!(matches!(route, ChatRoute::Ollama));
}

#[test]
fn resolve_route_rejects_mlx_lm_without_handle_id() {
    let err = resolve_route(&payload_for_route("mlx-lm", None))
        .expect_err("mlx-lm without handleId must reject");
    match err {
        IpcError::BadArgument(s) => {
            assert!(s.contains("handleId"), "msg was: {s}");
            assert!(s.contains("providers.startServer"), "msg was: {s}");
        }
        other => panic!("expected BadArgument, got {other:?}"),
    }
}

#[test]
fn resolve_route_rejects_mlx_lm_with_blank_handle_id() {
    let err = resolve_route(&payload_for_route("mlx-lm", Some("   ")))
        .expect_err("blank handleId must reject");
    match err {
        IpcError::BadArgument(s) => assert!(s.contains("handleId")),
        other => panic!("expected BadArgument, got {other:?}"),
    }
}

#[test]
fn resolve_route_rejects_unknown_handle_id_with_not_found() {
    // A handle id that's well-formed but not in the supervisor
    // registry surfaces as NotFound. The frontend uses the same
    // error to drive "start the server again" — a typed
    // distinction from BadArgument.
    let err = resolve_route(&payload_for_route("mlx-lm", Some("srv_ffffffffffffffff")))
        .expect_err("unknown handle must reject");
    match err {
        IpcError::NotFound(s) => {
            assert!(s.contains("MLX server"), "msg was: {s}");
            assert!(s.contains("srv_ffffffffffffffff"), "msg was: {s}");
        }
        other => panic!("expected NotFound, got {other:?}"),
    }
}

#[test]
fn resolve_route_returns_mlx_with_port_and_model_label_for_registered_handle() {
    // D45 Codex HIGH regression: when the handleId resolves to a
    // registered supervisor entry, `resolve_route` must yield
    // BOTH the port AND the model label the supervisor recorded
    // at spawn — not the IPC payload's `modelId`. The chat
    // adapter sends `model_label` on the wire as the OpenAI
    // `model` field so the server's "model matches loaded"
    // check passes. The positive route is exercised here for
    // the first time; pre-fix, `register_for_test` lived in
    // process.rs without a consumer and clippy's dead-code
    // lint flagged it.
    use crate::providers::mlx_lm::process::register_for_test;
    // Spawn a long-lived no-op child so the registry's `Child`
    // slot has something concrete. /bin/sleep is the same stub
    // the supervisor's own tests use; the child is reaped when
    // we let the registry drop it at process exit.
    let child = std::process::Command::new("/bin/sleep")
        .arg("60")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn sleep");
    let handle_id = register_for_test(54321, child, "/abs/path/to/mlx-folder");
    let payload = payload_for_route("mlx-lm", Some(&handle_id.0));
    let route = resolve_route(&payload).expect("registered handle must route");
    match route {
        ChatRoute::Mlx { port, model_label } => {
            assert_eq!(port, 54321);
            // Critical assertion: the route carries the
            // supervisor's `--model` label, NOT the payload's
            // pretty `modelId`. Without this fix, the wire's
            // `model` field would say "gemma-2b" while the
            // server has `/abs/path/to/mlx-folder` loaded.
            assert_eq!(model_label, "/abs/path/to/mlx-folder");
        }
        other => panic!("expected Mlx route, got {other:?}"),
    }
    // Cleanup: stop_server reaps the child and removes the
    // registry entry. Returns Ok(()) on a successfully-killed
    // child or any Io error — both leave the registry empty.
    let _ = crate::providers::mlx_lm::stop_server(&handle_id);
}

#[test]
fn resolve_route_rejects_unknown_provider() {
    let err =
        resolve_route(&payload_for_route("nope", None)).expect_err("unknown provider must reject");
    match err {
        IpcError::BadArgument(s) => {
            assert!(s.contains("nope"), "msg was: {s}");
            assert!(s.contains("mlx-lm"), "msg was: {s}");
        }
        other => panic!("expected BadArgument, got {other:?}"),
    }
}

#[test]
fn chat_send_payload_defaults_handle_id_to_none() {
    // Backward compat: an older Ollama payload that doesn't
    // include `handleId` must still deserialize.
    let json = r#"{
        "streamId": "s",
        "providerId": "ollama",
        "modelId": "llama3",
        "messages": [{"role":"user","content":"hi"}]
    }"#;
    let p: ChatSendPayload = serde_json::from_str(json).expect("must parse");
    assert!(p.handle_id.is_none());
}

#[test]
fn chat_send_payload_accepts_handle_id_in_camel_case() {
    let json = r#"{
        "streamId": "s",
        "providerId": "mlx-lm",
        "modelId": "gemma-2b",
        "handleId": "srv_0000000000000001",
        "messages": [{"role":"user","content":"hi"}]
    }"#;
    let p: ChatSendPayload = serde_json::from_str(json).expect("must parse");
    assert_eq!(p.handle_id.as_deref(), Some("srv_0000000000000001"));
}

#[test]
fn mlx_outcome_to_done_carries_through_completion_and_prompt_tokens() {
    // D45 stats translation: only the OpenAI-shape fields land.
    // eval_ms / prompt_ms / tokens_per_second stay None because
    // MLX-LM's usage chunk doesn't carry per-phase durations.
    let outcome: Result<mlx_chat::StreamOutcome, mlx_chat::ChatError> =
        Ok(mlx_chat::StreamOutcome::Done {
            model_id: "gemma-2b".into(),
            stats: mlx_chat::MlxFrameStats {
                prompt_tokens: Some(42),
                completion_tokens: Some(7),
            },
        });
    let seq = std::sync::atomic::AtomicU64::new(3);
    let started = Instant::now();
    let done = mlx_outcome_to_done(outcome, "s", "mlx-lm", "gemma-2b", &seq, started);
    assert!(matches!(done.finish, ChatFinish::Stop));
    assert_eq!(done.model_id.as_deref(), Some("gemma-2b"));
    let stats = done.stats.expect("stats present on Stop");
    assert_eq!(stats.prompt_tokens, Some(42));
    assert_eq!(stats.output_tokens, Some(7));
    assert!(stats.eval_ms.is_none());
    assert!(stats.prompt_ms.is_none());
    assert!(stats.tokens_per_second.is_none());
}

#[test]
fn mlx_outcome_to_done_maps_eof_to_length_finish() {
    let outcome = Ok(mlx_chat::StreamOutcome::EofBeforeDone { model_id: None });
    let seq = std::sync::atomic::AtomicU64::new(1);
    let done = mlx_outcome_to_done(outcome, "s", "mlx-lm", "gemma-2b", &seq, Instant::now());
    assert!(matches!(done.finish, ChatFinish::Length));
    // Falls back to the request's model id when the adapter
    // didn't observe one.
    assert_eq!(done.model_id.as_deref(), Some("gemma-2b"));
    assert!(done.stats.is_none());
}

#[test]
fn mlx_outcome_to_done_maps_cancelled_to_cancelled_finish() {
    let outcome = Ok(mlx_chat::StreamOutcome::Cancelled {
        model_id: Some("served-id".into()),
    });
    let seq = std::sync::atomic::AtomicU64::new(0);
    let done = mlx_outcome_to_done(outcome, "s", "mlx-lm", "gemma-2b", &seq, Instant::now());
    assert!(matches!(done.finish, ChatFinish::Cancelled));
    assert_eq!(done.model_id.as_deref(), Some("served-id"));
    assert!(done.stats.is_none());
}

#[test]
fn mlx_outcome_to_done_maps_transport_error_with_useful_message() {
    let err = mlx_chat::ChatError::Transport {
        port: 9999,
        source: std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "refused"),
    };
    let seq = std::sync::atomic::AtomicU64::new(0);
    let done = mlx_outcome_to_done(Err(err), "s", "mlx-lm", "gemma-2b", &seq, Instant::now());
    assert!(matches!(done.finish, ChatFinish::Error));
    let msg = done.error.expect("error message");
    assert!(msg.contains("mlx-lm"), "msg was: {msg}");
    assert!(msg.contains("9999"), "msg was: {msg}");
}

#[test]
fn format_mlx_chat_error_carries_through_messages() {
    let e = mlx_chat::ChatError::ModelNotFound {
        model: "ghost".into(),
        message: "not loaded".into(),
    };
    let s = format_mlx_chat_error(&e);
    assert!(s.contains("ghost"));
    assert!(s.contains("not loaded"));
}
