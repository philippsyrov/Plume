use super::*;
use crate::memory::{self, MemoryRememberResponse};
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
