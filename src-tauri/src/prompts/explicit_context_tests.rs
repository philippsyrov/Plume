use super::*;
use crate::browser::evidence::{store_text_evidence, BrowserCaptureKind, CapturedBrowserText};
use crate::browser::local_evidence::{store_local_text_evidence, LocalEvidenceOwner};
use crate::browser::screenshot_evidence::{store_screenshot_evidence, CapturedBrowserScreenshot};
use crate::memory::{
    self, MemoryRememberResponse, UserMemoryForgetResponse, UserMemoryRememberResponse,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "plume-explicit-context-{label}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn root(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn remember_id(root: &Path, text: &str) -> String {
    match memory::remember(root, text) {
        MemoryRememberResponse::Ok(ok) => ok.entry.id,
        MemoryRememberResponse::Err(error) => panic!("remember failed: {}", error.message),
    }
}

fn remember_user_id(user_memory_dir: &Path, text: &str) -> String {
    match memory::remember_user_memory(user_memory_dir, text) {
        UserMemoryRememberResponse::Ok(ok) => ok.entry.id,
        UserMemoryRememberResponse::Err(error) => {
            panic!("remember user memory failed: {}", error.message)
        }
    }
}

fn file(path: &str, start: Option<u32>, end: Option<u32>) -> ContextSourceRef {
    ContextSourceRef::ProjectFile {
        rel_path: path.into(),
        start_line: start,
        end_line: end,
    }
}

#[test]
fn dedupe_preserves_first_insertion_and_range_is_identity() {
    let refs = vec![
        file("src/main.rs", None, None),
        file("src/main.rs", Some(1), Some(2)),
        file("src/main.rs", None, None),
    ];
    assert_eq!(validate_context_source_refs(&refs).unwrap(), refs[..2]);
}

#[test]
fn source_count_and_shapes_are_bounded() {
    let refs = (0..17)
        .map(|index| file(&format!("src/{index}.rs"), None, None))
        .collect::<Vec<_>>();
    assert!(matches!(
        validate_context_source_refs(&refs),
        Err(IpcError::BadArgument(_))
    ));
    assert!(matches!(
        validate_context_source_refs(&[file("../escape", None, None)]),
        Err(IpcError::BadArgument(_))
    ));
    assert!(matches!(
        validate_context_source_refs(&[ContextSourceRef::TopicFile {
            name: "topics/nested/no.md".into()
        }]),
        Err(IpcError::BadArgument(_))
    ));
}

#[test]
fn wire_shapes_are_camel_case_and_tagged() {
    let source = ContextSourceRef::MemoryEntry {
        entry_id: "m_0123456789abcdef0123456789abcdef".into(),
    };
    assert_eq!(
        serde_json::to_value(source).unwrap(),
        serde_json::json!({
            "kind": "memoryEntry",
            "entryId": "m_0123456789abcdef0123456789abcdef"
        })
    );
}

#[test]
fn user_memory_wire_shape_is_distinct_from_project_memory() {
    let source = ContextSourceRef::UserMemoryEntry {
        entry_id: "m_0123456789abcdef0123456789abcdef".into(),
    };
    assert_eq!(
        serde_json::to_value(source).unwrap(),
        serde_json::json!({
            "kind": "userMemoryEntry",
            "entryId": "m_0123456789abcdef0123456789abcdef"
        })
    );
}

#[test]
fn user_memory_resolves_for_local_and_project_without_becoming_ambient() {
    let td = TempDir::new("user-memory-owners");
    let project_root = td.root().join("project");
    let user_memory_dir = td.root().join("app-data/memory");
    fs::create_dir_all(&project_root).unwrap();
    let entry_id = remember_user_id(&user_memory_dir, "prefers concise explanations");
    let source = ContextSourceRef::UserMemoryEntry {
        entry_id: entry_id.clone(),
    };

    let local_stores = ExplicitContextStores {
        project_root: None,
        user_memory_dir: &user_memory_dir,
        local_browser_owner: None,
    };
    let local =
        resolve_explicit_context_for_send_with_stores(local_stores, std::slice::from_ref(&source))
            .unwrap();
    assert!(local
        .system_message
        .as_deref()
        .unwrap()
        .contains("concise explanations"));
    assert!(local.explicit_memory_ids.is_empty());

    let project_stores = ExplicitContextStores {
        project_root: Some(&project_root),
        user_memory_dir: &user_memory_dir,
        local_browser_owner: None,
    };
    let project = resolve_explicit_context_for_send_with_stores(
        project_stores,
        std::slice::from_ref(&source),
    )
    .unwrap();
    let preview = resolve_explicit_context_for_preview_with_stores(
        project_stores,
        std::slice::from_ref(&source),
    );
    assert!(matches!(
        &project.manifest[0],
        ContextSourceManifestItem::UserMemoryEntry { entry_id: id, .. } if id == &entry_id
    ));
    assert!(matches!(
        &preview[0],
        ContextSourcePreviewOutcome::Ready(item) if item == &project.manifest[0]
    ));

    assert!(matches!(
        memory::forget_user_memory(&user_memory_dir, &entry_id),
        UserMemoryForgetResponse::Ok(_)
    ));
    assert!(matches!(
        resolve_explicit_context_for_send_with_stores(local_stores, &[source]),
        Err(IpcError::NotFound(_))
    ));
}

