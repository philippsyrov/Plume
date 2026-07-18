//! Descriptor-rooted catalog filesystem authority.
//!
//! Every mutable descendant is opened from the app-owned catalog directory by
//! file descriptor. Names are only ever resolved by `*at` calls below a
//! no-follow directory descriptor, so a later symlink swap cannot redirect a
//! download, publication, or removal outside the catalog root.

use std::collections::BTreeMap;
use std::ffi::{CStr, CString, OsStr, OsString};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use sha2::{Digest, Sha256};

use super::super::catalog::{CatalogStore, InstallReceipt, QWEN_CATALOG_ID, QWEN_REVISION};
use super::{DownloadError, DownloadManifest, ManifestFile, COPY_BUFFER_BYTES};

const STAGING_NAME: &str = ".b3252a2f97102b1fb1571fec2c9b27219a8536be.part";
const PREPARED_NAME: &str = ".b3252a2f97102b1fb1571fec2c9b27219a8536be.prepared";
const LOCK_NAME: &str = ".catalog-download.lock";
const RECEIPT_NAME: &str = "install-receipt.json";
const MAX_RECEIPT_BYTES: u64 = 16 * 1024;

#[path = "catalog_download_publish.rs"]
mod publish;
#[cfg(test)]
pub(crate) use publish::with_publication_hook_for_test;

/// Open descriptor for the single fixed catalog root.
pub(crate) struct CatalogRoot {
    directory: File,
}

/// Stable resumable staging directory plus descriptors for every part that has
/// passed integrity verification. Keeping these descriptors prevents a later
/// name replacement from changing the source used to create the final inode.
pub(crate) struct StagingDir {
    directory: File,
    verified: BTreeMap<String, File>,
}

/// Non-blocking advisory process lock. The lock descriptor stays owned by the
/// operation from synchronous begin through registry terminal cleanup.
pub(crate) struct CatalogFilesystemLock {
    file: File,
}

