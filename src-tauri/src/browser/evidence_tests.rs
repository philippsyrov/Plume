use std::fs;
use std::path::{Path, PathBuf};

use super::evidence::{
    read_text_evidence, store_text_evidence, BrowserCaptureKind, BrowserEvidenceRecord,
    CapturedBrowserText, BROWSER_EVIDENCE_MAX_RECORDS, BROWSER_EVIDENCE_TOTAL_BYTE_CAP,
    BROWSER_PAGE_BYTE_CAP, BROWSER_SELECTION_BYTE_CAP,
};

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "plume-browser-evidence-{label}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn capture(kind: BrowserCaptureKind, content: impl Into<String>) -> CapturedBrowserText {
    CapturedBrowserText {
        capture_kind: kind,
        source_url: "https://example.com/page?source=test".into(),
        title: Some("Example page".into()),
        content: content.into(),
        source_truncated: false,
    }
}

#[test]
fn round_trip_mints_an_opaque_id_and_returns_only_redacted_metadata() {
    let td = TempDir::new("round-trip");
    let secret = format!("hello sk-{} world", "a".repeat(24));
    let mut input = capture(BrowserCaptureKind::Selection, secret);
    input.title = Some(format!("token sk-{}", "b".repeat(24)));

    let summary = store_text_evidence(td.path(), input).expect("store evidence");

    assert_eq!(summary.evidence_id.len(), 35);
    assert!(summary.evidence_id.starts_with("be_"));
    assert!(summary.evidence_id[3..]
        .bytes()
        .all(|byte| byte.is_ascii_hexdigit()));
    assert_eq!(summary.redaction_count, 2);
    assert_eq!(summary.source_url, "https://example.com/page");
    assert!(summary.preview.contains("[REDACTED:api-key]"));
    let stored = read_text_evidence(td.path(), &summary.evidence_id)
        .unwrap()
        .expect("stored record");
    assert_eq!(stored.id, summary.evidence_id);
    assert!(stored.content.contains("[REDACTED:api-key]"));
    assert!(!stored.content.contains(&"a".repeat(24)));
    assert!(stored.title.unwrap().contains("[REDACTED:api-key]"));
    assert_eq!(stored.bytes, stored.content.len() as u64);
}

#[test]
fn provenance_drops_query_fragment_and_redacts_secret_shaped_paths() {
    let td = TempDir::new("url-provenance");
    let secret = format!("sk-{}", "c".repeat(24));
    let mut input = capture(BrowserCaptureKind::Page, "safe page text");
    input.source_url = format!("https://example.com/{secret}?token={secret}#private");

    let summary = store_text_evidence(td.path(), input).unwrap();

    assert!(!summary.source_url.contains(&secret));
    assert!(!summary.source_url.contains('?'));
    assert!(!summary.source_url.contains('#'));
    assert_eq!(summary.redaction_count, 1);
}

#[test]
fn provenance_never_falls_back_to_a_secret_bearing_hostname() {
    let td = TempDir::new("url-secret-host");
    let secret = format!("sk-{}", "h".repeat(24));
    let mut input = capture(BrowserCaptureKind::Page, "safe page text");
    input.source_url = format!("https://{secret}.example.com/path");

    let summary = store_text_evidence(td.path(), input).unwrap();

    assert_eq!(summary.source_url, "https://redacted.invalid/");
    assert!(!summary.source_url.contains(&secret));
    assert_eq!(summary.redaction_count, 1);
}

