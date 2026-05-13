//! Project-instructions reader.
//!
//! D11: when a trusted project has a root `AGENTS.md`, the chat
//! handler folds it in as a leading `system` message before each
//! send to the model. The reader reuses the Rust-private
//! `read_for_prompt` path so the same gates apply that protect
//! every other prompt read — secret-filename block, 256 KiB size
//! cap, binary detection, UTF-8 validation, hardlink alias check,
//! content redactor. Raw bytes never leave this module; the chat
//! handler only sees `RedactedContent`.
//!
//! Re-read on every send is intentional. Ollama's `/api/chat` is
//! stateless across calls, so the system message needs to ride
//! along every time. Caching would introduce a stale-content
//! window (user edits `AGENTS.md`, model still sees the old
//! version) that the simple re-read avoids. The file is small
//! (256 KiB ceiling) and the read is local — the cost is
//! negligible against the model call that follows.
//!
//! Errors don't propagate. If `AGENTS.md` exists but can't be
//! read (oversize, binary, hardlink alias, invalid UTF-8, …) we
//! log a debug-level note and return `None`. The user's chat
//! still works; the frontend's "Project instructions included"
//! indicator simply doesn't appear for that send. Failing the
//! whole send on a broken instructions file would be more
//! disruptive than the missing context, and the chat handler can
//! still surface the issue through the structured trace.
//!
//! Scope limits (D11):
//!   * `AGENTS.md` only — no `README.md` auto-context, no
//!     `.plume/` overlays, no nested per-directory instruction
//!     files. Those are roadmap.
//!   * Project root only — `AGENTS.md` at the canonical project
//!     root is the source. Subdirectory `AGENTS.md` files are not
//!     auto-included.

use std::path::Path;

use crate::prompts::read::{read_for_prompt, RedactedContent};
use crate::safety::path::ensure_inside;

/// Filename probed at the project root. Pinned as a constant so a
/// future "consolidate `CLAUDE.md` into `AGENTS.md`" sweep can
/// grep for the single source of truth.
pub(in crate::prompts) const INSTRUCTIONS_FILENAME: &str = "AGENTS.md";

