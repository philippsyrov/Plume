//! Owner-only, durable filesystem writes for app-private user memory.

use std::fs;
use std::io::Write;
use std::path::Path;

use super::types::MemoryStoreError;

const TEMP_ENTRIES_FILE_NAME: &str = ".entries.jsonl.plume-user-memory.tmp";

pub(super) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), MemoryStoreError> {
    let parent = path.parent().ok_or_else(|| {
        MemoryStoreError(format!("user memory path {} has no parent", path.display()))
    })?;
    let temporary = parent.join(TEMP_ENTRIES_FILE_NAME);
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    let mut file = options.open(&temporary).map_err(|error| {
        MemoryStoreError(format!(
            "create exclusive temp {}: {error}",
            temporary.display()
        ))
    })?;
    let result = (|| {
        secure_temp_file(&file, &temporary)?;
        file.write_all(bytes).map_err(|error| {
            MemoryStoreError(format!("write temp {}: {error}", temporary.display()))
        })?;
        file.sync_all().map_err(|error| {
            MemoryStoreError(format!("sync temp {}: {error}", temporary.display()))
        })?;
        fs::rename(&temporary, path)
            .map_err(|error| MemoryStoreError(format!("rename -> {}: {error}", path.display())))?;
        sync_directory(parent)
    })();
    if result.is_err() && temporary.exists() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(unix)]
fn secure_temp_file(file: &fs::File, path: &Path) -> Result<(), MemoryStoreError> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let metadata = file
        .metadata()
        .map_err(|error| MemoryStoreError(format!("inspect temp {}: {error}", path.display())))?;
    if !metadata.is_file() || metadata.nlink() != 1 {
        return Err(MemoryStoreError(format!(
            "user memory temp {} is not a single-link regular file",
            path.display()
        )));
    }
    if unsafe { libc::fchmod(file.as_raw_fd(), 0o600) } != 0 {
        return Err(MemoryStoreError(format!(
            "secure temp {}: {}",
            path.display(),
            std::io::Error::last_os_error()
        )));
    }
    let mode = file
        .metadata()
        .map_err(|error| MemoryStoreError(format!("verify temp {}: {error}", path.display())))?
        .permissions()
        .mode()
        & 0o777;
    if mode != 0o600 {
        return Err(MemoryStoreError(format!(
            "user memory temp {} mode is {mode:o}; expected 600",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(not(unix))]
fn secure_temp_file(_: &fs::File, _: &Path) -> Result<(), MemoryStoreError> {
    Err(MemoryStoreError(
        "user memory requires owner-only file modes on this platform".into(),
    ))
}

pub(super) fn sync_directory(path: &Path) -> Result<(), MemoryStoreError> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    let directory = options.open(path).map_err(|error| {
        MemoryStoreError(format!(
            "open directory {} for sync: {error}",
            path.display()
        ))
    })?;
    directory
        .sync_all()
        .map_err(|error| MemoryStoreError(format!("sync directory {}: {error}", path.display())))
}