impl Drop for CatalogFilesystemLock {
    fn drop(&mut self) {
        // Best effort is sufficient during process teardown; close releases it.
        unsafe {
            let _ = libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

impl CatalogRoot {
    pub(crate) fn open(store: &CatalogStore) -> Result<Self, DownloadError> {
        let app_data = open_absolute_directory(store.app_data_dir())?;
        let models = open_or_create_directory(&app_data, "models")?;
        let catalog = open_or_create_directory(&models, "catalog")?;
        let directory = open_or_create_directory(&catalog, QWEN_CATALOG_ID)?;
        Ok(Self { directory })
    }

    pub(crate) fn try_lock(&self) -> Result<CatalogFilesystemLock, DownloadError> {
        let file = open_or_create_regular(&self.directory, LOCK_NAME)?;
        validate_regular_unique(&file, LOCK_NAME)?;
        let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if result == 0 {
            Ok(CatalogFilesystemLock { file })
        } else if matches!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::EWOULDBLOCK)
        ) {
            Err(DownloadError::OperationActive {
                catalog_id: QWEN_CATALOG_ID.into(),
            })
        } else {
            Err(io_error(
                "catalog filesystem lock",
                std::io::Error::last_os_error(),
            ))
        }
    }

    pub(crate) fn open_staging(&self) -> Result<StagingDir, DownloadError> {
        self.remove_prepared_recovery()?;
        let directory = open_or_create_directory(&self.directory, STAGING_NAME)?;
        Ok(StagingDir {
            directory,
            verified: BTreeMap::new(),
        })
    }

    pub(crate) fn install_exists(&self) -> Result<bool, DownloadError> {
        match open_directory(&self.directory, OsStr::new(QWEN_REVISION)) {
            Ok(_) => Ok(true),
            Err(DownloadError::MissingPath) => Ok(false),
            Err(error) => Err(error),
        }
    }

    pub(crate) fn remove_verified_install(
        &self,
        store: &CatalogStore,
    ) -> Result<bool, DownloadError> {
        let install = match open_directory(&self.directory, OsStr::new(QWEN_REVISION)) {
            Ok(directory) => directory,
            Err(DownloadError::MissingPath) => return Ok(false),
            Err(error) => return Err(error),
        };
        if !receipt_is_valid(&install, store) {
            return Err(DownloadError::InstallNotVerified);
        }
        require_same_directory(&self.directory, QWEN_REVISION, &install)?;
        remove_directory_contents(&install)?;
        remove_directory_entry(&self.directory, QWEN_REVISION, &install)?;
        sync_directory(&self.directory)?;
        Ok(true)
    }

    #[cfg(test)]
    pub(crate) fn remove_verified_install_with_hook<F>(
        &self,
        store: &CatalogStore,
        hook: F,
    ) -> Result<bool, DownloadError>
    where
        F: FnOnce(),
    {
        let install = open_directory(&self.directory, OsStr::new(QWEN_REVISION))?;
        if !receipt_is_valid(&install, store) {
            return Err(DownloadError::InstallNotVerified);
        }
        hook();
        require_same_directory(&self.directory, QWEN_REVISION, &install)?;
        remove_directory_contents(&install)?;
        remove_directory_entry(&self.directory, QWEN_REVISION, &install)?;
        sync_directory(&self.directory)?;
        Ok(true)
    }

    fn remove_prepared_recovery(&self) -> Result<(), DownloadError> {
        match open_directory(&self.directory, OsStr::new(PREPARED_NAME)) {
            Ok(prepared) => {
                require_same_directory(&self.directory, PREPARED_NAME, &prepared)?;
                remove_directory_contents(&prepared)?;
                remove_directory_entry(&self.directory, PREPARED_NAME, &prepared)?;
                sync_directory(&self.directory)
            }
            Err(DownloadError::MissingPath) => Ok(()),
            Err(error) => Err(error),
        }
    }
}

impl StagingDir {
    pub(crate) fn preflight(&mut self, manifest: &DownloadManifest) -> Result<u64, DownloadError> {
        let allowed = manifest.files.iter().map(part_name).collect::<Vec<_>>();
        for name in directory_entries(&self.directory)? {
            if !allowed.iter().any(|allowed| OsStr::new(allowed) == name) {
                return Err(DownloadError::UnexpectedStagingPath {
                    path: name.to_string_lossy().into_owned(),
                });
            }
            let file = open_regular(&self.directory, &name)?;
            validate_regular_unique(&file, &name.to_string_lossy())?;
        }

        let mut initial = 0u64;
        for file in &manifest.files {
            let name = part_name(file);
            let Some(mut part) = self.open_existing_part(&name)? else {
                continue;
            };
            let length = regular_len_unique(&part, &file.path)?;
            if length > file.size {
                return Err(DownloadError::SizeMismatch {
                    path: file.path.clone(),
                    expected: file.size,
                    actual: length,
                });
            }
            if length == file.size {
                let digest = hash_verified_part(&mut part, &file.path)?;
                if digest == file.sha256 {
                    self.verified.insert(file.path.clone(), part);
                    initial = initial
                        .checked_add(length)
                        .ok_or(DownloadError::ByteCeiling)?;
                } else {
                    // This occurs before progress is published, so a retry cannot
                    // over-count or move progress backwards.
                    unlink_file(&self.directory, &name)?;
                }
            } else {
                initial = initial
                    .checked_add(length)
                    .ok_or(DownloadError::ByteCeiling)?;
            }
        }
        if initial > manifest.total_bytes {
            return Err(DownloadError::ByteCeiling);
        }
        Ok(initial)
    }

    pub(crate) fn verified(&self, path: &str) -> bool {
        self.verified.contains_key(path)
    }

    pub(crate) fn open_part_for_resume(
        &self,
        file: &ManifestFile,
    ) -> Result<(File, u64), DownloadError> {
        let name = part_name(file);
        let part = match self.open_existing_part(&name)? {
            Some(file) => file,
            None => create_regular(&self.directory, &name)?,
        };
        let length = regular_len_unique(&part, &file.path)?;
        if length > file.size {
            return Err(DownloadError::SizeMismatch {
                path: file.path.clone(),
                expected: file.size,
                actual: length,
            });
        }
        Ok((part, length))
    }

    pub(crate) fn remember_verified(
        &mut self,
        file: &ManifestFile,
        part: File,
    ) -> Result<(), DownloadError> {
        validate_regular_exact(&part, file, "downloaded staging part")?;
        self.verified.insert(file.path.clone(), part);
        Ok(())
    }

    pub(crate) fn truncate_part_for_restart(
        &self,
        part: &mut File,
        file: &ManifestFile,
    ) -> Result<(), DownloadError> {
        validate_regular_unique(part, &file.path)?;
        part.set_len(0)
            .map_err(|error| io_error(&file.path, error))?;
        part.seek(SeekFrom::Start(0))
            .map_err(|error| io_error(&file.path, error))?;
        Ok(())
    }