#[test]
fn provenance_detects_percent_encoded_secret_shaped_paths() {
    let td = TempDir::new("url-encoded-secret");
    let secret = format!("sk-{}", "p".repeat(24));
    let encoded = secret
        .bytes()
        .map(|byte| format!("%{byte:02X}"))
        .collect::<String>();
    let double_encoded = encoded
        .bytes()
        .map(|byte| format!("%{byte:02X}"))
        .collect::<String>();

    for hidden_secret in [&encoded, &double_encoded] {
        let mut input = capture(BrowserCaptureKind::Page, "safe page text");
        input.source_url = format!("https://example.com/{hidden_secret}?ignored=yes#private");

        let summary = store_text_evidence(td.path(), input).unwrap();

        assert_eq!(summary.source_url, "https://example.com/");
        assert!(!summary.source_url.contains(&secret));
        assert!(!summary.source_url.contains(hidden_secret));
        assert_eq!(summary.redaction_count, 1);
    }
}

#[test]
fn title_and_content_are_utf8_truncated_before_redaction() {
    let td = TempDir::new("caps");
    let mut value = "a".repeat(BROWSER_SELECTION_BYTE_CAP - 1);
    value.push('🔥');
    value.push('z');
    let mut input = capture(BrowserCaptureKind::Selection, value);
    input.title = Some("🔥".repeat(200));

    let summary = store_text_evidence(td.path(), input).unwrap();
    let stored = read_text_evidence(td.path(), &summary.evidence_id)
        .unwrap()
        .unwrap();

    assert!(stored.truncated);
    assert!(stored.content.len() <= BROWSER_SELECTION_BYTE_CAP);
    assert!(std::str::from_utf8(stored.content.as_bytes()).is_ok());
    assert!(stored.title.unwrap().len() <= 512);
}

#[test]
fn page_and_selection_use_different_content_caps() {
    let td = TempDir::new("kind-caps");
    let selection = store_text_evidence(
        td.path(),
        capture(
            BrowserCaptureKind::Selection,
            "s".repeat(BROWSER_PAGE_BYTE_CAP),
        ),
    )
    .unwrap();
    let page = store_text_evidence(
        td.path(),
        capture(BrowserCaptureKind::Page, "p".repeat(BROWSER_PAGE_BYTE_CAP)),
    )
    .unwrap();

    assert_eq!(selection.bytes as usize, BROWSER_SELECTION_BYTE_CAP);
    assert!(selection.truncated);
    assert_eq!(page.bytes as usize, BROWSER_PAGE_BYTE_CAP);
    assert!(!page.truncated);
}

#[test]
fn source_truncation_remains_visible_even_when_content_fits() {
    let td = TempDir::new("source-truncated");
    let mut input = capture(BrowserCaptureKind::Page, "bounded prefix");
    input.source_truncated = true;
    let summary = store_text_evidence(td.path(), input).unwrap();
    assert!(summary.truncated);
}

#[test]
fn redaction_runs_before_the_final_cap_so_a_boundary_secret_cannot_leak() {
    let td = TempDir::new("redact-before-cap");
    let secret = format!("sk-{}", "z".repeat(24));
    let content = format!(
        "{} {secret} tail",
        "x".repeat(BROWSER_SELECTION_BYTE_CAP - 8)
    );
    let summary =
        store_text_evidence(td.path(), capture(BrowserCaptureKind::Selection, content)).unwrap();
    let stored = read_text_evidence(td.path(), &summary.evidence_id)
        .unwrap()
        .unwrap();

    assert_eq!(stored.redaction_count, 1);
    assert!(!stored.content.contains("[REDACT"));
    assert!(!stored.content.contains(&secret));
    assert!(stored.content.len() <= BROWSER_SELECTION_BYTE_CAP);
}

#[test]
fn store_never_evicts_and_stops_at_the_record_cap() {
    let td = TempDir::new("record-cap");
    let first = store_text_evidence(td.path(), capture(BrowserCaptureKind::Page, "first")).unwrap();
    for index in 1..BROWSER_EVIDENCE_MAX_RECORDS {
        store_text_evidence(
            td.path(),
            capture(BrowserCaptureKind::Page, format!("record {index}")),
        )
        .unwrap();
    }

    let error =
        store_text_evidence(td.path(), capture(BrowserCaptureKind::Page, "overflow")).unwrap_err();
    assert!(error.to_string().contains("capacity"));
    assert!(read_text_evidence(td.path(), &first.evidence_id)
        .unwrap()
        .is_some());
}

