//! Descriptor-relative Unix storage primitives for project skills.

use std::ffi::{CStr, CString};
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path};
use std::time::{SystemTime, UNIX_EPOCH};

use super::parser::{parse, valid_slug};
use super::{SkillDocument, SkillsError, MAX_FILE_BYTES};

fn c_name(name: &str) -> Result<CString, SkillsError> {
    CString::new(name).map_err(|_| SkillsError("storage name contains NUL".into()))
}

fn last_error(context: &str) -> SkillsError {
    SkillsError(format!("{context}: {}", std::io::Error::last_os_error()))
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "visionos",
    target_os = "freebsd"
))]
unsafe fn errno_ptr() -> *mut libc::c_int {
    unsafe { libc::__error() }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
unsafe fn errno_ptr() -> *mut libc::c_int {
    unsafe { libc::__errno_location() }
}

#[cfg(any(target_os = "hurd", target_os = "redox"))]
unsafe fn errno_ptr() -> *mut libc::c_int {
    unsafe { libc::__errno_location() }
}

#[cfg(target_os = "haiku")]
unsafe fn errno_ptr() -> *mut libc::c_int {
    unsafe { libc::_errnop() }
}

#[cfg(target_os = "aix")]
unsafe fn errno_ptr() -> *mut libc::c_int {
    unsafe { libc::_Errno() }
}

#[cfg(target_os = "dragonfly")]
unsafe fn errno_ptr() -> *mut libc::c_int {
    unsafe { libc::__errno_location() }
}

#[cfg(any(target_os = "openbsd", target_os = "netbsd"))]
unsafe fn errno_ptr() -> *mut libc::c_int {
    unsafe { libc::__errno() }
}

#[cfg(any(target_os = "solaris", target_os = "illumos"))]
unsafe fn errno_ptr() -> *mut libc::c_int {
    unsafe { libc::___errno() }
}

fn readdir_finished(errno: libc::c_int) -> Result<(), SkillsError> {
    if errno == 0 {
        Ok(())
    } else {
        Err(SkillsError(format!(
            "read skills directory: {}",
            std::io::Error::from_raw_os_error(errno)
        )))
    }
}

pub(super) fn open_root(path: &Path) -> Result<OwnedFd, SkillsError> {
    open_root_with_hook(path, |_, _| {})
}

pub(super) fn open_root_with_hook<F>(
    path: &Path,
    mut before_step: F,
) -> Result<OwnedFd, SkillsError>
where
    F: FnMut(usize, &std::ffi::OsStr),
{
    if !path.is_absolute() {
        return Err(SkillsError("trusted project root must be absolute".into()));
    }
    let slash = c_name("/")?;
    let fd = unsafe {
        libc::open(
            slash.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(last_error(
            "open trusted project root without following links",
        ));
    }
    let mut current = unsafe { OwnedFd::from_raw_fd(fd) };
    let mut step = 0usize;
    for component in path.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(name) => {
                before_step(step, name);
                current = open_dir_at_os(current.as_raw_fd(), name)?;
                step += 1;
            }
            Component::CurDir | Component::ParentDir | Component::Prefix(_) => {
                return Err(SkillsError("trusted project root must be canonical".into()));
            }
        }
    }
    Ok(current)
}

pub(super) fn open_dir_at(parent: RawFd, name: &str) -> Result<OwnedFd, SkillsError> {
    open_dir_at_os(parent, std::ffi::OsStr::new(name))
}

