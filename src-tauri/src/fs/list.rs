//! `fs.list` — direct children of a directory inside the open trusted
//! project.
//!
//! Not a tree walker, not an indexer. The frontend navigates one
//! directory at a time so a large monorepo doesn't blow memory or
//! time on first open.

use std::fs;
use std::path::Path;
use std::time::UNIX_EPOCH;

use serde::Serialize;

use crate::error::IpcError;
use crate::safety::path::ensure_inside;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub kind: FileKind,
    pub size: Option<u64>,
    pub modified_ms: u64,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FileKind {
    File,
    Dir,
    Symlink,
}

/// List direct children of `target` (already-canonical, already-known
/// to live inside `root`). Sorted directories-first, then by name.
pub fn list_dir(root: &Path, target: &Path) -> Result<Vec<FileEntry>, IpcError> {
    debug_assert!(
        target.starts_with(root),
        "list_dir expects target inside root"
    );
    let metadata = fs::symlink_metadata(target).map_err(|err| io_to_ipc(target, err))?;
    if !metadata.is_dir() {
        return Err(IpcError::BadArgument(format!(
            "fs.list target is not a directory: {}",
            target.display()
        )));
    }

    let read_dir = fs::read_dir(target).map_err(|err| io_to_ipc(target, err))?;
    let mut entries = Vec::new();
    for entry in read_dir {
        let entry = entry.map_err(|err| io_to_ipc(target, err))?;
        match build_entry(root, &entry) {
            Ok(Some(file_entry)) => entries.push(file_entry),
            Ok(None) => continue,
            Err(err) => {
                // Per-entry errors don't fail the whole listing — a
                // permission-denied symlink target should not hide
                // the rest of the directory. Log and skip.
                tracing::warn!(
                    path = %entry.path().display(),
                    error = %err,
                    "fs.list skipping entry"
                );
            }
        }
    }

    entries.sort_by(|a, b| match (a.kind, b.kind) {
        (FileKind::Dir, FileKind::Dir) => a.name.cmp(&b.name),
        (FileKind::Dir, _) => std::cmp::Ordering::Less,
        (_, FileKind::Dir) => std::cmp::Ordering::Greater,
        _ => a.name.cmp(&b.name),
    });
    Ok(entries)
}

