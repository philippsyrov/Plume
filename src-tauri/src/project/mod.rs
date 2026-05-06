//! Project session: opening a folder, building `ProjectMeta`,
//! tracking the currently-open project for the window.
//!
//! See `docs/IPC_CONTRACT.md` for the wire shape and
//! `docs/ARCHITECTURE.md` for the module's place in the process model.

pub mod trust;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

/// Wire representation of a project session, mirrored in
/// `docs/IPC_CONTRACT.md`. `serde(rename_all = "camelCase")` is
/// load-bearing: the contract document and the TS wrapper both expect
/// camelCase keys.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMeta {
    pub id: String,
    pub root: String,
    pub has_agents_md: bool,
    pub has_claude_md: bool,
    pub package_managers: Vec<PackageManager>,
    pub git: Option<GitState>,
    pub trust: TrustState,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PackageManager {
    Npm,
    Pnpm,
    Yarn,
    Cargo,
    Pip,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitState {
    pub branch: Option<String>,
    pub dirty_count: u32,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TrustState {
    Trusted,
    Unknown,
}

/// Build metadata for an already-canonical root.
///
/// The caller owns ID stability and trust resolution. ID stability:
/// `ProjectId` is window-lifetime per the IPC contract, so callers
/// reuse the ID stored in `ProjectSession` across `open`/`refresh`/
/// `trust`. Trust gating: git state is read by shelling out to
/// `git rev-parse` / `git status`, both of which can run hooks or a
/// configured `core.fsmonitor` binary. We refuse to invoke them on a
/// project the user has not yet trusted. Untrusted projects always
/// report `git: None`; calling `project.trust` and then `project.refresh`
/// (or `project.trust` directly, which returns the trusted meta) fills
/// in the git state.
pub fn build_meta(id: &str, root: &Path, trust: TrustState) -> ProjectMeta {
    debug_assert!(
        root.is_absolute(),
        "build_meta expects already-canonical root, got {}",
        root.display()
    );
    let git = match trust {
        TrustState::Trusted => detect_git_state(root),
        TrustState::Unknown => None,
    };
    ProjectMeta {
        id: id.to_string(),
        root: root.to_string_lossy().into_owned(),
        has_agents_md: root.join("AGENTS.md").is_file(),
        has_claude_md: root.join("CLAUDE.md").is_file(),
        package_managers: detect_package_managers(root),
        git,
        trust,
    }
}

fn detect_package_managers(root: &Path) -> Vec<PackageManager> {
    let mut found = Vec::new();
    if root.join("pnpm-lock.yaml").is_file() {
        found.push(PackageManager::Pnpm);
    }
    if root.join("yarn.lock").is_file() {
        found.push(PackageManager::Yarn);
    }
    // Use `package-lock.json` or `package.json` as the npm signal, but
    // skip if pnpm/yarn already claimed the project — those tools also
    // ship a `package.json`.
    let has_npm_signal =
        root.join("package-lock.json").is_file() || root.join("package.json").is_file();
    if has_npm_signal
        && !found
            .iter()
            .any(|p| matches!(p, PackageManager::Pnpm | PackageManager::Yarn))
    {
        found.push(PackageManager::Npm);
    }
    if root.join("Cargo.toml").is_file() {
        found.push(PackageManager::Cargo);
    }
    if root.join("pyproject.toml").is_file() || root.join("requirements.txt").is_file() {
        found.push(PackageManager::Pip);
    }
    found
}

/// Read git state by shelling out to `git`. Subprocess use is internal
/// and not exposed through the user-facing command runner. If the
/// `git` binary is missing or the directory is not a repo, returns
/// `None`. Errors past that are logged at WARN and surface as `None`
/// — opening a project must not fail because git misbehaved.
fn detect_git_state(root: &Path) -> Option<GitState> {
    if !root.join(".git").exists() {
        return None;
    }
    let branch = run_git(root, &["rev-parse", "--abbrev-ref", "HEAD"]).and_then(|s| {
        let s = s.trim();
        if s.is_empty() || s == "HEAD" {
            None
        } else {
            Some(s.to_string())
        }
    });
    let dirty_count = match run_git(root, &["status", "--porcelain=v1"]) {
        Some(out) => out.lines().filter(|l| !l.is_empty()).count() as u32,
        None => 0,
    };
    Some(GitState {
        branch,
        dirty_count,
    })
}

fn run_git(root: &Path, args: &[&str]) -> Option<String> {
    // `GIT_OPTIONAL_LOCKS=0` keeps `git status` etc. strictly read-only.
    // Without it git is permitted to take optional locks and update the
    // index's stat cache as a side effect of a "read." Trust in Plume
    // covers spawning git for metadata reads; that trust shouldn't also
    // licence git to mutate the working tree's bookkeeping.
    match Command::new("git")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .current_dir(root)
        .args(args)
        .output()
    {
        Ok(out) if out.status.success() => Some(String::from_utf8_lossy(&out.stdout).into_owned()),
        Ok(out) => {
            tracing::warn!(
                args = ?args,
                stderr = %String::from_utf8_lossy(&out.stderr).trim(),
                "git command exited non-zero"
            );
            None
        }
        Err(err) => {
            tracing::warn!(args = ?args, error = %err, "git command failed to spawn");
            None
        }
    }
}

/// Generate an opaque ID. Frontend never parses these. Format chosen
/// to be distinct-per-call within a process and roughly time-ordered;
/// `docs/IPC_CONTRACT.md` only requires opacity.
pub(crate) fn mint_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    format!("{nanos:016x}-{n:08x}")
}

/// Session state: the currently-open project for this window. Holds
/// the `ProjectId` so that `project.refresh` and `project.trust` see
/// the same opaque ID across calls — `docs/IPC_CONTRACT.md` § IDs
/// requires `ProjectId` lifetime to be window-scoped.
#[derive(Default)]
pub struct ProjectSession {
    inner: std::sync::Mutex<Option<OpenProject>>,
}

#[derive(Debug, Clone)]
pub struct OpenProject {
    pub id: String,
    pub root: PathBuf,
}

impl ProjectSession {
    /// Replace the open project, minting a fresh ID. Returns the new ID.
    /// Each `project.open` call is treated as a fresh session, so
    /// re-opening the same path twice gets two different IDs.
    pub fn open(&self, root: PathBuf) -> String {
        let id = mint_id();
        let mut guard = self.inner.lock().expect("project session poisoned");
        *guard = Some(OpenProject {
            id: id.clone(),
            root,
        });
        id
    }

    pub fn current(&self) -> Option<OpenProject> {
        self.inner.lock().expect("project session poisoned").clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::safety::path::canonicalize_root;
    use std::fs;

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
                "plume-test-{}-{}-{}",
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
    fn detect_package_managers_reads_signal_files() {
        let td = TempDir::new("pm");
        fs::write(td.path().join("package.json"), "{}").unwrap();
        fs::write(td.path().join("Cargo.toml"), "").unwrap();
        let pms = detect_package_managers(td.path());
        assert!(pms.iter().any(|p| matches!(p, PackageManager::Npm)));
        assert!(pms.iter().any(|p| matches!(p, PackageManager::Cargo)));
    }

    #[test]
    fn detect_package_managers_prefers_pnpm_over_npm_when_both_signals_present() {
        let td = TempDir::new("pm-pnpm");
        fs::write(td.path().join("package.json"), "{}").unwrap();
        fs::write(td.path().join("pnpm-lock.yaml"), "").unwrap();
        let pms = detect_package_managers(td.path());
        assert!(pms.iter().any(|p| matches!(p, PackageManager::Pnpm)));
        assert!(
            !pms.iter().any(|p| matches!(p, PackageManager::Npm)),
            "pnpm project should not also be flagged as npm"
        );
    }

    #[test]
    fn detect_git_state_returns_none_for_non_repo() {
        let td = TempDir::new("nogit");
        assert!(detect_git_state(td.path()).is_none());
    }

    #[test]
    fn mint_id_is_unique() {
        let a = mint_id();
        let b = mint_id();
        assert_ne!(a, b);
    }

    #[test]
    fn build_meta_uses_supplied_trust_state_and_id() {
        let td = TempDir::new("bm");
        fs::write(td.path().join("AGENTS.md"), "# x").unwrap();
        let root = canonicalize_root(td.path()).unwrap();
        let meta = build_meta("fixed-id", &root, TrustState::Trusted);
        assert_eq!(meta.trust, TrustState::Trusted);
        assert_eq!(meta.id, "fixed-id");
        assert!(meta.has_agents_md);
    }

    #[test]
    fn build_meta_skips_git_subprocess_when_untrusted() {
        // Even with a populated .git/ directory present, build_meta
        // must not shell out to git when the project is untrusted.
        // git can execute hooks or a configured core.fsmonitor binary,
        // and a malicious repo could leverage that on first open.
        let td = TempDir::new("bm-trust-gate");
        fs::create_dir_all(td.path().join(".git")).unwrap();
        let root = canonicalize_root(td.path()).unwrap();
        let meta = build_meta("id", &root, TrustState::Unknown);
        assert!(
            meta.git.is_none(),
            "untrusted project must not surface git state, got {:?}",
            meta.git
        );
    }

    #[test]
    fn project_session_keeps_same_id_for_open_and_current() {
        let td = TempDir::new("session");
        let root = canonicalize_root(td.path()).unwrap();
        let session = ProjectSession::default();
        let id = session.open(root.clone());
        let current = session.current().expect("session should be set");
        assert_eq!(current.id, id);
        assert_eq!(current.root, root);
    }

    #[test]
    fn project_session_mints_new_id_on_reopen() {
        let td = TempDir::new("session-reopen");
        let root = canonicalize_root(td.path()).unwrap();
        let session = ProjectSession::default();
        let first = session.open(root.clone());
        let second = session.open(root);
        assert_ne!(first, second, "re-opening counts as a fresh session");
    }
}