    fn open_existing_part(&self, name: &str) -> Result<Option<File>, DownloadError> {
        match open_regular(&self.directory, OsStr::new(name)) {
            Ok(file) => Ok(Some(file)),
            Err(DownloadError::MissingPath) => Ok(None),
            Err(error) => Err(error),
        }
    }
}

pub(crate) fn acquire_catalog_lock(
    store: &CatalogStore,
) -> Result<CatalogFilesystemLock, DownloadError> {
    CatalogRoot::open(store)?.try_lock()
}

fn open_absolute_directory(path: &Path) -> Result<File, DownloadError> {
    if !path.is_absolute() {
        return Err(DownloadError::NotDirectory {
            path: path.display().to_string(),
        });
    }
    let root = open_directory_path(Path::new("/"))?;
    let mut directory = root;
    for component in path.components() {
        if let std::path::Component::Normal(name) = component {
            directory = open_directory(&directory, name)?;
        } else if !matches!(component, std::path::Component::RootDir) {
            return Err(DownloadError::NotDirectory {
                path: path.display().to_string(),
            });
        }
    }
    Ok(directory)
}

fn open_directory_path(path: &Path) -> Result<File, DownloadError> {
    let name = CString::new(path.as_os_str().as_bytes()).expect("absolute slash has no NUL");
    let fd = unsafe {
        libc::open(
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    file_from_fd(fd, path.display().to_string())
}

fn open_or_create_directory(parent: &File, name: &str) -> Result<File, DownloadError> {
    match open_directory(parent, OsStr::new(name)) {
        Ok(directory) => Ok(directory),
        Err(DownloadError::MissingPath) => {
            let name_c = c_name(OsStr::new(name))?;
            let result = unsafe { libc::mkdirat(parent.as_raw_fd(), name_c.as_ptr(), 0o700) };
            if result != 0
                && std::io::Error::last_os_error().kind() != std::io::ErrorKind::AlreadyExists
            {
                return Err(io_error(name, std::io::Error::last_os_error()));
            }
            open_directory(parent, OsStr::new(name))
        }
        Err(error) => Err(error),
    }
}

fn open_directory(parent: &File, name: &OsStr) -> Result<File, DownloadError> {
    let name_c = c_name(name)?;
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name_c.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        let open_error = std::io::Error::last_os_error();
        if entry_is_symlink(parent, name)? {
            return Err(DownloadError::SymlinkPath {
                path: name.to_string_lossy().into_owned(),
            });
        }
        if open_error.kind() == std::io::ErrorKind::NotFound {
            return Err(DownloadError::MissingPath);
        }
        return Err(io_error(&name.to_string_lossy(), open_error));
    }
    let file = file_from_fd(fd, name.to_string_lossy().into_owned())?;
    if !file
        .metadata()
        .map_err(|error| io_error(&name.to_string_lossy(), error))?
        .is_dir()
    {
        return Err(DownloadError::NotDirectory {
            path: name.to_string_lossy().into_owned(),
        });
    }
    Ok(file)
}

fn open_regular(parent: &File, name: &OsStr) -> Result<File, DownloadError> {
    let name_c = c_name(name)?;
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name_c.as_ptr(),
            libc::O_RDWR | libc::O_APPEND | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        let open_error = std::io::Error::last_os_error();
        if entry_is_symlink(parent, name)? {
            return Err(DownloadError::SymlinkPath {
                path: name.to_string_lossy().into_owned(),
            });
        }
        if open_error.kind() == std::io::ErrorKind::NotFound {
            return Err(DownloadError::MissingPath);
        }
        return Err(io_error(&name.to_string_lossy(), open_error));
    }
    file_from_fd(fd, name.to_string_lossy().into_owned())
}

fn open_or_create_regular(parent: &File, name: &str) -> Result<File, DownloadError> {
    match open_regular(parent, OsStr::new(name)) {
        Ok(file) => Ok(file),
        Err(DownloadError::MissingPath) => match create_regular(parent, name) {
            Ok(file) => Ok(file),
            // The lock is the sole exception to exclusive publication output:
            // another cooperating process may have created it while we raced
            // from the failed open to O_EXCL creation. Re-open only this fixed
            // lock name, then validate it before flocking.
            Err(DownloadError::AlreadyExists { .. }) => open_regular(parent, OsStr::new(name)),
            Err(error) => Err(error),
        },
        Err(error) => Err(error),
    }
}

fn create_regular(parent: &File, name: &str) -> Result<File, DownloadError> {
    let name_c = c_name(OsStr::new(name))?;
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name_c.as_ptr(),
            libc::O_RDWR
                | libc::O_APPEND
                | libc::O_CREAT
                | libc::O_EXCL
                | libc::O_NOFOLLOW
                | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::AlreadyExists {
        // A prepared output must be created by this exact O_EXCL call. Opening
        // an existing name here would turn a replacement or hardlink race into
        // a writable descriptor, so callers fail and recover the prepared dir.
        return Err(DownloadError::AlreadyExists { path: name.into() });
    }
    file_from_fd(fd, name.into())
}

fn validate_regular_unique(file: &File, label: &str) -> Result<(), DownloadError> {
    let metadata = file.metadata().map_err(|error| io_error(label, error))?;
    if !metadata.is_file() || metadata.nlink() != 1 {
        return Err(DownloadError::UnexpectedStagingPath { path: label.into() });
    }
    Ok(())
}

fn regular_len_unique(file: &File, label: &str) -> Result<u64, DownloadError> {
    validate_regular_unique(file, label)?;
    file.metadata()
        .map(|metadata| metadata.len())
        .map_err(|error| io_error(label, error))
}

fn validate_regular_exact(
    file: &File,
    expected: &ManifestFile,
    label: &str,
) -> Result<(), DownloadError> {
    let length = regular_len_unique(file, label)?;
    if length != expected.size {
        return Err(DownloadError::SizeMismatch {
            path: expected.path.clone(),
            expected: expected.size,
            actual: length,
        });
    }
    Ok(())
}

pub(crate) fn hash_verified_part(file: &mut File, label: &str) -> Result<String, DownloadError> {
    file.seek(SeekFrom::Start(0))
        .map_err(|error| io_error(label, error))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; COPY_BUFFER_BYTES];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| io_error(label, error))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    file.seek(SeekFrom::End(0))
        .map_err(|error| io_error(label, error))?;
    Ok(format!("{:x}", hasher.finalize()))
}

