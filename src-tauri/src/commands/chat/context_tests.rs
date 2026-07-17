use super::*;
use crate::chat::stream::ChatStreamRegistry;
use crate::commands::sessions::SessionScope;
use crate::memory::{self, MemoryRememberResponse, UserMemoryRememberResponse};
use crate::project::trust::TrustStore;
use crate::project::ProjectSession;
use crate::prompts::LineRange;
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
            "plume-chat-context-command-{label}-{}-{nonce}",
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

fn remember_user_id(dir: &Path, text: &str) -> String {
    match memory::remember_user_memory(dir, text) {
        UserMemoryRememberResponse::Ok(ok) => ok.entry.id,
        UserMemoryRememberResponse::Err(error) => panic!("remember failed: {}", error.message),
    }
}

fn remember_project_id(root: &Path, text: &str) -> String {
    match memory::remember(root, text) {
        MemoryRememberResponse::Ok(ok) => ok.entry.id,
        MemoryRememberResponse::Err(error) => panic!("remember failed: {}", error.message),
    }
}

fn context_payload(
    include_project_context: bool,
    owner: Option<ChatContextOwner>,
    context_sources: Vec<ContextSourceRef>,
) -> ChatContextPayload {
    ChatContextPayload {
        provider_id: None,
        model_id: None,
        attachment: None,
        context_sources,
        context_owner: owner,
        include_project_context,
    }
}

#[test]
fn real_context_preflight_resolves_local_user_memory_for_the_exact_owner() {
    let td = CommandTempDir::new("local-user-memory");
    let state = command_state(&td.0);
    let session = sessions::create(&state.local_sessions_dir, None).unwrap();
    let entry_id = remember_user_id(&state.user_memory_dir, "user-level preference");
    let response = tauri::async_runtime::block_on(chat_context_impl(
        context_payload(
            false,
            Some(ChatContextOwner {
                scope: SessionScope::Local,
                session_id: session.id,
            }),
            vec![ContextSourceRef::UserMemoryEntry {
                entry_id: entry_id.clone(),
            }],
        ),
        &state,
    ))
    .unwrap();

    assert!(matches!(
        response.context_sources.as_slice(),
        [ChatContextSourcePreview::Ready {
            source: ContextSourceManifestItem::UserMemoryEntry { entry_id: id, .. }
        }] if id.as_str() == entry_id.as_str()
    ));
}

#[test]
fn real_context_preflight_rejects_missing_or_wrong_local_owner() {
    let td = CommandTempDir::new("owner-errors");
    let state = command_state(&td.0);
    let entry_id = remember_user_id(&state.user_memory_dir, "user-level preference");
    let source = vec![ContextSourceRef::UserMemoryEntry { entry_id }];

    let missing = tauri::async_runtime::block_on(chat_context_impl(
        context_payload(false, None, source.clone()),
        &state,
    ));
    assert!(matches!(missing, Err(IpcError::BadArgument(_))));

    let wrong = tauri::async_runtime::block_on(chat_context_impl(
        context_payload(
            false,
            Some(ChatContextOwner {
                scope: SessionScope::Local,
                session_id: "s00000000000000000000000000000000".into(),
            }),
            source,
        ),
        &state,
    ));
    assert!(matches!(wrong, Err(IpcError::NotFound(_))));
}

#[test]
fn real_context_preflight_resolves_mixed_project_and_user_memory() {
    let td = CommandTempDir::new("mixed-memory");
    let state = command_state(&td.0);
    let project = td.0.join("project");
    fs::create_dir_all(&project).unwrap();
    let project = fs::canonicalize(project).unwrap();
    state.session.open(project.clone());
    state.trust.lock().unwrap().mark_trusted(&project).unwrap();
    let owner = sessions::create(&sessions::project_sessions_dir(&project).unwrap(), None).unwrap();
    let user_id = remember_user_id(&state.user_memory_dir, "user preference");
    let project_id = remember_project_id(&project, "project decision");

    let response = tauri::async_runtime::block_on(chat_context_impl(
        context_payload(
            true,
            Some(ChatContextOwner {
                scope: SessionScope::Project,
                session_id: owner.id,
            }),
            vec![
                ContextSourceRef::UserMemoryEntry {
                    entry_id: user_id.clone(),
                },
                ContextSourceRef::MemoryEntry {
                    entry_id: project_id.clone(),
                },
            ],
        ),
        &state,
    ))
    .unwrap();

    assert!(matches!(
        response.context_sources.as_slice(),
        [
            ChatContextSourcePreview::Ready {
                source: ContextSourceManifestItem::UserMemoryEntry { entry_id: first, .. }
            },
            ChatContextSourcePreview::Ready {
                source: ContextSourceManifestItem::MemoryEntry { entry_id: second, .. }
            }
        ] if first.as_str() == user_id.as_str() && second.as_str() == project_id.as_str()
    ));
}