fn open_dir_at_os(parent: RawFd, name: &std::ffi::OsStr) -> Result<OwnedFd, SkillsError> {
    let name = CString::new(name.as_bytes())
        .map_err(|_| SkillsError("directory component contains NUL".into()))?;
    let fd = unsafe {
        libc::openat(
            parent,
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(last_error(
            "open skill directory component without following links",
        ));
    }
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

pub(super) fn open_optional_dir_at(
    parent: RawFd,
    name: &str,
) -> Result<Option<OwnedFd>, SkillsError> {
    let c = c_name(name)?;
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    if unsafe {
        libc::fstatat(
            parent,
            c.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } != 0
    {
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::NotFound {
            return Ok(None);
        }
        return Err(SkillsError(format!(
            "inspect directory component {name}: {error}"
        )));
    }
    let stat = unsafe { stat.assume_init() };
    if (stat.st_mode & libc::S_IFMT) == libc::S_IFLNK {
        return Err(SkillsError(format!(
            "directory component {name} is a symlink"
        )));
    }
    open_dir_at(parent, name).map(Some)
}

fn fsync_dir(fd: RawFd, label: &str) -> Result<(), SkillsError> {
    if unsafe { libc::fsync(fd) } != 0 {
        return Err(last_error(&format!("fsync {label}")));
    }
    Ok(())
}

fn mkdir_open(parent: RawFd, name: &str) -> Result<(OwnedFd, bool), SkillsError> {
    let c = c_name(name)?;
    let created = if unsafe { libc::mkdirat(parent, c.as_ptr(), 0o700) } == 0 {
        if let Err(error) = fsync_dir(parent, "parent after mkdir") {
            unsafe { libc::unlinkat(parent, c.as_ptr(), libc::AT_REMOVEDIR) };
            let _ = fsync_dir(parent, "parent after failed mkdir rollback");
            return Err(error);
        }
        true
    } else {
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::AlreadyExists {
            return Err(SkillsError(format!("mkdir {name}: {error}")));
        }
        false
    };
    let dir = open_optional_dir_at(parent, name)?
        .ok_or_else(|| SkillsError(format!("directory component {name} disappeared")))?;
    Ok((dir, created))
}

pub(super) fn remove_empty_dir(parent: RawFd, name: &str) -> Result<bool, SkillsError> {
    let name = c_name(name)?;
    if unsafe { libc::unlinkat(parent, name.as_ptr(), libc::AT_REMOVEDIR) } != 0 {
        return Ok(false);
    }
    fsync_dir(parent, "skills directory after failed-create cleanup")?;
    Ok(true)
}

pub(super) fn open_store(
    root: &OwnedFd,
    create: bool,
) -> Result<Option<(OwnedFd, OwnedFd)>, SkillsError> {
    if create {
        let (plume, _) = mkdir_open(root.as_raw_fd(), ".plume")?;
        let (skills, _) = mkdir_open(plume.as_raw_fd(), "skills")?;
        return Ok(Some((plume, skills)));
    }
    let Some(plume) = open_optional_dir_at(root.as_raw_fd(), ".plume")? else {
        return Ok(None);
    };
    let Some(skills) = open_optional_dir_at(plume.as_raw_fd(), "skills")? else {
        return Ok(None);
    };
    Ok(Some((plume, skills)))
}

pub(super) fn list_names(dir: RawFd) -> Result<Vec<String>, SkillsError> {
    let duplicate = unsafe { libc::dup(dir) };
    if duplicate < 0 {
        return Err(last_error("duplicate skills directory fd"));
    }
    let stream = unsafe { libc::fdopendir(duplicate) };
    if stream.is_null() {
        unsafe { libc::close(duplicate) };
        return Err(last_error("open skills directory stream"));
    }
    let mut names = Vec::new();
    loop {
        unsafe { *errno_ptr() = 0 };
        let entry = unsafe { libc::readdir(stream) };
        if entry.is_null() {
            let errno = unsafe { *errno_ptr() };
            unsafe { libc::closedir(stream) };
            readdir_finished(errno)?;
            break;
        }
        let raw = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
        let name = raw.to_string_lossy();
        if name != "." && name != ".." {
            names.push(name.into_owned());
        }
    }
    Ok(names)
}

pub(super) fn read_document(skills: RawFd, slug: &str) -> Result<SkillDocument, SkillsError> {
    read_document_with_hook(skills, slug, || {})
}

pub(super) fn read_document_with_hook<F>(
    skills: RawFd,
    slug: &str,
    after_open: F,
) -> Result<SkillDocument, SkillsError>
where
    F: FnOnce(),
{
    if !valid_slug(slug) {
        return Err(SkillsError("invalid skill slug".into()));
    }
    let slug_dir = open_dir_at(skills, slug)?;
    validate_existing_final(slug_dir.as_raw_fd())?;
    let name = c_name("SKILL.md")?;
    let fd = unsafe {
        libc::openat(
            slug_dir.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(last_error("open SKILL.md without following links"));
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    let metadata = file
        .metadata()
        .map_err(|e| SkillsError(format!("fstat SKILL.md: {e}")))?;
    if !metadata.is_file() {
        return Err(SkillsError("SKILL.md is not a regular file".into()));
    }
    use std::os::unix::fs::MetadataExt;
    if metadata.nlink() != 1 {
        return Err(SkillsError("SKILL.md is hardlinked".into()));
    }
    if metadata.len() > MAX_FILE_BYTES as u64 {
        return Err(SkillsError(format!(
            "SKILL.md exceeds {MAX_FILE_BYTES} bytes"
        )));
    }
    after_open();
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut bytes)
        .map_err(|e| SkillsError(format!("read SKILL.md: {e}")))?;
    parse(slug, &bytes)
}

pub(super) fn validate_existing_final(slug_dir: RawFd) -> Result<(), SkillsError> {
    let name = c_name("SKILL.md")?;
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    if unsafe {
        libc::fstatat(
            slug_dir,
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } != 0
    {
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::NotFound {
            return Ok(());
        }
        return Err(SkillsError(format!("inspect existing SKILL.md: {error}")));
    }
    let stat = unsafe { stat.assume_init() };
    if (stat.st_mode & libc::S_IFMT) == libc::S_IFLNK {
        return Err(SkillsError("SKILL.md is a symlink".into()));
    }
    if stat.st_nlink != 1 {
        return Err(SkillsError("SKILL.md is hardlinked".into()));
    }
    Ok(())
}

pub(super) fn is_symlink_at(parent: RawFd, name: &str) -> Result<bool, SkillsError> {
    let name = c_name(name)?;
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    let result = unsafe {
        libc::fstatat(
            parent,
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result != 0 {
        return Err(last_error("inspect skill entry without following links"));
    }
    let stat = unsafe { stat.assume_init() };
    Ok((stat.st_mode & libc::S_IFMT) == libc::S_IFLNK)
}

pub(super) fn create_slug(skills: RawFd, slug: &str) -> Result<Option<OwnedFd>, SkillsError> {
    let (dir, created) = mkdir_open(skills, slug)?;
    Ok(created.then_some(dir))
}

pub(super) fn install<F>(slug_dir: RawFd, content: &[u8], before_link: F) -> Result<(), SkillsError>
where
    F: FnOnce() -> Result<(), SkillsError>,
{
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let temp_name = format!(".SKILL.md.{nonce}.tmp");
    let temp = c_name(&temp_name)?;
    let final_name = c_name("SKILL.md")?;
    let fd = unsafe {
        libc::openat(
            slug_dir,
            temp.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        return Err(last_error("create skill temp"));
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    let result = (|| {
        file.write_all(content)
            .map_err(|e| SkillsError(format!("write skill temp: {e}")))?;
        file.sync_all()
            .map_err(|e| SkillsError(format!("fsync skill temp: {e}")))?;
        drop(file);
        before_link()?;
        if unsafe { libc::linkat(slug_dir, temp.as_ptr(), slug_dir, final_name.as_ptr(), 0) } != 0 {
            return Err(last_error("install SKILL.md without overwrite"));
        }
        fsync_dir(slug_dir, "skill directory after install")?;
        if unsafe { libc::unlinkat(slug_dir, temp.as_ptr(), 0) } != 0 {
            return Err(last_error("unlink skill temp after install"));
        }
        fsync_dir(slug_dir, "skill directory after temp cleanup")?;
        Ok(())
    })();
    if result.is_err() {
        unsafe { libc::unlinkat(slug_dir, temp.as_ptr(), 0) };
        let _ = fsync_dir(slug_dir, "skill directory after failed temp cleanup");
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readdir_end_distinguishes_eof_from_error() {
        assert!(readdir_finished(0).is_ok());
        let error = readdir_finished(libc::EIO).unwrap_err();
        assert!(error.0.contains("read skills directory"));
    }

    #[test]
    fn root_walker_rejects_relative_and_noncanonical_components() {
        assert!(open_root(Path::new("relative/project")).is_err());
        assert!(open_root(Path::new("/tmp/../tmp")).is_err());
    }
}
