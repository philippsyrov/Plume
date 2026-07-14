//! Cross-process serialization for the app-private user-memory store.

use std::fs;
use std::path::Path;

use super::types::MemoryStoreError;

const PROCESS_LOCK_FILE_NAME: &str = ".process.lock";

#[cfg(unix)]
pub(super) struct UserMemoryProcessLock {
    file: fs::File,
}

#[cfg(unix)]
impl Drop for UserMemoryProcessLock {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;

        // SAFETY: this guard owns a live descriptor until drop completes.
        // Closing it immediately afterward also releases flock if unlock fails.
        unsafe {
            libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

#[cfg(not(unix))]
pub(super) struct UserMemoryProcessLock;

#[cfg(unix)]
pub(super) fn acquire_user_memory_process_lock(
    user_memory_dir: &Path,
) -> Result<UserMemoryProcessLock, MemoryStoreError> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

    refuse_directory_symlink(user_memory_dir)?;
    fs::create_dir_all(user_memory_dir)
        .map_err(|error| MemoryStoreError(format!("create user memory directory: {error}")))?;
    refuse_directory_symlink(user_memory_dir)?;

    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(user_memory_dir.join(PROCESS_LOCK_FILE_NAME))
        .map_err(|error| MemoryStoreError(format!("open user memory process lock: {error}")))?;
    let guard = UserMemoryProcessLock { file };
    let metadata = guard
        .file
        .metadata()
        .map_err(|error| MemoryStoreError(format!("inspect user memory process lock: {error}")))?;
    if !metadata.is_file() || metadata.nlink() != 1 {
        return Err(MemoryStoreError(
            "user memory process lock is not a single-link regular file".into(),
        ));
    }

    // SAFETY: the guard owns a live descriptor. flock covers the full store access.
    if unsafe { libc::flock(guard.file.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return Err(last_os_error("lock user memory process state"));
    }
    // The create mode does not repair a pre-existing permissive file. Tighten
    // the opened inode only after it is locked, then verify the effective mode.
    if unsafe { libc::fchmod(guard.file.as_raw_fd(), 0o600) } != 0 {
        return Err(last_os_error("secure user memory process lock"));
    }
    let mode = guard
        .file
        .metadata()
        .map_err(|error| MemoryStoreError(format!("verify user memory process lock: {error}")))?
        .mode()
        & 0o777;
    if mode != 0o600 {
        return Err(MemoryStoreError(format!(
            "user memory process lock mode is {mode:o}; expected 600"
        )));
    }
    Ok(guard)
}

#[cfg(unix)]
fn refuse_directory_symlink(path: &Path) -> Result<(), MemoryStoreError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(MemoryStoreError(format!(
            "user memory directory at {} is a symlink; refusing to touch it",
            path.display()
        ))),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(MemoryStoreError(format!(
            "inspect user memory directory {}: {error}",
            path.display()
        ))),
    }
}

#[cfg(unix)]
fn last_os_error(action: &str) -> MemoryStoreError {
    MemoryStoreError(format!("{action}: {}", std::io::Error::last_os_error()))
}

#[cfg(not(unix))]
pub(super) fn acquire_user_memory_process_lock(
    _user_memory_dir: &Path,
) -> Result<UserMemoryProcessLock, MemoryStoreError> {
    Err(MemoryStoreError(
        "user memory requires cross-process file locking on this platform".into(),
    ))
}