#[test]
fn real_context_preflight_blocks_project_memory_from_local_chat() {
    let td = CommandTempDir::new("local-project-memory");
    let state = command_state(&td.0);
    let owner = sessions::create(&state.local_sessions_dir, None).unwrap();
    let requested = ContextSourceRef::MemoryEntry {
        entry_id: "m_0123456789abcdef0123456789abcdef".into(),
    };
    let response = tauri::async_runtime::block_on(chat_context_impl(
        context_payload(
            false,
            Some(ChatContextOwner {
                scope: SessionScope::Local,
                session_id: owner.id,
            }),
            vec![requested.clone()],
        ),
        &state,
    ))
    .unwrap();

    assert!(matches!(
        response.context_sources.as_slice(),
        [ChatContextSourcePreview::Blocked {
            source_ref,
            reason: ChatContextBlockReason::NeedsApproval,
            ..
        }] if source_ref == &requested
    ));
}

// ---- D12 chat.context response wire-shape (serde Serialize) ----
//
// Mirrors the `AttachmentPayload` request-side bug on the
// RESPONSE side: serde's `rename_all` on an enum doesn't
// cascade into struct-variant fields when serializing either,
// so the JSON went out as snake_case and TypeScript got
// `undefined` for every field. The tests below assert that
// each field appears in camelCase on the wire AND that the
// snake_case form never leaks through.

#[test]
fn serializes_ready_attachment_preview_with_camelcase_fields() {
    let value = ChatContextAttachmentPreview::Ready {
        rel_path: "docs/BOOTSTRAP.md".into(),
        start_line: Some(1),
        end_line: Some(3),
        original_bytes: 2048,
        redaction_count: 0,
    };
    let json = serde_json::to_string(&value).expect("Ready must serialize");
    // Positive: every camelCase field appears as a JSON key.
    for key in [
        "\"status\"",
        "\"relPath\"",
        "\"startLine\"",
        "\"endLine\"",
        "\"originalBytes\"",
        "\"redactionCount\"",
    ] {
        assert!(
            json.contains(key),
            "Ready JSON must contain {key}; got: {json}"
        );
    }
    // Negative: no snake_case form leaks through. A future
    // refactor that drops the per-field `rename = "..."`
    // annotations would re-introduce the original P2 bug;
    // these assertions fire if that happens.
    for leaked in [
        "\"rel_path\"",
        "\"start_line\"",
        "\"end_line\"",
        "\"original_bytes\"",
        "\"redaction_count\"",
    ] {
        assert!(
            !json.contains(leaked),
            "Ready JSON must NOT contain snake_case {leaked}; got: {json}"
        );
    }
    // Discriminator must be the lowercase "ready" the
    // TypeScript switch statement matches on.
    assert!(
        json.contains("\"status\":\"ready\""),
        "Ready JSON must carry status='ready'; got: {json}"
    );
}

#[test]
fn serializes_ready_attachment_preview_with_null_line_range() {
    // Whole-file attach: `startLine` and `endLine` must be
    // present as JSON `null`, not omitted. The TypeScript shape
    // expects `startLine: number | null` and a missing field
    // would land as `undefined`, breaking the rendered chip.
    let value = ChatContextAttachmentPreview::Ready {
        rel_path: "src/main.rs".into(),
        start_line: None,
        end_line: None,
        original_bytes: 50,
        redaction_count: 0,
    };
    let json = serde_json::to_string(&value).expect("Ready must serialize");
    assert!(
        json.contains("\"startLine\":null"),
        "startLine must serialize as null when whole-file; got: {json}"
    );
    assert!(
        json.contains("\"endLine\":null"),
        "endLine must serialize as null when whole-file; got: {json}"
    );
}

#[test]
fn serializes_blocked_attachment_preview_with_camelcase_fields() {
    let value = ChatContextAttachmentPreview::Blocked {
        rel_path: "src/.env".into(),
        reason: ChatContextBlockReason::Blocked,
        message: ".env is blocked by policy".into(),
    };
    let json = serde_json::to_string(&value).expect("Blocked must serialize");
    for key in ["\"status\"", "\"relPath\"", "\"reason\"", "\"message\""] {
        assert!(
            json.contains(key),
            "Blocked JSON must contain {key}; got: {json}"
        );
    }
    assert!(
        !json.contains("\"rel_path\""),
        "Blocked JSON must NOT contain snake_case rel_path; got: {json}"
    );
    assert!(
        json.contains("\"status\":\"blocked\""),
        "Blocked JSON must carry status='blocked'; got: {json}"
    );
    // The `reason` enum is unit-style; its variants are renamed
    // via the enum-level `rename_all = "camelCase"` (which IS
    // load-bearing for unit enums). Pin the camelCase form.
    assert!(
        json.contains("\"reason\":\"blocked\""),
        "Blocked JSON must carry reason='blocked' camelCase; got: {json}"
    );
}

