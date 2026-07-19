use std::cell::RefCell;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::export::{
    default_markdown_name, export_choice, write_markdown_atomic, write_markdown_atomic_with,
    ExportChoice, ExportError, ExportFilePort, ExportOutcome,
};

#[derive(Default)]
struct RecordingPort {
    writes: RefCell<Vec<(PathBuf, Vec<u8>, bool)>>,
    fail: bool,
}

impl ExportFilePort for RecordingPort {
    fn write(&self, path: &Path, bytes: &[u8], overwrite: bool) -> Result<(), ExportError> {
        self.writes
            .borrow_mut()
            .push((path.to_path_buf(), bytes.to_vec(), overwrite));
        if self.fail {
            Err(ExportError::Write("simulated failure".into()))
        } else {
            Ok(())
        }
    }
}

#[test]
fn cancellation_is_not_an_error_and_writes_nothing() {
    let port = RecordingPort::default();
    assert_eq!(
        export_choice(ExportChoice::Cancelled, b"# Note\n", &port).unwrap(),
        ExportOutcome::Cancelled
    );
    assert!(port.writes.borrow().is_empty());
}

#[test]
fn save_passes_exact_bytes_and_only_returns_the_display_name() {
    let port = RecordingPort::default();
    let path = std::env::temp_dir().join("private-note.md");
    let outcome = export_choice(
        ExportChoice::Save {
            path: path.clone(),
            overwrite_confirmed: true,
        },
        b"# Note\n\nExact bytes.\n",
        &port,
    )
    .unwrap();
    assert_eq!(
        outcome,
        ExportOutcome::Saved {
            file_name: "private-note.md".into()
        }
    );
    assert_eq!(
        serde_json::to_value(&outcome).unwrap(),
        serde_json::json!({ "status": "saved", "fileName": "private-note.md" })
    );
    assert_eq!(
        port.writes.into_inner(),
        vec![(path, b"# Note\n\nExact bytes.\n".to_vec(), true)]
    );
}

#[test]
fn file_failures_stay_failures_without_mutating_source_bytes() {
    let port = RecordingPort {
        fail: true,
        ..RecordingPort::default()
    };
    let markdown = b"staged artifact remains".to_vec();
    let before = markdown.clone();
    let result = export_choice(
        ExportChoice::Save {
            path: std::env::temp_dir().join("failed.md"),
            overwrite_confirmed: false,
        },
        &markdown,
        &port,
    );
    assert!(matches!(result, Err(ExportError::Write(_))));
    assert_eq!(markdown, before);
}

#[test]
fn default_name_is_markdown_and_safe() {
    assert_eq!(default_markdown_name(), "research-note.md");
}

#[test]
fn atomic_writer_creates_and_replaces_only_with_consent() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("note.md");
    write_markdown_atomic(&target, b"first", false).unwrap();
    assert_eq!(fs::read(&target).unwrap(), b"first");
    assert!(matches!(
        write_markdown_atomic(&target, b"blocked", false),
        Err(ExportError::Exists)
    ));
    assert_eq!(fs::read(&target).unwrap(), b"first");
    write_markdown_atomic(&target, b"second", true).unwrap();
    assert_eq!(fs::read(&target).unwrap(), b"second");
}

#[cfg(unix)]
#[test]
fn atomic_writer_refuses_symlink_and_hardlink_targets() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let outside = dir.path().join("outside.md");
    fs::write(&outside, "outside").unwrap();
    let symlink_target = dir.path().join("link.md");
    symlink(&outside, &symlink_target).unwrap();
    assert!(matches!(
        write_markdown_atomic(&symlink_target, b"no", true),
        Err(ExportError::Refused(_))
    ));

    let original = dir.path().join("original.md");
    fs::write(&original, "original").unwrap();
    let hardlink_target = dir.path().join("alias.md");
    fs::hard_link(&original, &hardlink_target).unwrap();
    assert!(matches!(
        write_markdown_atomic(&hardlink_target, b"no", true),
        Err(ExportError::Refused(_))
    ));
}

#[test]
fn failed_atomic_replace_cleans_temp_and_preserves_destination() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("note.md");
    fs::write(&target, "old").unwrap();
    let result = write_markdown_atomic_with(&target, b"new", true, |_from, _to| {
        Err(io::Error::other("simulated rename failure"))
    });
    assert!(matches!(result, Err(ExportError::Write(_))));
    assert_eq!(fs::read(&target).unwrap(), b"old");
    let leftovers = fs::read_dir(dir.path())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().contains("plume-export"))
        .count();
    assert_eq!(leftovers, 0);
}