fn build_entry(root: &Path, entry: &fs::DirEntry) -> std::io::Result<Option<FileEntry>> {
    let path = entry.path();
    let name = match path.file_name() {
        Some(n) => n.to_string_lossy().into_owned(),
        None => return Ok(None),
    };
    let metadata = entry.metadata()?;
    let file_type = metadata.file_type();

    let kind = if file_type.is_symlink() {
        FileKind::Symlink
    } else if file_type.is_dir() {
        FileKind::Dir
    } else {
        FileKind::File
    };

    let size = if matches!(kind, FileKind::File) {
        Some(metadata.len())
    } else {
        None
    };

    let modified_ms = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    // Display the canonical absolute path *only when the entry is
    // already inside the project root*. For symlinks pointing outside
    // we fall back to the link's own path, leaving the resolution to
    // the caller of `fs.read` — which will reject with PathEscape.
    let display_path = match (kind, fs::canonicalize(&path)) {
        (FileKind::Symlink, _) => path.to_string_lossy().into_owned(),
        (_, Ok(canon)) if canon.starts_with(root) => canon.to_string_lossy().into_owned(),
        _ => path.to_string_lossy().into_owned(),
    };

    Ok(Some(FileEntry {
        name,
        path: display_path,
        kind,
        size,
        modified_ms,
    }))
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

/// Resolve the user-supplied `path` to a canonical absolute path
/// inside `root`. See `docs/IPC_CONTRACT.md` § fs for the rules.
pub fn resolve(root: &Path, supplied: &str) -> Result<std::path::PathBuf, IpcError> {
    let trimmed = supplied.trim();
    let candidate = if trimmed.is_empty() || trimmed == "." {
        root.to_path_buf()
    } else {
        let p = Path::new(trimmed);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            root.join(p)
        }
    };
    ensure_inside(root, &candidate).map_err(IpcError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::safety::path::canonicalize_root;
    use std::fs;
    use std::path::PathBuf;
    use std::time::SystemTime;

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
    fn list_returns_direct_children_only() {
        let td = TempDir::new("list-direct");
        fs::create_dir_all(td.path().join("sub/nested")).unwrap();
        fs::write(td.path().join("a.txt"), "x").unwrap();
        fs::write(td.path().join("sub/inner.txt"), "y").unwrap();
        let root = canonicalize_root(td.path()).unwrap();
        let entries = list_dir(&root, &root).unwrap();
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["sub", "a.txt"]);
    }

    #[test]
    fn list_sorts_dirs_first_then_alpha() {
        let td = TempDir::new("list-sort");
        fs::create_dir_all(td.path().join("zeta")).unwrap();
        fs::create_dir_all(td.path().join("alpha")).unwrap();
        fs::write(td.path().join("a.txt"), "").unwrap();
        fs::write(td.path().join("b.txt"), "").unwrap();
        let root = canonicalize_root(td.path()).unwrap();
        let entries = list_dir(&root, &root).unwrap();
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "zeta", "a.txt", "b.txt"]);
    }

    #[test]
    fn list_marks_symlink_as_symlink_kind() {
        let td = TempDir::new("list-symlink");
        let target = td.path().join("target");
        fs::create_dir_all(&target).unwrap();
        let link = td.path().join("link");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let root = canonicalize_root(td.path()).unwrap();
        let entries = list_dir(&root, &root).unwrap();
        let link_entry = entries.iter().find(|e| e.name == "link").unwrap();
        assert_eq!(link_entry.kind, FileKind::Symlink);
    }

    #[test]
    fn list_rejects_target_that_is_a_file() {
        let td = TempDir::new("list-file-target");
        let f = td.path().join("a.txt");
        fs::write(&f, "x").unwrap();
        let root = canonicalize_root(td.path()).unwrap();
        let target = fs::canonicalize(&f).unwrap();
        let err = list_dir(&root, &target).unwrap_err();
        assert!(matches!(err, IpcError::BadArgument(_)));
    }

    #[test]
    fn resolve_empty_returns_root() {
        let td = TempDir::new("resolve-empty");
        let root = canonicalize_root(td.path()).unwrap();
        assert_eq!(resolve(&root, "").unwrap(), root);
        assert_eq!(resolve(&root, ".").unwrap(), root);
    }

    #[test]
    fn resolve_relative_joins_under_root() {
        let td = TempDir::new("resolve-rel");
        fs::create_dir_all(td.path().join("sub")).unwrap();
        let root = canonicalize_root(td.path()).unwrap();
        let resolved = resolve(&root, "sub").unwrap();
        assert!(resolved.starts_with(&root));
        assert!(resolved.ends_with("sub"));
    }

    #[test]
    fn resolve_rejects_dotdot_escape() {
        let td_a = TempDir::new("resolve-esc-a");
        let _td_b = TempDir::new("resolve-esc-b");
        let root = canonicalize_root(td_a.path()).unwrap();
        let err = resolve(&root, "../").unwrap_err();
        assert!(
            matches!(err, IpcError::PathEscape(_) | IpcError::NotFound(_)),
            "expected PathEscape or NotFound, got {err:?}"
        );
    }

    #[test]
    fn resolve_rejects_absolute_outside_root() {
        let td_a = TempDir::new("resolve-abs-a");
        let td_b = TempDir::new("resolve-abs-b");
        fs::write(td_b.path().join("oops.txt"), "x").unwrap();
        let root = canonicalize_root(td_a.path()).unwrap();
        let outside = td_b.path().join("oops.txt");
        let err = resolve(&root, &outside.to_string_lossy()).unwrap_err();
        assert!(matches!(err, IpcError::PathEscape(_)));
    }
}