#[test]
fn serializes_block_reason_variants_in_camel_case() {
    // Pins every `ChatContextBlockReason` variant against the
    // exact wire string the TypeScript `ChatContextBlockReason`
    // union expects. A future enum-level change that drops
    // `rename_all = "camelCase"` would break this.
    let cases = [
        (ChatContextBlockReason::NotFound, "\"notFound\""),
        (ChatContextBlockReason::PathEscape, "\"pathEscape\""),
        (ChatContextBlockReason::Blocked, "\"blocked\""),
        (ChatContextBlockReason::BadArgument, "\"badArgument\""),
        (ChatContextBlockReason::NeedsApproval, "\"needsApproval\""),
        (ChatContextBlockReason::Internal, "\"internal\""),
    ];
    for (variant, expected) in cases {
        let json = serde_json::to_string(&variant).expect("variant must serialize");
        assert_eq!(json, expected, "variant did not serialize to {expected}");
    }
}

#[test]
fn serializes_instructions_preview_with_camelcase_fields() {
    // Struct (not enum) — `rename_all = "camelCase"` DOES
    // cascade here. Pinned for safety so a refactor that
    // accidentally drops the struct-level attribute fires this
    // test rather than silently breaking the UI's AGENTS.md
    // chip.
    let value = ChatContextInstructionsPreview {
        source: "AGENTS.md".into(),
        original_bytes: 1234,
        redaction_count: 2,
    };
    let json = serde_json::to_string(&value).expect("instructions must serialize");
    for key in ["\"source\"", "\"originalBytes\"", "\"redactionCount\""] {
        assert!(
            json.contains(key),
            "instructions JSON must contain {key}; got: {json}"
        );
    }
    for leaked in ["\"original_bytes\"", "\"redaction_count\""] {
        assert!(
            !json.contains(leaked),
            "instructions JSON must NOT contain snake_case {leaked}; got: {json}"
        );
    }
}

#[test]
fn serializes_memory_manifest_with_exact_camelcase_wire_shape() {
    let value = ChatContextMemoryPreview {
        entry_count: 1,
        bytes: 13,
        byte_cap: 4096,
        truncated: false,
        entries: vec![ChatMemoryContextEntry {
            id: "m_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            created_at_ms: 42,
            text_bytes: 13,
            preview: "hello memory".into(),
        }],
    };
    let json = serde_json::to_value(value).expect("memory preview must serialize");
    assert_eq!(json["entries"][0]["createdAtMs"], 42);
    assert_eq!(json["entries"][0]["textBytes"], 13);
    assert_eq!(json["entries"][0]["preview"], "hello memory");
    assert!(json["entries"][0].get("created_at_ms").is_none());
}

#[test]
fn serializes_topic_manifest_with_exact_names_and_bytes() {
    let value = ChatContextTopicsPreview {
        file_count: 2,
        bytes: 12,
        byte_cap: 6144,
        truncated: false,
        files: vec![
            ChatTopicContextFile {
                name: "INDEX.md".into(),
                bytes: 5,
            },
            ChatTopicContextFile {
                name: "SOUL.md".into(),
                bytes: 7,
            },
        ],
    };
    let json = serde_json::to_value(value).expect("topics preview must serialize");
    assert_eq!(json["files"][0]["name"], "INDEX.md");
    assert_eq!(json["files"][0]["bytes"], 5);
    assert_eq!(json["files"][1]["name"], "SOUL.md");
}

// ---- D12: chat.context handler-level mapping ----
//
// The underlying preview behaviour (which paths reject, what
// an AGENTS.md summary looks like, etc.) is pinned by tests in
// `prompts::assemble`. Here we only test the chat-handler-side
// mapping from `AttachmentPreviewOutcome` → wire shape, so the
// mapping table doesn't drift.

