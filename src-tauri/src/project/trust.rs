//! Persisted project trust state.
//!
//! v1 stores trust in a JSON file inside the OS app-data dir. Keying
//! is on the canonical absolute root path; renaming or moving the
//! folder loses trust, which is the conservative behavior.
//!
//! The store is loaded on demand and saved with an atomic
//! tmp-then-rename. We do not cache: trust changes are rare and the
//! file is small.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

const STORE_VERSION: u32 = 1;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct TrustFile {
    version: u32,
    #[serde(default)]
    trusted: Vec<TrustEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TrustEntry {
    path: String,
    #[serde(rename = "addedAtMs")]
    added_at_ms: u64,
}

/// Trust store backed by a single JSON file. Construct with the path
/// to the file (typically `<app-data>/trusted-projects.json`); the
/// file is allowed to be missing.
pub struct TrustStore {
    file: PathBuf,
    state: TrustFile,
}

impl TrustStore {
    /// Load from disk. A missing file is treated as an empty store. A
    /// corrupt file is logged and treated as empty so that a bad write
    /// can never lock the user out.
    pub fn load(file: PathBuf) -> Self {
        let state = match fs::read_to_string(&file) {
            Ok(text) => match serde_json::from_str::<TrustFile>(&text) {
                Ok(parsed) if parsed.version == STORE_VERSION => parsed,
                Ok(parsed) => {
                    tracing::warn!(
                        path = %file.display(),
                        version = parsed.version,
                        "trust store has unexpected version; treating as empty"
                    );
                    TrustFile::default()
                }
                Err(err) => {
                    tracing::warn!(path = %file.display(), error = %err, "trust store parse failed; treating as empty");
                    TrustFile::default()
                }
            },
            Err(err) if err.kind() == io::ErrorKind::NotFound => TrustFile::default(),
            Err(err) => {
                tracing::warn!(path = %file.display(), error = %err, "trust store read failed; treating as empty");
                TrustFile::default()
            }
        };
        Self { file, state }
    }

    /// Returns true iff `root` is in the store. `root` must be an
    /// already-canonicalized absolute path.
    pub fn is_trusted(&self, root: &Path) -> bool {
        let needle = root.to_string_lossy();
        self.state.trusted.iter().any(|e| e.path == needle)
    }

    /// Add `root` to the store and persist. No-op if already trusted.
    /// `root` must be an already-canonicalized absolute path.
    pub fn mark_trusted(&mut self, root: &Path) -> io::Result<()> {
        let key = root.to_string_lossy().into_owned();
        if self.state.trusted.iter().any(|e| e.path == key) {
            return Ok(());
        }
        self.state.trusted.push(TrustEntry {
            path: key,
            added_at_ms: now_ms(),
        });
        self.state.version = STORE_VERSION;
        self.persist()
    }

    fn persist(&self) -> io::Result<()> {
        if let Some(parent) = self.file.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(&self.state).map_err(io::Error::other)?;
        let tmp = self.file.with_extension("tmp");
        fs::write(&tmp, text)?;
        fs::rename(&tmp, &self.file)?;
        Ok(())
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn load_treats_missing_file_as_empty() {
        let td = TempDir::new("ts-miss");
        let file = td.path().join("trusted-projects.json");
        let store = TrustStore::load(file);
        assert!(!store.is_trusted(Path::new("/anywhere")));
    }

    #[test]
    fn mark_trusted_persists_and_reloads() {
        let td = TempDir::new("ts-roundtrip");
        let file = td.path().join("trusted-projects.json");
        let project = td.path().join("project");
        fs::create_dir_all(&project).unwrap();

        {
            let mut store = TrustStore::load(file.clone());
            store.mark_trusted(&project).unwrap();
            assert!(store.is_trusted(&project));
        }

        let store = TrustStore::load(file);
        assert!(store.is_trusted(&project));
    }

    #[test]
    fn mark_trusted_is_idempotent() {
        let td = TempDir::new("ts-idem");
        let file = td.path().join("trusted-projects.json");
        let project = td.path().join("project");
        fs::create_dir_all(&project).unwrap();

        let mut store = TrustStore::load(file);
        store.mark_trusted(&project).unwrap();
        store.mark_trusted(&project).unwrap();
        store.mark_trusted(&project).unwrap();
        // No public count; the assertion is just "no error and still trusted".
        assert!(store.is_trusted(&project));
    }

    #[test]
    fn corrupt_file_is_treated_as_empty() {
        let td = TempDir::new("ts-corrupt");
        let file = td.path().join("trusted-projects.json");
        fs::write(&file, "this is not json").unwrap();
        let store = TrustStore::load(file);
        assert!(!store.is_trusted(Path::new("/anywhere")));
    }
}
