//! Prompt-read path: reach disk for the model.
//!
//! Display reads (`fs::read::read_file`) return `FileContent` and
//! are safe to surface in the editor; this module returns
//! `RedactedContent` which is the only type the assembler accepts.
//! There is intentionally no `From<FileContent> for RedactedContent`
//! — the secret redactor is the single producer, so the boundary is
//! enforced at the type level.
//!
//! The module is registered with the crate as `mod read;` inside
//! `prompts/mod.rs` so visibility is `pub(in crate::prompts)` by
//! default. Only `prompts::assemble` calls in here; no IPC handler,
//! no other module, no test outside this file constructs a
//! `RedactedContent` value.
//!
//! Reasons this can reject a path:
//!   * Filename matches the secret-pattern policy (`.env*`,
//!     `id_rsa*`, `*.pem`, `*.key`, `credentials`, `token`) — same
//!     deny-list as display reads.
//!   * Path is under `.git/objects/**` — same git carve-out as
//!     display reads.
//!   * File is larger than `PROMPT_READ_MAX_BYTES` — separate cap
//!     from display reads because we're now feeding a model.
//!   * File is binary (any NUL byte; falls back to a UTF-8 check
//!     when no NUL is seen).
//!   * Hardlink alias (`nlink > 1` on Unix).
//!
//! Everything else flows through the redactor and emerges as
//! `RedactedContent`.

use std::fs;
use std::path::Path;

use serde::Serialize;

use crate::error::IpcError;
use crate::fs::policy::block_reason;
use crate::prompts::redact::{redact, RedactionSpan};
use crate::safety::path::ensure_no_hardlink_alias;

/// Size cap for a single prompt-read attachment. Smaller than the
/// display cap (2 MiB) because the content is going into a model
/// context window, not a viewport. 256 KiB fits a long source file,
/// a small JSON config, or a generous README — and stays well under
/// any current local-model context budget when paired with a normal
/// user instruction.
///
/// Documented in `docs/IPC_CONTRACT.md § chat` so frontends can
/// surface the limit before a send.
pub const PROMPT_READ_MAX_BYTES: u64 = 256 * 1024;

/// File content that has passed the secret-filename gate, the size
/// gate, the binary gate, and the content redactor. The assembler is
/// the only consumer; no IPC verb returns this value.
///
/// `#[serde(rename_all = "camelCase")]` is forward-looking: the
/// shape is not on the wire today, but if a future verb returns it
/// the field names already match the project convention.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RedactedContent {
    /// Project-relative path the assembler will quote inside the
    /// prompt. Caller passes this in; we re-emit it so callers don't
    /// have to thread it back through their own state.
    pub rel_path: String,
    /// Redacted text. Original bytes never leave this module.
    pub content: String,
    /// Byte count on disk before redaction. Used by tests + the
    /// frontend chip's tooltip ("184 KiB read, 2 secrets redacted").
    pub original_bytes: u64,
    /// Redaction spans pointing into the ORIGINAL content. Useful
    /// for tests; the assembler only needs the count.
    pub redactions: Vec<RedactionSpan>,
}