#[test]
fn block_reason_for_maps_each_ipc_error_to_its_stable_code() {
    // Each IpcError variant the preview path can produce must
    // map to a distinct, stable `ChatContextBlockReason`. The
    // mapping is part of the wire contract — drift here would
    // silently retag rejections.
    assert!(matches!(
        block_reason_for(&IpcError::NotFound("x".into())),
        ChatContextBlockReason::NotFound
    ));
    assert!(matches!(
        block_reason_for(&IpcError::PathEscape("x".into())),
        ChatContextBlockReason::PathEscape
    ));
    assert!(matches!(
        block_reason_for(&IpcError::Blocked("x".into())),
        ChatContextBlockReason::Blocked
    ));
    assert!(matches!(
        block_reason_for(&IpcError::BadArgument("x".into())),
        ChatContextBlockReason::BadArgument
    ));
    assert!(matches!(
        block_reason_for(&IpcError::NeedsApproval),
        ChatContextBlockReason::NeedsApproval
    ));
    // Variants the preview shouldn't produce today still map
    // to a defined value (Internal) so the wire response never
    // carries an undefined discriminator.
    assert!(matches!(
        block_reason_for(&IpcError::Internal("x".into())),
        ChatContextBlockReason::Internal
    ));
    assert!(matches!(
        block_reason_for(&IpcError::Cancelled),
        ChatContextBlockReason::Internal
    ));
}

#[test]
fn chat_context_attachment_ready_maps_summary_fields_verbatim() {
    // The wire shape echoes the in-Rust summary. We're testing
    // that no field is dropped or transformed — `usize` →
    // `u64` widens cleanly and `LineRange` flattens into the
    // `startLine` / `endLine` pair.
    use crate::prompts::AttachmentRequest;
    let outcome = AttachmentPreviewOutcome::Ready(crate::prompts::AttachmentSummary {
        rel_path: "src/foo.rs".into(),
        original_bytes: 1234,
        redaction_count: 2,
        line_range: Some(LineRange { start: 4, end: 7 }),
    });
    // Use the request type just to exercise the path the
    // handler uses; the helper itself doesn't take a request.
    let _ = AttachmentRequest::ProjectFile {
        rel_path: "src/foo.rs".into(),
        line_range: None,
    };
    match chat_context_attachment_from_outcome(outcome) {
        ChatContextAttachmentPreview::Ready {
            rel_path,
            start_line,
            end_line,
            original_bytes,
            redaction_count,
        } => {
            assert_eq!(rel_path, "src/foo.rs");
            assert_eq!(start_line, Some(4));
            assert_eq!(end_line, Some(7));
            assert_eq!(original_bytes, 1234);
            assert_eq!(redaction_count, 2);
        }
        other => panic!("expected Ready, got {other:?}"),
    }
}