fn receipt_is_valid(directory: &File, store: &CatalogStore) -> bool {
    let Ok(mut receipt_file) = open_regular(directory, OsStr::new(RECEIPT_NAME)) else {
        return false;
    };
    let Ok(length) = regular_len_unique(&receipt_file, RECEIPT_NAME) else {
        return false;
    };
    if length > MAX_RECEIPT_BYTES {
        return false;
    }
    if receipt_file.seek(SeekFrom::Start(0)).is_err() {
        return false;
    }
    let mut bytes = Vec::with_capacity(length as usize);
    if receipt_file
        .take(MAX_RECEIPT_BYTES + 1)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return false;
    }
    let Ok(receipt) = serde_json::from_slice::<InstallReceipt>(&bytes) else {
        return false;
    };
    receipt.catalog_id == QWEN_CATALOG_ID
        && receipt.revision == QWEN_REVISION
        && receipt.manifest_sha256 == store.expected_manifest_sha256()
}

fn remove_directory_contents(directory: &File) -> Result<(), DownloadError> {
    for name in directory_entries(directory)? {
        let file = open_regular(directory, &name).or_else(|error| match error {
            DownloadError::NotDirectory { .. } | DownloadError::MissingPath => Err(error),
            _ => open_directory(directory, &name),
        });
        match file {
            Ok(file)
                if file
                    .metadata()
                    .map_err(|error| io_error("catalog entry", error))?
                    .is_dir() =>
            {
                require_same_directory(directory, &name.to_string_lossy(), &file)?;
                remove_directory_contents(&file)?;
                remove_directory_entry(directory, &name.to_string_lossy(), &file)?;
            }
            Ok(file) => {
                validate_regular_unique(&file, &name.to_string_lossy())?;
                unlink_file(directory, &name.to_string_lossy())?;
            }
            Err(error) => return Err(error),
        }
    }
    sync_directory(directory)
}

fn require_same_directory(parent: &File, name: &str, expected: &File) -> Result<(), DownloadError> {
    let current = open_directory(parent, OsStr::new(name))?;
    let expected_metadata = expected.metadata().map_err(|error| io_error(name, error))?;
    let current_metadata = current.metadata().map_err(|error| io_error(name, error))?;
    if expected_metadata.dev() != current_metadata.dev()
        || expected_metadata.ino() != current_metadata.ino()
    {
        return Err(DownloadError::PathSwap { path: name.into() });
    }
    Ok(())
}

