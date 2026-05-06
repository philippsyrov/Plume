//! `fs.read` — display reads only.
//!
//! Display ≠ prompt. The prompt-read path will produce `RedactedContent`
//! through a different module; this one's output is safe to render in
//! the editor but **not** to feed to a model. The two paths are
//! deliberately split so the secret redactor stays a single chokepoint
//! when it lands.

use std::fs;
use std::path::Path;

use serde::Serialize;

use crate::error::IpcError;
use crate::fs::policy::{block_reason, DISPLAY_READ_MAX_BYTES};
use crate::safety::path::ensure_no_hardlink_alias;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FileEncoding {
    #[serde(rename = "utf-8")]
    Utf8,
    Binary,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileContent {
    pub content: String,
    pub encoding: FileEncoding,
    pub bytes: u64,
}

/// Read `target` for display. `target` and `root` must be canonical
/// absolute paths and the caller must have already confirmed `target`
/// is inside `root`.
pub fn read_file(root: &Path, target: &Path) -> Result<FileContent, IpcError> {
    debug_assert!(
        target.starts_with(root),
        "read_file expects target inside root"
    );

    if let Some(reason) = block_reason(target, root) {
        return Err(IpcError::Blocked(reason));
    }

    let metadata = fs::symlink_metadata(target).map_err(|err| io_to_ipc(target, err))?;
    if !metadata.is_file() {
        return Err(IpcError::BadArgument(format!(
            "fs.read target is not a regular file: {}",
            target.display()
        )));
    }

    ensure_no_hardlink_alias(target).map_err(IpcError::from)?;

    let bytes_on_disk = metadata.len();
    if bytes_on_disk > DISPLAY_READ_MAX_BYTES {
        return Err(IpcError::Blocked(format!(
            "{} is {} bytes; display reads are capped at {} bytes",
            target.display(),
            bytes_on_disk,
            DISPLAY_READ_MAX_BYTES
        )));
    }

    let raw = fs::read(target).map_err(|err| io_to_ipc(target, err))?;
    Ok(decode(raw, bytes_on_disk))
}

fn decode(raw: Vec<u8>, bytes: u64) -> FileContent {
    // Quick null-byte check catches typical binaries without forcing
    // a full UTF-8 walk on, say, a 1.5 MB image. Real text files almost
    // never contain NUL.
    if raw.contains(&0u8) {
        return FileContent {
            content: String::new(),
            encoding: FileEncoding::Binary,
            bytes,
        };
    }

    match String::from_utf8(raw) {
        Ok(text) => FileContent {
            content: text,
            encoding: FileEncoding::Utf8,
            bytes,
        },
        Err(_) => FileContent {
            content: String::new(),
            encoding: FileEncoding::Binary,
            bytes,
        },
    }
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
    fn reads_utf8_text_file() {
        let td = TempDir::new("read-utf8");
        let f = td.path().join("hello.txt");
        fs::write(&f, "héllo, world").unwrap();
        let root = canonicalize_root(td.path()).unwrap();
        let target = fs::canonicalize(&f).unwrap();
        let content = read_file(&root, &target).unwrap();
        assert_eq!(content.encoding, FileEncoding::Utf8);
        assert_eq!(content.content, "héllo, world");
        assert_eq!(content.bytes, "héllo, world".len() as u64);
    }

    #[test]
    fn reports_binary_for_files_with_nul() {
        let td = TempDir::new("read-binary");
        let f = td.path().join("bin.dat");
        fs::write(&f, [b'P', b'K', 0u8, 1, 2, 3]).unwrap();
        let root = canonicalize_root(td.path()).unwrap();
        let target = fs::canonicalize(&f).unwrap();
        let content = read_file(&root, &target).unwrap();
        assert_eq!(content.encoding, FileEncoding::Binary);
        assert!(content.content.is_empty());
        assert_eq!(content.bytes, 6);
    }

    #[test]
    fn blocks_dot_env() {
        let td = TempDir::new("read-env");
        let f = td.path().join(".env");
        fs::write(&f, "API_KEY=secret").unwrap();
        let root = canonicalize_root(td.path()).unwrap();
        let target = fs::canonicalize(&f).unwrap();
        let err = read_file(&root, &target).unwrap_err();
        assert!(matches!(err, IpcError::Blocked(_)), "got {err:?}");
    }

    #[test]
    fn blocks_git_objects() {
        let td = TempDir::new("read-git-obj");
        fs::create_dir_all(td.path().join(".git/objects/ab")).unwrap();
        let f = td.path().join(".git/objects/ab/cdef");
        fs::write(&f, [0u8, 1, 2]).unwrap();
        let root = canonicalize_root(td.path()).unwrap();
        let target = fs::canonicalize(&f).unwrap();
        let err = read_file(&root, &target).unwrap_err();
        assert!(matches!(err, IpcError::Blocked(_)), "got {err:?}");
    }

    #[test]
    fn blocks_oversize_files() {
        let td = TempDir::new("read-big");
        let f = td.path().join("big.txt");
        // Write just over the cap. Slow-ish but only one I/O.
        let big = vec![b'a'; (DISPLAY_READ_MAX_BYTES + 1) as usize];
        fs::write(&f, big).unwrap();
        let root = canonicalize_root(td.path()).unwrap();
        let target = fs::canonicalize(&f).unwrap();
        let err = read_file(&root, &target).unwrap_err();
        assert!(matches!(err, IpcError::Blocked(_)), "got {err:?}");
    }

    #[cfg(unix)]
    #[test]
    fn blocks_hardlink_alias() {
        let td = TempDir::new("read-hardlink");
        let original = td.path().join("orig.txt");
        fs::write(&original, "x").unwrap();
        let alias = td.path().join("alias.txt");
        fs::hard_link(&original, &alias).unwrap();
        let root = canonicalize_root(td.path()).unwrap();
        let target = fs::canonicalize(&original).unwrap();
        let err = read_file(&root, &target).unwrap_err();
        assert!(matches!(err, IpcError::PathEscape(_)), "got {err:?}");
    }

    #[test]
    fn rejects_directory_target() {
        let td = TempDir::new("read-dir");
        fs::create_dir_all(td.path().join("subdir")).unwrap();
        let root = canonicalize_root(td.path()).unwrap();
        let target = fs::canonicalize(td.path().join("subdir")).unwrap();
        let err = read_file(&root, &target).unwrap_err();
        assert!(matches!(err, IpcError::BadArgument(_)), "got {err:?}");
    }
}