#[test]
fn chat_context_payload_defaults_project_context_on() {
    let payload: ChatContextPayload =
        serde_json::from_str(r#"{}"#).expect("empty preview payload must parse");
    assert!(payload.include_project_context);
}

#[test]
fn chat_context_payload_accepts_project_context_off() {
    let payload: ChatContextPayload = serde_json::from_str(r#"{"includeProjectContext": false}"#)
        .expect("no-project preview flag must parse");
    assert!(!payload.include_project_context);
}

#[test]
fn chat_context_payload_parses_typed_refs_and_preview_serializes_exact_source() {
    let payload: ChatContextPayload = serde_json::from_str(
        r#"{"contextSources":[{"kind":"topicFile","name":"topics/testing.md"}]}"#,
    )
    .unwrap();
    assert_eq!(
        payload.context_sources,
        vec![ContextSourceRef::TopicFile {
            name: "topics/testing.md".into()
        }]
    );
    let value = ChatContextSourcePreview::Ready {
        source: ContextSourceManifestItem::TopicFile {
            name: "topics/testing.md".into(),
            bytes: 42,
        },
    };
    assert_eq!(
        serde_json::to_value(value).unwrap(),
        serde_json::json!({
            "status": "ready",
            "source": {
                "kind": "topicFile",
                "name": "topics/testing.md",
                "bytes": 42
            }
        })
    );
}

#[test]
fn chat_context_payload_and_preview_preserve_user_memory_tag() {
    let payload: ChatContextPayload = serde_json::from_str(
        r#"{"includeProjectContext":false,"contextSources":[{"kind":"userMemoryEntry","entryId":"m_0123456789abcdef0123456789abcdef"}]}"#,
    )
    .unwrap();
    assert_eq!(
        payload.context_sources,
        vec![ContextSourceRef::UserMemoryEntry {
            entry_id: "m_0123456789abcdef0123456789abcdef".into(),
        }]
    );
    let value = ChatContextSourcePreview::Ready {
        source: ContextSourceManifestItem::UserMemoryEntry {
            entry_id: "m_0123456789abcdef0123456789abcdef".into(),
            created_at_ms: 9,
            bytes: 12,
            preview: "user memory".into(),
        },
    };
    assert_eq!(
        serde_json::to_value(value).unwrap(),
        serde_json::json!({
            "status":"ready",
            "source": {
                "kind":"userMemoryEntry",
                "entryId":"m_0123456789abcdef0123456789abcdef",
                "createdAtMs":9,
                "bytes":12,
                "preview":"user memory"
            }
        })
    );
}

#[test]
fn chat_context_payload_and_preview_preserve_exact_screenshot_provenance() {
    let payload: ChatContextPayload = serde_json::from_str(
        r#"{"providerId":"ollama","modelId":"llava","contextSources":[{"kind":"browserScreenshotEvidence","evidenceId":"bs_0123456789abcdef0123456789abcdef"}]}"#,
    )
    .unwrap();
    assert_eq!(payload.provider_id.as_deref(), Some("ollama"));
    assert_eq!(payload.model_id.as_deref(), Some("llava"));
    assert_eq!(
        payload.context_sources,
        vec![ContextSourceRef::BrowserScreenshotEvidence {
            evidence_id: "bs_0123456789abcdef0123456789abcdef".into(),
        }]
    );

    let value = ChatContextSourcePreview::Ready {
        source: ContextSourceManifestItem::BrowserScreenshotEvidence {
            evidence_id: "bs_0123456789abcdef0123456789abcdef".into(),
            source_url: "https://example.com/diagram".into(),
            title: Some("Architecture diagram".into()),
            captured_at_ms: 10,
            width: 1440,
            height: 900,
            bytes: 81_135,
            sha256: "ab".repeat(32),
        },
    };
    assert_eq!(
        serde_json::to_value(value).unwrap(),
        serde_json::json!({
            "status": "ready",
            "source": {
                "kind": "browserScreenshotEvidence",
                "evidenceId": "bs_0123456789abcdef0123456789abcdef",
                "sourceUrl": "https://example.com/diagram",
                "title": "Architecture diagram",
                "capturedAtMs": 10,
                "width": 1440,
                "height": 900,
                "bytes": 81_135,
                "sha256": "ab".repeat(32)
            }
        })
    );
}

#[test]
fn chat_context_attachment_ready_whole_file_has_null_range() {
    // Whole-file attachments (line_range == None) must yield
    // `null` for both `startLine` and `endLine` on the wire so
    // the UI can render `src/foo.rs` without a trailing
    // `:undefined–undefined`.
    let outcome = AttachmentPreviewOutcome::Ready(crate::prompts::AttachmentSummary {
        rel_path: "src/foo.rs".into(),
        original_bytes: 50,
        redaction_count: 0,
        line_range: None,
    });
    match chat_context_attachment_from_outcome(outcome) {
        ChatContextAttachmentPreview::Ready {
            start_line,
            end_line,
            ..
        } => {
            assert_eq!(start_line, None);
            assert_eq!(end_line, None);
        }
        other => panic!("expected Ready, got {other:?}"),
    }
}

#[test]
fn chat_context_attachment_blocked_carries_reason_and_message() {
    // The Blocked variant must surface the IpcError's
    // human-readable text on the wire so the UI can show the
    // same diagnostic `chat.send` would have, without
    // duplicating the mapping.
    let outcome = AttachmentPreviewOutcome::Blocked {
        rel_path: "src/.env".into(),
        error: IpcError::Blocked(".env is blocked by policy".into()),
    };
    match chat_context_attachment_from_outcome(outcome) {
        ChatContextAttachmentPreview::Blocked {
            rel_path,
            reason,
            message,
        } => {
            assert_eq!(rel_path, "src/.env");
            assert!(matches!(reason, ChatContextBlockReason::Blocked));
            assert!(
                message.contains(".env is blocked"),
                "message must echo the IpcError text, got: {message}"
            );
        }
        other => panic!("expected Blocked, got {other:?}"),
    }
}

#[test]
fn chat_context_attachment_blocked_needs_approval_maps_to_typed_reason() {
    // NeedsApproval is the typed reason for "no trusted project,
    // can't read the attachment". The UI flips the chip to a
    // warn-coloured "Trust required" hint based on this code.
    let outcome = AttachmentPreviewOutcome::Blocked {
        rel_path: "anything.rs".into(),
        error: IpcError::NeedsApproval,
    };
    match chat_context_attachment_from_outcome(outcome) {
        ChatContextAttachmentPreview::Blocked { reason, .. } => {
            assert!(matches!(reason, ChatContextBlockReason::NeedsApproval));
        }
        other => panic!("expected Blocked, got {other:?}"),
    }
}