fn remove_directory_entry(parent: &File, name: &str, expected: &File) -> Result<(), DownloadError> {
    require_same_directory(parent, name, expected)?;
    let name = c_name(OsStr::new(name))?;
    let result = unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR) };
    if result != 0 {
        return Err(io_error(
            "catalog directory removal",
            std::io::Error::last_os_error(),
        ));
    }
    Ok(())
}

fn unlink_file(parent: &File, name: &str) -> Result<(), DownloadError> {
    let name = c_name(OsStr::new(name))?;
    let result = unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), 0) };
    if result != 0 {
        return Err(io_error(
            "catalog file removal",
            std::io::Error::last_os_error(),
        ));
    }
    Ok(())
}

fn rename_directory_no_replace(parent: &File, from: &str, to: &str) -> Result<(), DownloadError> {
    let from = c_name(OsStr::new(from))?;
    let to = c_name(OsStr::new(to))?;
    #[cfg(target_os = "macos")]
    let result = unsafe {
        renameatx_np(
            parent.as_raw_fd(),
            from.as_ptr(),
            parent.as_raw_fd(),
            to.as_ptr(),
            0x0000_0004,
        )
    };
    #[cfg(target_os = "linux")]
    let result = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            parent.as_raw_fd(),
            from.as_ptr(),
            parent.as_raw_fd(),
            to.as_ptr(),
            1,
        ) as i32
    };
    if result != 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            return Err(DownloadError::InstallExists);
        }
        return Err(io_error("catalog atomic publish", error));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn renameatx_np(
        fromfd: RawFd,
        from: *const libc::c_char,
        tofd: RawFd,
        to: *const libc::c_char,
        flags: u32,
    ) -> libc::c_int;
}

fn sync_directory(directory: &File) -> Result<(), DownloadError> {
    directory
        .sync_all()
        .map_err(|error| io_error("catalog directory sync", error))
}

fn directory_entries(directory: &File) -> Result<Vec<OsString>, DownloadError> {
    let duplicate = unsafe { libc::dup(directory.as_raw_fd()) };
    if duplicate < 0 {
        return Err(io_error(
            "catalog directory duplicate",
            std::io::Error::last_os_error(),
        ));
    }
    let raw = unsafe { libc::fdopendir(duplicate) };
    if raw.is_null() {
        unsafe {
            libc::close(duplicate);
        }
        return Err(io_error(
            "catalog directory iterator",
            std::io::Error::last_os_error(),
        ));
    }
    let mut entries = Vec::new();
    loop {
        clear_errno();
        let entry = unsafe { libc::readdir(raw) };
        if entry.is_null() {
            let error = std::io::Error::last_os_error();
            unsafe {
                libc::closedir(raw);
            }
            if error.raw_os_error().unwrap_or(0) != 0 {
                return Err(io_error("catalog directory iterator", error));
            }
            return Ok(entries);
        }
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
        if name != b"." && name != b".." {
            entries.push(OsString::from_vec(name.to_vec()));
        }
    }
}

#[cfg(target_os = "macos")]
fn clear_errno() {
    unsafe {
        *libc::__error() = 0;
    }
}

#[cfg(target_os = "linux")]
fn clear_errno() {
    unsafe {
        *libc::__errno_location() = 0;
    }
}

fn part_name(file: &ManifestFile) -> String {
    format!("{}.part", file.path)
}

fn c_name(name: &OsStr) -> Result<CString, DownloadError> {
    CString::new(name.as_bytes()).map_err(|_| DownloadError::UnexpectedStagingPath {
        path: name.to_string_lossy().into_owned(),
    })
}

fn entry_is_symlink(parent: &File, name: &OsStr) -> Result<bool, DownloadError> {
    let name_c = c_name(name)?;
    let mut stat = unsafe { std::mem::zeroed::<libc::stat>() };
    let result = unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name_c.as_ptr(),
            &mut stat,
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result == 0 {
        return Ok((stat.st_mode & libc::S_IFMT) == libc::S_IFLNK);
    }
    let error = std::io::Error::last_os_error();
    if error.kind() == std::io::ErrorKind::NotFound {
        return Ok(false);
    }
    Err(io_error(&name.to_string_lossy(), error))
}

fn file_from_fd(fd: RawFd, label: String) -> Result<File, DownloadError> {
    if fd < 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ELOOP) {
            return Err(DownloadError::SymlinkPath { path: label });
        }
        return Err(io_error(&label, error));
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn io_error(path: &str, error: std::io::Error) -> DownloadError {
    DownloadError::Io {
        path: path.into(),
        reason: error.to_string(),
    }
}
