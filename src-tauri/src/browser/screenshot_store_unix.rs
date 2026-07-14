//! Descriptor-relative storage for browser screenshot evidence on Unix.

use std::ffi::{CStr, CString, OsStr};
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path};

use super::screenshot_evidence::BrowserScreenshotError;

pub(super) struct ScreenshotDir(OwnedFd);

fn error(context: &str) -> BrowserScreenshotError {
    BrowserScreenshotError(format!("{context}: {}", std::io::Error::last_os_error()))
}

fn name(value: &str) -> Result<CString, BrowserScreenshotError> {
    CString::new(value).map_err(|_| BrowserScreenshotError("storage name contains NUL".into()))
}

fn open_dir_at(parent: RawFd, value: &OsStr) -> Result<OwnedFd, BrowserScreenshotError> {
    let value = CString::new(value.as_bytes())
        .map_err(|_| BrowserScreenshotError("directory name contains NUL".into()))?;
    let fd = unsafe {
        libc::openat(
            parent,
            value.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(error("open screenshot directory without following links"));
    }
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

fn open_root(path: &Path) -> Result<OwnedFd, BrowserScreenshotError> {
    if !path.is_absolute() {
        return Err(BrowserScreenshotError(
            "trusted project root must be absolute".into(),
        ));
    }
    let path = path.canonicalize().map_err(|error| {
        BrowserScreenshotError(format!("canonicalize trusted project root: {error}"))
    })?;
    let slash = name("/")?;
    let fd = unsafe {
        libc::open(
            slash.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(error("open filesystem root"));
    }
    let mut current = unsafe { OwnedFd::from_raw_fd(fd) };
    for component in path.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(part) => current = open_dir_at(current.as_raw_fd(), part)?,
            _ => {
                return Err(BrowserScreenshotError(
                    "trusted project root must be canonical".into(),
                ))
            }
        }
    }
    Ok(current)
}

fn open_or_create_dir(
    parent: RawFd,
    value: &str,
    create: bool,
) -> Result<Option<OwnedFd>, BrowserScreenshotError> {
    let value_c = name(value)?;
    let fd = unsafe {
        libc::openat(
            parent,
            value_c.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd >= 0 {
        return Ok(Some(unsafe { OwnedFd::from_raw_fd(fd) }));
    }
    let open_error = std::io::Error::last_os_error();
    if open_error.kind() != std::io::ErrorKind::NotFound || !create {
        return if open_error.kind() == std::io::ErrorKind::NotFound {
            Ok(None)
        } else {
            Err(BrowserScreenshotError(format!(
                "open screenshot directory {value}: {open_error}"
            )))
        };
    }
    if unsafe { libc::mkdirat(parent, value_c.as_ptr(), 0o700) } != 0 {
        let mkdir_error = std::io::Error::last_os_error();
        if mkdir_error.kind() != std::io::ErrorKind::AlreadyExists {
            return Err(BrowserScreenshotError(format!(
                "create screenshot directory {value}: {mkdir_error}"
            )));
        }
    }
    open_dir_at(parent, OsStr::new(value)).map(Some)
}

pub(super) fn open(
    project_root: &Path,
    create: bool,
) -> Result<Option<ScreenshotDir>, BrowserScreenshotError> {
    let root = open_root(project_root)?;
    let Some(plume) = open_or_create_dir(root.as_raw_fd(), ".plume", create)? else {
        return Ok(None);
    };
    let Some(evidence) = open_or_create_dir(plume.as_raw_fd(), "browser-evidence", create)? else {
        return Ok(None);
    };
    let Some(screenshots) = open_or_create_dir(evidence.as_raw_fd(), "screenshots", create)? else {
        return Ok(None);
    };
    Ok(Some(ScreenshotDir(screenshots)))
}

impl ScreenshotDir {
    pub(super) fn read(
        &self,
        value: &str,
        cap: u64,
    ) -> Result<Option<Vec<u8>>, BrowserScreenshotError> {
        self.read_with_hook(value, cap, || {})
    }

    pub(super) fn read_with_hook<F>(
        &self,
        value: &str,
        cap: u64,
        after_open: F,
    ) -> Result<Option<Vec<u8>>, BrowserScreenshotError>
    where
        F: FnOnce(),
    {
        let value = name(value)?;
        let fd = unsafe {
            libc::openat(
                self.0.as_raw_fd(),
                value.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            let open_error = std::io::Error::last_os_error();
            return if open_error.kind() == std::io::ErrorKind::NotFound {
                Ok(None)
            } else {
                Err(BrowserScreenshotError(format!(
                    "open screenshot file without following links: {open_error}"
                )))
            };
        }
        let mut file = unsafe { File::from_raw_fd(fd) };
        let metadata = file
            .metadata()
            .map_err(|error| BrowserScreenshotError(format!("inspect screenshot file: {error}")))?;
        if !metadata.is_file() || metadata.nlink() != 1 {
            return Err(BrowserScreenshotError(
                "screenshot file is not a single-link regular file".into(),
            ));
        }
        if metadata.len() > cap {
            return Err(BrowserScreenshotError(
                "screenshot file is oversized".into(),
            ));
        }
        after_open();
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        file.read_to_end(&mut bytes)
            .map_err(|error| BrowserScreenshotError(format!("read screenshot file: {error}")))?;
        if bytes.len() as u64 != metadata.len() {
            return Err(BrowserScreenshotError(
                "screenshot file changed while being read".into(),
            ));
        }
        Ok(Some(bytes))
    }

    pub(super) fn write_new(
        &self,
        value: &str,
        bytes: &[u8],
    ) -> Result<(), BrowserScreenshotError> {
        let final_name = name(value)?;
        let temp_value = format!(".{value}.{}.tmp", super::screenshot_evidence::next_nonce());
        let temp_name = name(&temp_value)?;
        let fd = unsafe {
            libc::openat(
                self.0.as_raw_fd(),
                temp_name.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        };
        if fd < 0 {
            return Err(error("create screenshot temp file"));
        }
        let mut file = unsafe { File::from_raw_fd(fd) };
        let result = (|| {
            file.write_all(bytes).map_err(|error| {
                BrowserScreenshotError(format!("write screenshot temp file: {error}"))
            })?;
            file.sync_all().map_err(|error| {
                BrowserScreenshotError(format!("sync screenshot temp file: {error}"))
            })?;
            drop(file);
            if unsafe {
                libc::linkat(
                    self.0.as_raw_fd(),
                    temp_name.as_ptr(),
                    self.0.as_raw_fd(),
                    final_name.as_ptr(),
                    0,
                )
            } != 0
            {
                return Err(error("commit screenshot file without overwrite"));
            }
            if unsafe { libc::unlinkat(self.0.as_raw_fd(), temp_name.as_ptr(), 0) } != 0 {
                return Err(error("remove screenshot temp file"));
            }
            if unsafe { libc::fsync(self.0.as_raw_fd()) } != 0 {
                return Err(error("sync screenshot directory"));
            }
            Ok(())
        })();
        if result.is_err() {
            unsafe { libc::unlinkat(self.0.as_raw_fd(), temp_name.as_ptr(), 0) };
        }
        result
    }

    pub(super) fn remove(&self, value: &str) {
        if let Ok(value) = name(value) {
            unsafe { libc::unlinkat(self.0.as_raw_fd(), value.as_ptr(), 0) };
        }
    }

    pub(super) fn usage(&self) -> Result<(usize, u64), BrowserScreenshotError> {
        let duplicate = unsafe { libc::dup(self.0.as_raw_fd()) };
        if duplicate < 0 {
            return Err(error("duplicate screenshot directory"));
        }
        let stream = unsafe { libc::fdopendir(duplicate) };
        if stream.is_null() {
            unsafe { libc::close(duplicate) };
            return Err(error("open screenshot directory stream"));
        }
        let mut names = Vec::new();
        loop {
            let entry = unsafe { libc::readdir(stream) };
            if entry.is_null() {
                break;
            }
            let raw = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
            let value = raw.to_string_lossy();
            if value.ends_with(".png") {
                names.push(value.into_owned());
            }
        }
        unsafe { libc::closedir(stream) };
        let mut bytes = 0_u64;
        for value in &names {
            let file = self
                .read(
                    value,
                    super::screenshot_evidence::BROWSER_SCREENSHOT_BYTE_CAP as u64,
                )?
                .ok_or_else(|| {
                    BrowserScreenshotError("screenshot disappeared during scan".into())
                })?;
            bytes = bytes.saturating_add(file.len() as u64);
        }
        Ok((names.len(), bytes))
    }
}