#[test]
fn browser_evidence_ref_wire_shape_and_id_validation_are_strict() {
    let source = ContextSourceRef::BrowserTextEvidence {
        evidence_id: "be_0123456789abcdef0123456789abcdef".into(),
    };
    assert_eq!(
        serde_json::to_value(source).unwrap(),
        serde_json::json!({
            "kind": "browserTextEvidence",
            "evidenceId": "be_0123456789abcdef0123456789abcdef"
        })
    );
    for evidence_id in [
        "be_short",
        "m_0123456789abcdef0123456789abcdef",
        "be_0123456789abcdef0123456789abcdeg",
    ] {
        assert!(
            validate_context_source_refs(&[ContextSourceRef::BrowserTextEvidence {
                evidence_id: evidence_id.into(),
            }])
            .is_err()
        );
    }
}

#[test]
fn missing_trust_blocks_every_preview_item() {
    let outcomes = resolve_explicit_context_for_preview(
        None,
        &[ContextSourceRef::MemoryEntry {
            entry_id: "m_0123456789abcdef0123456789abcdef".into(),
        }],
    );
    assert!(matches!(
        &outcomes[0],
        ContextSourcePreviewOutcome::Blocked {
            error: IpcError::NeedsApproval,
            ..
        }
    ));
}

#[test]
fn send_resolves_ordered_file_memory_and_topic_with_exact_manifest() {
    let td = TempDir::new("resolve-all");
    let root = fs::canonicalize(td.root()).unwrap();
    fs::create_dir_all(td.root().join("src")).unwrap();
    fs::write(
        td.root().join("src/lib.rs"),
        "first\nsk-test-secret-value-1234567890\nthird\n",
    )
    .unwrap();
    let memory_id = remember_id(&root, "prefer focused integration tests");
    fs::create_dir_all(td.root().join(".plume/memory/topics")).unwrap();
    fs::write(
        td.root().join(".plume/memory/topics/architecture.md"),
        "local-first architecture",
    )
    .unwrap();

    let refs = vec![
        file("src/lib.rs", Some(2), Some(3)),
        ContextSourceRef::MemoryEntry {
            entry_id: memory_id.clone(),
        },
        ContextSourceRef::TopicFile {
            name: "topics/architecture.md".into(),
        },
    ];
    let resolved = resolve_explicit_context_for_send(Some(&root), &refs).unwrap();

    assert_eq!(resolved.manifest.len(), 3);
    assert!(matches!(
        &resolved.manifest[0],
        ContextSourceManifestItem::ProjectFile {
            rel_path,
            start_line: Some(2),
            end_line: Some(3),
            redaction_count: 1,
            ..
        } if rel_path == "src/lib.rs"
    ));
    assert!(matches!(
        &resolved.manifest[1],
        ContextSourceManifestItem::MemoryEntry { entry_id, .. } if entry_id == &memory_id
    ));
    assert!(matches!(
        &resolved.manifest[2],
        ContextSourceManifestItem::TopicFile { name, .. }
            if name == "topics/architecture.md"
    ));
    let prompt = resolved.system_message.unwrap();
    assert!(prompt.contains("[REDACTED:"));
    assert!(!prompt.contains("sk-test-secret-value"));
    assert!(prompt.contains("prefer focused integration tests"));
    assert!(prompt.contains("local-first architecture"));
    assert_eq!(resolved.explicit_memory_ids, HashSet::from([memory_id]));

    let preview = resolve_explicit_context_for_preview(Some(&root), &refs);
    let preview_manifest = preview
        .into_iter()
        .map(|outcome| match outcome {
            ContextSourcePreviewOutcome::Ready(item) => item,
            ContextSourcePreviewOutcome::Blocked { error, .. } => {
                panic!("unexpected blocked preview: {error}")
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(preview_manifest, resolved.manifest);
}

#[test]
fn browser_evidence_preview_and_send_share_exact_immutable_manifest() {
    let td = TempDir::new("browser-evidence");
    let root = fs::canonicalize(td.root()).unwrap();
    let stored = store_text_evidence(
        &root,
        CapturedBrowserText {
            capture_kind: BrowserCaptureKind::Selection,
            source_url: "https://example.com/research".into(),
            title: Some("Research page".into()),
            content: "Selected evidence".into(),
            source_truncated: true,
        },
    )
    .unwrap();
    let refs = [ContextSourceRef::BrowserTextEvidence {
        evidence_id: stored.evidence_id.clone(),
    }];

    let sent = resolve_explicit_context_for_send(Some(&root), &refs).unwrap();
    assert_eq!(sent.manifest.len(), 1);
    assert!(matches!(
        &sent.manifest[0],
        ContextSourceManifestItem::BrowserTextEvidence {
            evidence_id,
            capture_kind: BrowserCaptureKind::Selection,
            source_url,
            title: Some(title),
            bytes: 17,
            truncated: true,
            preview,
            ..
        } if evidence_id == &stored.evidence_id
            && source_url == "https://example.com/research"
            && title == "Research page"
            && preview == "Selected evidence"
    ));
    let prompt = sent.system_message.unwrap();
    assert!(prompt.contains("Selected evidence"));
    assert!(prompt.contains("https://example.com/research"));

    let preview = resolve_explicit_context_for_preview(Some(&root), &refs);
    assert!(matches!(
        &preview[0],
        ContextSourcePreviewOutcome::Ready(item) if item == &sent.manifest[0]
    ));
}

#[test]
fn local_browser_evidence_resolves_only_for_its_exact_session_owner() {
    let td = TempDir::new("local-browser-owner");
    let local_sessions_dir = crate::sessions::local_sessions_dir(td.root());
    let owner = crate::sessions::create(&local_sessions_dir, Some("owner")).unwrap();
    let foreign = crate::sessions::create(&local_sessions_dir, Some("foreign")).unwrap();
    let stored = store_local_text_evidence(
        &local_sessions_dir,
        &LocalEvidenceOwner {
            session_id: owner.id.clone(),
        },
        CapturedBrowserText {
            capture_kind: BrowserCaptureKind::Page,
            source_url: "https://example.com/local".into(),
            title: Some("Local evidence".into()),
            content: "owned by one casual chat".into(),
            source_truncated: false,
        },
    )
    .unwrap();
    let source = ContextSourceRef::BrowserTextEvidence {
        evidence_id: stored.evidence_id,
    };

    let resolved = resolve_explicit_context_for_send_with_local_owner(
        None,
        Some((&local_sessions_dir, &owner.id)),
        std::slice::from_ref(&source),
    )
    .unwrap();
    assert!(resolved
        .system_message
        .unwrap()
        .contains("owned by one casual chat"));

    assert!(matches!(
        resolve_explicit_context_for_send_with_local_owner(
            None,
            Some((&local_sessions_dir, &foreign.id)),
            &[source]
        ),
        Err(IpcError::NotFound(_))
    ));
}

#[test]
fn missing_browser_evidence_is_not_found_without_refetching_the_page() {
    let td = TempDir::new("missing-browser-evidence");
    let root = fs::canonicalize(td.root()).unwrap();
    let result = resolve_explicit_context_for_send(
        Some(&root),
        &[ContextSourceRef::BrowserTextEvidence {
            evidence_id: "be_0123456789abcdef0123456789abcdef".into(),
        }],
    );
    assert!(matches!(result, Err(IpcError::NotFound(_))));
}

#[test]
fn screenshot_evidence_is_manifested_exactly_and_stays_out_of_text_budget() {
    let td = TempDir::new("browser-screenshot-evidence");
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, 800, 600);
        encoder.set_color(png::ColorType::Grayscale);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&vec![0; 800 * 600]).unwrap();
    }
    let stored = store_screenshot_evidence(
        &td.path,
        CapturedBrowserScreenshot {
            source_url: "https://example.com/page?private=yes".into(),
            title: Some("Example".into()),
            png_bytes: png.clone(),
            width: 800,
            height: 600,
        },
    )
    .unwrap();
    let source = ContextSourceRef::BrowserScreenshotEvidence {
        evidence_id: stored.evidence_id.clone(),
    };

    let resolved = resolve_explicit_context_for_send(Some(&td.path), &[source]).unwrap();

    assert!(resolved.system_message.is_none());
    assert_eq!(resolved.images.len(), 1);
    assert_eq!(resolved.images[0].evidence_id, stored.evidence_id);
    assert_eq!(resolved.images[0].png_bytes, png);
    assert!(matches!(
        &resolved.manifest[0],
        ContextSourceManifestItem::BrowserScreenshotEvidence {
            evidence_id,
            source_url,
            title: Some(title),
            width: 800,
            height: 600,
            bytes,
            sha256,
            ..
        } if evidence_id == &stored.evidence_id
            && source_url == "https://example.com/page"
            && title == "Example"
            && *bytes == stored.bytes
            && sha256 == &stored.sha256
    ));
}

#[test]
fn aggregate_budget_rejects_send_and_marks_overflowing_preview_item() {
    let td = TempDir::new("aggregate-cap");
    let root = fs::canonicalize(td.root()).unwrap();
    let chunk = "x".repeat(140 * 1024);
    fs::write(td.root().join("one.txt"), &chunk).unwrap();
    fs::write(td.root().join("two.txt"), &chunk).unwrap();
    let refs = vec![file("one.txt", None, None), file("two.txt", None, None)];

    assert!(matches!(
        resolve_explicit_context_for_send(Some(&root), &refs),
        Err(IpcError::BadArgument(message)) if message.contains("cap")
    ));
    let preview = resolve_explicit_context_for_preview(Some(&root), &refs);
    assert!(matches!(preview[0], ContextSourcePreviewOutcome::Ready(_)));
    assert!(matches!(
        &preview[1],
        ContextSourcePreviewOutcome::Blocked {
            error: IpcError::BadArgument(message),
            ..
        } if message.contains("cap")
    ));
}