/// Read `target` for prompt context. `target` is already canonical
/// and confirmed inside `root` — see the chat command handler. This
/// function does NOT re-canonicalize; canonicalize once, then call.
///
/// `rel_path` is the project-relative form the caller wants quoted
/// in the assembled prompt. Passing it through here keeps the
/// model-facing label deterministic even if the canonical absolute
/// path drifts (symlinks, case-folded filesystems on macOS).
///
/// Visibility is `pub(in crate::prompts)` so the secret redactor's
/// single-producer property is enforced at the type level — no
/// module outside `prompts::` can construct a `RedactedContent`.
pub(in crate::prompts) fn read_for_prompt(
    root: &Path,
    target: &Path,
    rel_path: &str,
) -> Result<RedactedContent, IpcError> {
    debug_assert!(
        target.starts_with(root),
        "read_for_prompt expects target inside root"
    );

    // Same secret-filename / .git/objects gate as `fs.read`. We
    // intentionally use the SAME function so the two paths can
    // never drift on what counts as "do not surface this." Display
    // reads return `Blocked`; we do too.
    if let Some(reason) = block_reason(target, root) {
        return Err(IpcError::Blocked(reason));
    }

    let metadata = fs::symlink_metadata(target).map_err(|err| io_to_ipc(target, err))?;
    if !metadata.is_file() {
        return Err(IpcError::BadArgument(format!(
            "attachment is not a regular file: {}",
            target.display()
        )));
    }

    ensure_no_hardlink_alias(target).map_err(IpcError::from)?;

    let bytes_on_disk = metadata.len();
    if bytes_on_disk > PROMPT_READ_MAX_BYTES {
        return Err(IpcError::Blocked(format!(
            "{} is {} bytes; prompt attachments are capped at {} bytes",
            target.display(),
            bytes_on_disk,
            PROMPT_READ_MAX_BYTES
        )));
    }

    let raw = fs::read(target).map_err(|err| io_to_ipc(target, err))?;

    // Binary detection: same heuristic as display reads. A NUL byte
    // is a cheap "this isn't text" signal; if it passes that, the
    // UTF-8 conversion below catches the rest.
    if raw.contains(&0u8) {
        return Err(IpcError::Blocked(format!(
            "{} looks binary (NUL byte) — attachments must be UTF-8 text",
            target.display()
        )));
    }
    let text = String::from_utf8(raw).map_err(|_| {
        IpcError::Blocked(format!(
            "{} is not valid UTF-8 — attachments must be UTF-8 text",
            target.display()
        ))
    })?;

    let (redacted, redactions) = redact(&text);
    Ok(RedactedContent {
        rel_path: rel_path.to_string(),
        content: redacted,
        original_bytes: bytes_on_disk,
        redactions,
    })
}