#[test]
fn total_byte_cap_is_enforced_before_the_record_cap() {
    let td = TempDir::new("byte-cap");
    let content = "x".repeat(BROWSER_PAGE_BYTE_CAP);
    let mut accepted = 0usize;
    loop {
        match store_text_evidence(
            td.path(),
            capture(BrowserCaptureKind::Page, content.clone()),
        ) {
            Ok(_) => accepted += 1,
            Err(error) => {
                assert!(error.to_string().contains("capacity"));
                break;
            }
        }
    }
    assert!(accepted < BROWSER_EVIDENCE_MAX_RECORDS);
    assert!((accepted as u64) * (BROWSER_PAGE_BYTE_CAP as u64) <= BROWSER_EVIDENCE_TOTAL_BYTE_CAP);
}

#[test]
fn invalid_ids_and_tampered_records_are_rejected() {
    let td = TempDir::new("tampered");
    assert!(read_text_evidence(td.path(), "../outside").is_err());

    let evidence_dir = td.path().join(".plume/browser-evidence");
    fs::create_dir_all(&evidence_dir).unwrap();
    let id = format!("be_{}", "a".repeat(32));
    fs::write(
        evidence_dir.join(format!("{id}.json")),
        r#"{"version":2,"id":"be_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","captureKind":"page","sourceUrl":"https://example.com/","title":null,"capturedAtMs":1,"content":"x","bytes":1,"redactionCount":0,"truncated":false}"#,
    )
    .unwrap();
    assert!(read_text_evidence(td.path(), &id).is_err());
}

#[test]
fn read_rejects_valid_but_unsanitized_url_provenance() {
    let td = TempDir::new("tampered-url");
    let summary = store_text_evidence(
        td.path(),
        capture(BrowserCaptureKind::Page, "safe page text"),
    )
    .unwrap();
    let path = td
        .path()
        .join(".plume/browser-evidence")
        .join(format!("{}.json", summary.evidence_id));
    let mut record: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    record["sourceUrl"] = serde_json::Value::String(format!(
        "https://sk-{}.example.com/path?secret=yes#private",
        "t".repeat(24)
    ));
    fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();

    assert!(read_text_evidence(td.path(), &summary.evidence_id).is_err());
}

#[cfg(unix)]
#[test]
fn planted_symlinks_and_hardlinked_records_are_refused() {
    use std::os::unix::fs::symlink;

    let td = TempDir::new("links");
    let outside = TempDir::new("outside");
    symlink(outside.path(), td.path().join(".plume")).unwrap();
    assert!(store_text_evidence(td.path(), capture(BrowserCaptureKind::Page, "x")).is_err());

    let td = TempDir::new("hardlink");
    let summary = store_text_evidence(td.path(), capture(BrowserCaptureKind::Page, "x")).unwrap();
    let path = td
        .path()
        .join(".plume/browser-evidence")
        .join(format!("{}.json", summary.evidence_id));
    fs::hard_link(&path, td.path().join("alias.json")).unwrap();
    assert!(read_text_evidence(td.path(), &summary.evidence_id).is_err());
}

#[test]
fn record_wire_shape_is_camel_case_and_versioned() {
    let record = BrowserEvidenceRecord {
        version: 1,
        id: format!("be_{}", "b".repeat(32)),
        capture_kind: BrowserCaptureKind::Selection,
        source_url: "https://example.com/".into(),
        title: None,
        captured_at_ms: 1,
        content: "text".into(),
        bytes: 4,
        redaction_count: 0,
        truncated: false,
    };
    let value = serde_json::to_value(record).unwrap();
    assert_eq!(value["captureKind"], "selection");
    assert_eq!(value["capturedAtMs"], 1);
    assert!(value.get("capture_kind").is_none());
}
