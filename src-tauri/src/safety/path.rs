//! Path safety primitives.
//!
//! Two responsibilities, both narrow:
//!
//! 1. `canonicalize_root` — turn a user-supplied path into a canonical,
//!    confirmed-directory absolute path. This is the value Plume holds
//!    as "the project root" for the rest of the session.
//!
//! 2. `ensure_inside` — given an already-canonical project root and
//!    some target path, verify the canonical form of `target` is
//!    inside `root`. This is what keeps a `../` or symlink trick from
//!    leaking writes outside the project. Symlinks are collapsed by
//!    `fs::canonicalize`, so a symlink pointing at `/etc/passwd`
//!    canonicalizes to `/etc/passwd` and fails the `starts_with`
//!    check.
//!
//! TOCTOU note: this module's checks are by-path, not by-FD. A fully
//! TOCTOU-safe implementation needs `openat`-style FD ops on Unix and
//! the equivalent on Windows. That hardening is reserved (see
//! `docs/SAFETY.md`); v1 catches the common cases.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum PathError {
    #[error("path is outside project root: {0}")]
    Escape(PathBuf),
    #[error("path not found: {0}")]
    NotFound(PathBuf),
    #[error("not a directory: {0}")]
    NotADirectory(PathBuf),
    #[error("file has multiple hardlink aliases (rejected): {0}")]
    Hardlink(PathBuf),
    #[error("io error on {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// Canonicalize and confirm-directory a project root.
pub fn canonicalize_root(p: &Path) -> Result<PathBuf, PathError> {
    let canon = canonicalize(p)?;
    if !canon.is_dir() {
        return Err(PathError::NotADirectory(canon));
    }
    Ok(canon)
}

/// Verify `target` lives inside `root`, after canonicalizing `target`.
/// `root` must already be canonical (use `canonicalize_root`).
pub fn ensure_inside(root: &Path, target: &Path) -> Result<PathBuf, PathError> {
    debug_assert!(
        root == fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf()),
        "ensure_inside: root must be canonical, got {}",
        root.display()
    );
    let canon_target = canonicalize(target)?;
    if !canon_target.starts_with(root) {
        return Err(PathError::Escape(canon_target));
    }
    Ok(canon_target)
}

/// Reject a file with more than one hardlink alias. Coarse: rejects
/// any nlink > 1 for files, even if the other links are also inside
/// the project. False positives are rare for editor source trees and
/// the cost of being permissive is exfiltration via a pre-planted
/// hardlink to `/etc/passwd`.
///
/// Directories are exempt because `.` and `..` give every dir nlink>=2.
#[cfg(unix)]
pub fn ensure_no_hardlink_alias(path: &Path) -> Result<(), PathError> {
    use std::os::unix::fs::MetadataExt;
    let meta = fs::symlink_metadata(path).map_err(|source| PathError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if meta.file_type().is_file() && meta.nlink() > 1 {
        return Err(PathError::Hardlink(path.to_path_buf()));
    }
    Ok(())
}

#[cfg(not(unix))]
pub fn ensure_no_hardlink_alias(_path: &Path) -> Result<(), PathError> {
    // Windows: NTFS hardlinks exist but link count is not as commonly
    // abused. Reserved for a Windows-specific implementation.
    Ok(())
}

fn canonicalize(p: &Path) -> Result<PathBuf, PathError> {
    fs::canonicalize(p).map_err(|source| match source.kind() {
        io::ErrorKind::NotFound => PathError::NotFound(p.to_path_buf()),
        _ => PathError::Io {
            path: p.to_path_buf(),
            source,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;

    /// Minimal tempdir helper. Avoids pulling in `tempfile` as a
    /// dev-dependency for v1; replace with `tempfile::TempDir` if we
    /// ever need cross-platform Windows support for tests.
    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(label: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock before epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "plume-test-{}-{}-{}",
                label,
                std::process::id(),
                nanos
            ));
            fs::create_dir_all(&path).expect("create tempdir");
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
    fn canonicalize_root_succeeds_on_existing_dir() {
        let td = TempDir::new("croot");
        let canon = canonicalize_root(td.path()).unwrap();
        assert!(canon.is_absolute());
        assert!(canon.is_dir());
    }

    #[test]
    fn canonicalize_root_fails_on_missing_path() {
        let td = TempDir::new("cmiss");
        let missing = td.path().join("does-not-exist");
        let err = canonicalize_root(&missing).unwrap_err();
        assert!(matches!(err, PathError::NotFound(_)), "got {err:?}");
    }

    #[test]
    fn canonicalize_root_fails_on_file() {
        let td = TempDir::new("cfile");
        let f = td.path().join("a.txt");
        File::create(&f).unwrap();
        let err = canonicalize_root(&f).unwrap_err();
        assert!(matches!(err, PathError::NotADirectory(_)), "got {err:?}");
    }

    #[test]
    fn ensure_inside_accepts_child() {
        let td = TempDir::new("eichild");
        let root = canonicalize_root(td.path()).unwrap();
        let child = td.path().join("file.txt");
        File::create(&child).unwrap();
        let canon = ensure_inside(&root, &child).unwrap();
        assert!(canon.starts_with(&root));
    }

    #[test]
    fn ensure_inside_rejects_sibling_outside() {
        let td_a = TempDir::new("eia");
        let td_b = TempDir::new("eib");
        let root = canonicalize_root(td_a.path()).unwrap();
        let outside = td_b.path().join("oops.txt");
        File::create(&outside).unwrap();
        let err = ensure_inside(&root, &outside).unwrap_err();
        assert!(matches!(err, PathError::Escape(_)), "got {err:?}");
    }

    #[cfg(unix)]
    #[test]
    fn ensure_inside_rejects_symlink_escape() {
        use std::os::unix::fs::symlink;
        let td_root = TempDir::new("eisym-r");
        let td_outside = TempDir::new("eisym-o");
        let root = canonicalize_root(td_root.path()).unwrap();
        let link = td_root.path().join("escape");
        symlink(td_outside.path(), &link).unwrap();
        let err = ensure_inside(&root, &link).unwrap_err();
        assert!(matches!(err, PathError::Escape(_)), "got {err:?}");
    }

    #[cfg(unix)]
    #[test]
    fn ensure_no_hardlink_alias_accepts_lone_file() {
        let td = TempDir::new("nhl-ok");
        let f = td.path().join("solo.txt");
        File::create(&f).unwrap();
        ensure_no_hardlink_alias(&f).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn ensure_no_hardlink_alias_rejects_aliased_file() {
        let td = TempDir::new("nhl-bad");
        let original = td.path().join("orig.txt");
        File::create(&original).unwrap();
        let alias = td.path().join("alias.txt");
        fs::hard_link(&original, &alias).unwrap();
        let err = ensure_no_hardlink_alias(&original).unwrap_err();
        assert!(matches!(err, PathError::Hardlink(_)), "got {err:?}");
    }
}
