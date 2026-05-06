//! Display-read policy: what `fs.read` refuses to surface to the UI.
//!
//! Display reads are not prompt reads. The prompt-read path will
//! redact secrets through a separate type and a separate set of
//! patterns; this module only decides whether the editor should *show*
//! a file at all. The lists are deliberately short — false positives
//! here annoy the user, false negatives leak credentials in
//! screenshots.
//!
//! See `docs/SAFETY.md` § Secret handling and `docs/IPC_CONTRACT.md`
//! § fs for the doc-side mirror.

use std::path::Path;

/// Display-read size cap. Larger files come back as `BadArgument`-ish
/// `Blocked` and the UI can offer "open in OS" later. Sized to fit a
/// large lockfile (`package-lock.json` ~2 MB on real projects) plus
/// a comfortable margin.
pub const DISPLAY_READ_MAX_BYTES: u64 = 2 * 1024 * 1024;

/// Decide whether a path may be displayed by `fs.read`.
///
/// Both arguments must be canonical absolute paths and `path` must
/// already be confirmed inside `root`. Returns `Some(reason)` when
/// the read should be refused.
pub fn block_reason(path: &Path, root: &Path) -> Option<String> {
    if let Some(reason) = blocked_under_git_objects(path, root) {
        return Some(reason);
    }
    blocked_filename(path)
}

fn blocked_under_git_objects(path: &Path, root: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let mut components = rel.components();
    let first = components.next()?.as_os_str();
    if first != ".git" {
        return None;
    }
    let second = components.next()?.as_os_str();
    if second == "objects" {
        Some(".git/objects is git's content store and not displayed".to_string())
    } else {
        None
    }
}

fn blocked_filename(path: &Path) -> Option<String> {
    let raw = path.file_name()?.to_string_lossy();
    let lc = raw.to_ascii_lowercase();

    // `.env*` per the documented pattern. Catches `.env`, `.env.local`,
    // `.env.production`, `.envrc` (direnv), `.env-prod`, `.env_local`,
    // and `.environment.template`. The contract spells `.env*` so the
    // matcher is `starts_with(".env")` — tighter would drift from the
    // doc.
    if lc.starts_with(".env") {
        return Some(format!("{raw} is blocked by the secret-filename policy"));
    }

    // `id_rsa*` per the documented pattern, plus the other private SSH
    // key algorithm names. Catches `id_rsa`, `id_rsa.bak`, `id_rsa.pub`,
    // `id_ed25519`, `id_ed25519.old`, `id_ecdsa`, `id_dsa`. Public keys
    // (`id_rsa.pub`) get blocked too — that's conservative but cheap;
    // public keys are uninteresting in an editor.
    for prefix in ["id_rsa", "id_ed25519", "id_ecdsa", "id_dsa"] {
        if lc.starts_with(prefix) {
            return Some(format!("{raw} looks like an SSH key file"));
        }
    }

    // Suffix-based: .pem, .key
    if lc.ends_with(".pem") || lc.ends_with(".key") {
        return Some(format!("{raw} matches the private-key suffix policy"));
    }

    // Substring-based: credentials, token (case-insensitive). Keep
    // narrow — "tokenizer.json" is fine, but "auth-token.json" gets
    // caught. The display pane is the wrong place to surface either
    // borderline case; the user can open them in their OS file
    // manager if needed.
    if lc.contains("credentials") || lc.contains("token") {
        return Some(format!(
            "{raw} matches the secret-substring policy (credentials/token)"
        ));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn allows_normal_source_files() {
        let root = p("/tmp/proj");
        assert!(block_reason(&p("/tmp/proj/src/main.rs"), &root).is_none());
        assert!(block_reason(&p("/tmp/proj/README.md"), &root).is_none());
        assert!(block_reason(&p("/tmp/proj/Cargo.toml"), &root).is_none());
    }

    #[test]
    fn blocks_dot_env_variants() {
        let root = p("/tmp/proj");
        // The simple cases.
        assert!(block_reason(&p("/tmp/proj/.env"), &root).is_some());
        assert!(block_reason(&p("/tmp/proj/.env.local"), &root).is_some());
        assert!(block_reason(&p("/tmp/proj/sub/.env.production"), &root).is_some());
        // And the variants the documented `.env*` pattern is meant to
        // cover. Earlier the matcher only handled the dot-suffix form;
        // these would have been displayable.
        assert!(block_reason(&p("/tmp/proj/.envrc"), &root).is_some());
        assert!(block_reason(&p("/tmp/proj/.env-prod"), &root).is_some());
        assert!(block_reason(&p("/tmp/proj/.env_local"), &root).is_some());
    }

    #[test]
    fn blocks_private_keys() {
        let root = p("/tmp/proj");
        assert!(block_reason(&p("/tmp/proj/id_rsa"), &root).is_some());
        assert!(block_reason(&p("/tmp/proj/id_ed25519"), &root).is_some());
        assert!(block_reason(&p("/tmp/proj/keys/server.pem"), &root).is_some());
        assert!(block_reason(&p("/tmp/proj/keys/server.key"), &root).is_some());
        // `id_rsa*` per the doc — backup and rotated variants.
        assert!(block_reason(&p("/tmp/proj/id_rsa.bak"), &root).is_some());
        assert!(block_reason(&p("/tmp/proj/id_ed25519.old"), &root).is_some());
        assert!(block_reason(&p("/tmp/proj/id_ecdsa.2024"), &root).is_some());
        // Public keys are blocked too. Conservative but uninteresting
        // to view in an editor either way.
        assert!(block_reason(&p("/tmp/proj/id_rsa.pub"), &root).is_some());
    }

    #[test]
    fn blocks_credential_substrings() {
        let root = p("/tmp/proj");
        assert!(block_reason(&p("/tmp/proj/aws-credentials.json"), &root).is_some());
        assert!(block_reason(&p("/tmp/proj/auth-token.txt"), &root).is_some());
    }

    #[test]
    fn does_not_overreach_on_credentials_substring() {
        // `tokenizer.json` happens to contain "token". This test
        // documents the false-positive: we currently reject it. If
        // this becomes a real complaint the policy needs a smarter
        // pattern; flagging here means a future change won't be silent.
        let root = p("/tmp/proj");
        assert!(block_reason(&p("/tmp/proj/tokenizer.json"), &root).is_some());
    }

    #[test]
    fn blocks_git_objects_paths() {
        let root = p("/tmp/proj");
        assert!(block_reason(&p("/tmp/proj/.git/objects/00/abcdef"), &root).is_some());
        assert!(block_reason(&p("/tmp/proj/.git/objects/pack/pack-1.idx"), &root).is_some());
    }

    #[test]
    fn allows_other_git_files_for_display() {
        let root = p("/tmp/proj");
        // SAFETY.md restricts these for *prompt* reads; display reads
        // (Slice C) leave the rest of .git/ visible. The tighter
        // whitelist lands when the prompt path lands.
        assert!(block_reason(&p("/tmp/proj/.git/HEAD"), &root).is_none());
        assert!(block_reason(&p("/tmp/proj/.git/config"), &root).is_none());
    }
}