/// Read `AGENTS.md` from the project root, return `None` if it
/// can't be folded into the prompt for any reason. The chat
/// handler is the only caller via `prompts::assemble`.
pub(in crate::prompts) fn read_project_instructions(root: &Path) -> Option<RedactedContent> {
    let target = root.join(INSTRUCTIONS_FILENAME);
    // `is_file` follows symlinks; the canonical-target check below
    // catches symlink escapes regardless. The early bail keeps the
    // "no AGENTS.md" case free of filesystem syscalls past the
    // metadata probe.
    if !target.is_file() {
        return None;
    }
    let canon = match ensure_inside(root, &target) {
        Ok(c) => c,
        Err(err) => {
            // A symlinked AGENTS.md that resolves outside the
            // project lands here — refuse to surface its content.
            tracing::debug!(
                error = %err,
                "AGENTS.md exists but failed path-safety check; skipping project instructions"
            );
            return None;
        }
    };
    match read_for_prompt(root, &canon, INSTRUCTIONS_FILENAME) {
        Ok(content) => {
            if content.content.trim().is_empty() {
                // An empty / whitespace-only AGENTS.md adds no
                // useful signal to the model and would still fire
                // the "Project instructions included" indicator
                // dishonestly. Treat as absent.
                tracing::debug!(
                    "AGENTS.md is empty or whitespace-only; skipping project instructions"
                );
                return None;
            }
            Some(content)
        }
        Err(err) => {
            // Most likely paths into this branch: the file is over
            // the 256 KiB prompt-read cap, contains a NUL byte and
            // failed binary detection, or canonicalises to a
            // hardlink alias. Skipping is more user-friendly than
            // failing the whole send — the chat still works,
            // they're just not getting auto-context this turn.
            tracing::debug!(
                error = ?err,
                "AGENTS.md present but unreadable via prompt-read path; skipping project instructions"
            );
            None
        }
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
                "plume-instr-{}-{}-{}",
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

    #[test]
    fn returns_none_when_no_agents_md() {
        let td = TempDir::new("none");
        let root = canonicalize_root(td.path()).unwrap();
        assert!(read_project_instructions(&root).is_none());
    }

    #[test]
    fn reads_agents_md_when_present() {
        let td = TempDir::new("present");
        fs::write(
            td.path().join("AGENTS.md"),
            "# Project rules\n\nNo writes without approval.\n",
        )
        .unwrap();
        let root = canonicalize_root(td.path()).unwrap();
        let red = read_project_instructions(&root).expect("must read");
        assert!(red.content.contains("No writes without approval."));
        assert_eq!(red.rel_path, "AGENTS.md");
    }

    #[test]
    fn returns_none_for_empty_agents_md() {
        // An empty file would surface a system message containing
        // only the preamble — pointless and dishonest. Treat as
        // "no instructions".
        let td = TempDir::new("empty");
        fs::write(td.path().join("AGENTS.md"), "").unwrap();
        let root = canonicalize_root(td.path()).unwrap();
        assert!(read_project_instructions(&root).is_none());
    }

    #[test]
    fn returns_none_for_whitespace_only_agents_md() {
        let td = TempDir::new("whitespace");
        fs::write(td.path().join("AGENTS.md"), "\n\n  \t\n\n").unwrap();
        let root = canonicalize_root(td.path()).unwrap();
        assert!(read_project_instructions(&root).is_none());
    }

    #[test]
    fn returns_none_for_oversize_agents_md() {
        // > 256 KiB → read_for_prompt rejects with Blocked; we
        // map that to a skip rather than failing the chat send.
        let td = TempDir::new("big");
        let oversized = vec![b'a'; (256 * 1024 + 1) as usize];
        fs::write(td.path().join("AGENTS.md"), &oversized).unwrap();
        let root = canonicalize_root(td.path()).unwrap();
        assert!(read_project_instructions(&root).is_none());
    }

    #[test]
    fn returns_none_for_binary_agents_md() {
        // Pathological case — AGENTS.md with a NUL byte. Treated
        // as binary and skipped.
        let td = TempDir::new("binary");
        fs::write(td.path().join("AGENTS.md"), [b'h', b'i', 0u8, b'x']).unwrap();
        let root = canonicalize_root(td.path()).unwrap();
        assert!(read_project_instructions(&root).is_none());
    }

    #[test]
    fn runs_content_redactor_over_agents_md() {
        // The same redactor that runs over file attachments runs
        // here too — defense in depth in case a user pastes a key
        // into AGENTS.md by accident.
        let td = TempDir::new("redact");
        // Deliberate fake — the literal is the test input we
        // expect the redactor to catch. `gitleaks:allow` must sit
        // on the same line as the literal for the pre-commit
        // scanner to honor it.
        let raw = "Don't share OPENAI_API_KEY=sk-1234567890abcdef1234567890abcdef please.\n"; // gitleaks:allow
        fs::write(td.path().join("AGENTS.md"), raw).unwrap();
        let root = canonicalize_root(td.path()).unwrap();
        let red = read_project_instructions(&root).expect("must read");
        assert!(!red.content.contains("sk-1234567890abcdef1234567890abcdef")); // gitleaks:allow
        assert!(red.content.contains("[REDACTED:api-key]"));
        assert_eq!(red.redactions.len(), 1);
    }

    #[test]
    fn directory_named_agents_md_is_skipped() {
        // Defensive: a directory called AGENTS.md (rare, but
        // possible) must not crash; `is_file` returns false so we
        // skip cleanly.
        let td = TempDir::new("dir");
        fs::create_dir_all(td.path().join("AGENTS.md")).unwrap();
        let root = canonicalize_root(td.path()).unwrap();
        assert!(read_project_instructions(&root).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn skips_symlinked_agents_md_pointing_outside_the_project() {
        // A malicious or accidentally-misconfigured AGENTS.md
        // symlink that resolves to a file outside the project
        // root must NOT surface its content to the model. The
        // canonicalize-then-`starts_with` check inside
        // `ensure_inside` catches the escape; this test pins
        // that behavior at the entry point used for D11.
        use std::os::unix::fs::symlink;

        let outside = TempDir::new("symlink-target");
        let outside_file = outside.path().join("secret-notes.md");
        fs::write(&outside_file, "outside-project content\n").unwrap();

        let td = TempDir::new("symlink-root");
        let link = td.path().join("AGENTS.md");
        symlink(&outside_file, &link).expect("symlink must succeed on unix tempfs");
        let root = canonicalize_root(td.path()).unwrap();

        let result = read_project_instructions(&root);
        assert!(
            result.is_none(),
            "symlink escaping the project root must skip the read; got {result:?}"
        );
    }
}