fn io_to_ipc(path: &Path, err: std::io::Error) -> IpcError {
    use std::io::ErrorKind;
    match err.kind() {
        ErrorKind::NotFound => IpcError::NotFound(path.display().to_string()),
        ErrorKind::PermissionDenied => IpcError::Internal(format!(
            "permission denied reading {}: {err}",
            path.display()
        )),
        _ => IpcError::Internal(format!("io error on {}: {err}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::safety::path::canonicalize_root;
    use std::fs;
    use std::path::PathBuf;
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
                "plume-prompt-{}-{}-{}",
                label,
                std::process::id(),
                nanos
            ));
            fs::create_dir_all(&path).unwrap();
            Self { path }
        }
        fn path(&self) -> &Path {
            &self.path
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn write_file(path: &Path, content: &[u8]) {
        fs::write(path, content).unwrap();
    }

    #[test]
    fn reads_utf8_and_runs_redactor() {
        let td = TempDir::new("happy");
        let root = canonicalize_root(td.path()).unwrap();
        let file = td.path().join("a.txt");
        // Deliberate fake — the pattern is what we want the redactor
        // to catch. `gitleaks:allow` keeps the pre-commit scanner
        // from flagging the literal.
        let raw = "OPENAI_API_KEY=sk-1234567890abcdef1234567890abcdef\nhello\n"; // gitleaks:allow
        write_file(&file, raw.as_bytes());
        let canon = fs::canonicalize(&file).unwrap();

        let red = read_for_prompt(&root, &canon, "a.txt").expect("ok");
        assert_eq!(red.rel_path, "a.txt");
        assert_eq!(red.original_bytes, raw.len() as u64);
        assert_eq!(red.redactions.len(), 1);
        assert_eq!(red.redactions[0].kind, "api-key");
        assert!(red.content.contains("[REDACTED:api-key]"));
        assert!(red.content.contains("hello"));
    }

    #[test]
    fn rejects_secret_filename() {
        // Same deny-list as display reads — never surface .env*.
        let td = TempDir::new("envfn");
        let root = canonicalize_root(td.path()).unwrap();
        let file = td.path().join(".env.local");
        write_file(&file, b"X=1");
        let canon = fs::canonicalize(&file).unwrap();

        let err = read_for_prompt(&root, &canon, ".env.local").unwrap_err();
        assert!(matches!(err, IpcError::Blocked(_)), "got {err:?}");
    }

    #[test]
    fn rejects_oversized_attachment() {
        // One byte over the cap. Tests the gate runs BEFORE the
        // read, so we don't allocate the buffer.
        let td = TempDir::new("big");
        let root = canonicalize_root(td.path()).unwrap();
        let file = td.path().join("big.txt");
        let big = vec![b'a'; (PROMPT_READ_MAX_BYTES + 1) as usize];
        write_file(&file, &big);
        let canon = fs::canonicalize(&file).unwrap();

        let err = read_for_prompt(&root, &canon, "big.txt").unwrap_err();
        match err {
            IpcError::Blocked(msg) => assert!(
                msg.contains("prompt attachments are capped"),
                "msg was: {msg}"
            ),
            other => panic!("expected Blocked, got {other:?}"),
        }
    }

    #[test]
    fn rejects_binary_with_nul() {
        let td = TempDir::new("bin");
        let root = canonicalize_root(td.path()).unwrap();
        let file = td.path().join("logo.bin");
        write_file(&file, &[b'P', b'K', 0u8, 1, 2, 3]);
        let canon = fs::canonicalize(&file).unwrap();

        let err = read_for_prompt(&root, &canon, "logo.bin").unwrap_err();
        match err {
            IpcError::Blocked(msg) => assert!(msg.contains("binary"), "msg was: {msg}"),
            other => panic!("expected Blocked, got {other:?}"),
        }
    }

    #[test]
    fn rejects_invalid_utf8_even_without_nul() {
        let td = TempDir::new("notutf");
        let root = canonicalize_root(td.path()).unwrap();
        let file = td.path().join("latin1.txt");
        // Lone 0xFF — invalid UTF-8 but no NUL.
        write_file(&file, &[b'a', 0xFFu8, b'b']);
        let canon = fs::canonicalize(&file).unwrap();

        let err = read_for_prompt(&root, &canon, "latin1.txt").unwrap_err();
        match err {
            IpcError::Blocked(msg) => assert!(msg.contains("UTF-8"), "msg was: {msg}"),
            other => panic!("expected Blocked, got {other:?}"),
        }
    }

    #[test]
    fn rejects_directory_target() {
        let td = TempDir::new("dir");
        fs::create_dir_all(td.path().join("subdir")).unwrap();
        let root = canonicalize_root(td.path()).unwrap();
        let canon = fs::canonicalize(td.path().join("subdir")).unwrap();
        let err = read_for_prompt(&root, &canon, "subdir").unwrap_err();
        assert!(matches!(err, IpcError::BadArgument(_)), "got {err:?}");
    }

    #[cfg(unix)]
    #[test]
    fn rejects_hardlink_alias() {
        let td = TempDir::new("hl");
        let root = canonicalize_root(td.path()).unwrap();
        let original = td.path().join("orig.txt");
        write_file(&original, b"x");
        let alias = td.path().join("alias.txt");
        fs::hard_link(&original, &alias).unwrap();
        let canon = fs::canonicalize(&original).unwrap();
        let err = read_for_prompt(&root, &canon, "orig.txt").unwrap_err();
        assert!(matches!(err, IpcError::PathEscape(_)), "got {err:?}");
    }

    #[test]
    fn passes_through_when_no_secrets_present() {
        let td = TempDir::new("plain");
        let root = canonicalize_root(td.path()).unwrap();
        let file = td.path().join("plain.rs");
        let raw = "fn add(a: i32, b: i32) -> i32 { a + b }";
        write_file(&file, raw.as_bytes());
        let canon = fs::canonicalize(&file).unwrap();

        let red = read_for_prompt(&root, &canon, "plain.rs").expect("ok");
        assert_eq!(red.content, raw);
        assert!(red.redactions.is_empty());
        assert_eq!(red.original_bytes, raw.len() as u64);
    }

    #[test]
    fn blocks_paths_under_git_objects() {
        let td = TempDir::new("gobj");
        fs::create_dir_all(td.path().join(".git/objects/ab")).unwrap();
        let file = td.path().join(".git/objects/ab/cdef");
        write_file(&file, b"a");
        let root = canonicalize_root(td.path()).unwrap();
        let canon = fs::canonicalize(&file).unwrap();
        let err = read_for_prompt(&root, &canon, ".git/objects/ab/cdef").unwrap_err();
        assert!(matches!(err, IpcError::Blocked(_)), "got {err:?}");
    }
}
