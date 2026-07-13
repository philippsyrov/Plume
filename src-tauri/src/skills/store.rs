use std::path::Path;
use std::sync::{Mutex, OnceLock};

use super::parser::{canonical, valid_slug};
use super::{
    SkillApplyResponse, SkillDocument, SkillIndex, SkillInput, SkillInvalid, SkillMetadata,
    SkillPreview, SkillsError, MAX_SKILLS,
};

pub(super) fn skill_mutex() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[cfg(not(unix))]
fn unsupported<T>() -> Result<T, SkillsError> {
    Err(SkillsError(
        "project skill storage requires descriptor-relative Unix filesystem operations".into(),
    ))
}

#[cfg(unix)]
fn list_unlocked(project_root: &Path) -> Result<SkillIndex, SkillsError> {
    use std::os::fd::AsRawFd;

    let root = super::unix::open_root(project_root)?;
    let Some((_plume, skills_dir)) = super::unix::open_store(&root, false)? else {
        return Ok(SkillIndex {
            skills: vec![],
            invalid: vec![],
        });
    };
    let mut skills = Vec::new();
    let mut invalid = Vec::new();
    for slug in super::unix::list_names(skills_dir.as_raw_fd())? {
        if !valid_slug(&slug) {
            invalid.push(SkillInvalid {
                slug,
                reason: "invalid skill directory slug".into(),
            });
            continue;
        }
        if super::unix::is_symlink_at(skills_dir.as_raw_fd(), &slug)? {
            return Err(SkillsError(format!("skill directory {slug} is a symlink")));
        }
        match super::unix::read_document(skills_dir.as_raw_fd(), &slug) {
            Ok(doc) => skills.push(SkillMetadata {
                slug,
                name: doc.name,
                description: doc.description,
            }),
            Err(error) if is_unsafe(&error) => return Err(error),
            Err(error) => invalid.push(SkillInvalid {
                slug,
                reason: error.0,
            }),
        }
    }
    skills.sort_by(|a, b| a.slug.cmp(&b.slug));
    invalid.sort_by(|a, b| a.slug.cmp(&b.slug));
    Ok(SkillIndex { skills, invalid })
}

#[cfg(not(unix))]
fn list_unlocked(_project_root: &Path) -> Result<SkillIndex, SkillsError> {
    unsupported()
}

fn is_unsafe(error: &SkillsError) -> bool {
    error.0.contains("symlink") || error.0.contains("hardlink")
}

pub fn list(project_root: &Path) -> Result<SkillIndex, SkillsError> {
    let _guard = skill_mutex().lock().unwrap_or_else(|e| e.into_inner());
    list_unlocked(project_root)
}

#[cfg(unix)]
fn load_unlocked(project_root: &Path, slug: &str) -> Result<SkillDocument, SkillsError> {
    use std::os::fd::AsRawFd;
    let root = super::unix::open_root(project_root)?;
    let (_plume, skills) = super::unix::open_store(&root, false)?
        .ok_or_else(|| SkillsError("skill library does not exist".into()))?;
    super::unix::read_document(skills.as_raw_fd(), slug)
}

#[cfg(not(unix))]
fn load_unlocked(_project_root: &Path, _slug: &str) -> Result<SkillDocument, SkillsError> {
    unsupported()
}

pub fn load(project_root: &Path, slug: &str) -> Result<SkillDocument, SkillsError> {
    let _guard = skill_mutex().lock().unwrap_or_else(|e| e.into_inner());
    load_unlocked(project_root, slug)
}

#[cfg(unix)]
fn preview_unlocked(project_root: &Path, input: &SkillInput) -> Result<SkillPreview, SkillsError> {
    use std::os::fd::AsRawFd;
    let content = canonical(input)?;
    let root = super::unix::open_root(project_root)?;
    let exists = match super::unix::open_store(&root, false)? {
        Some((_plume, skills)) => {
            match super::unix::open_optional_dir_at(skills.as_raw_fd(), &input.slug)? {
                Some(slug) => {
                    super::unix::validate_existing_final(slug.as_raw_fd())?;
                    true
                }
                None => false,
            }
        }
        None => false,
    };
    Ok(SkillPreview {
        slug: input.slug.clone(),
        content,
        exists,
    })
}

#[cfg(not(unix))]
fn preview_unlocked(
    _project_root: &Path,
    _input: &SkillInput,
) -> Result<SkillPreview, SkillsError> {
    unsupported()
}

pub fn preview(project_root: &Path, input: &SkillInput) -> Result<SkillPreview, SkillsError> {
    let _guard = skill_mutex().lock().unwrap_or_else(|e| e.into_inner());
    preview_unlocked(project_root, input)
}

#[cfg(unix)]
fn apply_unlocked<F>(
    project_root: &Path,
    input: &SkillInput,
    before_link: F,
) -> Result<SkillApplyResponse, SkillsError>
where
    F: FnOnce() -> Result<(), SkillsError>,
{
    use std::os::fd::AsRawFd;
    let content = canonical(input)?;
    let root = super::unix::open_root(project_root)?;
    let (_plume, skills) = super::unix::open_store(&root, true)?.expect("create store returns fds");
    let index = list_from_fd(skills.as_raw_fd())?;
    if index.skills.len() + index.invalid.len() >= MAX_SKILLS {
        return Ok(SkillApplyResponse {
            ok: false,
            skill: None,
            reason: Some("capacityReached".into()),
            message: Some(format!("skill library is capped at {MAX_SKILLS}")),
        });
    }
    let Some(slug_dir) = super::unix::create_slug(skills.as_raw_fd(), &input.slug)? else {
        return Ok(SkillApplyResponse {
            ok: false,
            skill: None,
            reason: Some("alreadyExists".into()),
            message: Some(format!("skill {} already exists", input.slug)),
        });
    };
    if let Err(error) = super::unix::install(slug_dir.as_raw_fd(), content.as_bytes(), before_link)
    {
        drop(slug_dir);
        let cleanup = super::unix::remove_empty_dir(skills.as_raw_fd(), &input.slug);
        return Err(with_cleanup_result(error, cleanup));
    }
    Ok(SkillApplyResponse {
        ok: true,
        skill: Some(SkillMetadata {
            slug: input.slug.clone(),
            name: input.name.clone(),
            description: input.description.clone(),
        }),
        reason: None,
        message: None,
    })
}

pub(super) fn with_cleanup_result(
    install_error: SkillsError,
    cleanup: Result<bool, SkillsError>,
) -> SkillsError {
    match cleanup {
        Ok(true) => install_error,
        Ok(false) => SkillsError(format!(
            "{}; newly-created slug directory was not empty and may remain reserved",
            install_error.0
        )),
        Err(cleanup_error) => SkillsError(format!(
            "{}; failed to clean newly-created slug directory: {}",
            install_error.0, cleanup_error.0
        )),
    }
}

#[cfg(unix)]
fn list_from_fd(skills_fd: std::os::fd::RawFd) -> Result<SkillIndex, SkillsError> {
    let mut skills = Vec::new();
    let mut invalid = Vec::new();
    for slug in super::unix::list_names(skills_fd)? {
        if !valid_slug(&slug) {
            invalid.push(SkillInvalid {
                slug,
                reason: "invalid skill directory slug".into(),
            });
        } else if super::unix::is_symlink_at(skills_fd, &slug)? {
            return Err(SkillsError(format!("skill directory {slug} is a symlink")));
        } else {
            match super::unix::read_document(skills_fd, &slug) {
                Ok(doc) => skills.push(SkillMetadata {
                    slug,
                    name: doc.name,
                    description: doc.description,
                }),
                Err(error) if is_unsafe(&error) => return Err(error),
                Err(error) => invalid.push(SkillInvalid {
                    slug,
                    reason: error.0,
                }),
            }
        }
    }
    skills.sort_by(|a, b| a.slug.cmp(&b.slug));
    invalid.sort_by(|a, b| a.slug.cmp(&b.slug));
    Ok(SkillIndex { skills, invalid })
}

pub fn apply(project_root: &Path, input: &SkillInput) -> Result<SkillApplyResponse, SkillsError> {
    let _guard = skill_mutex().lock().unwrap_or_else(|e| e.into_inner());
    #[cfg(unix)]
    {
        apply_unlocked(project_root, input, || Ok(()))
    }
    #[cfg(not(unix))]
    {
        let _ = (project_root, input);
        unsupported()
    }
}

#[cfg(all(test, unix))]
pub(super) fn apply_with_hook<F>(
    project_root: &Path,
    input: &SkillInput,
    hook: F,
) -> Result<SkillApplyResponse, SkillsError>
where
    F: FnOnce() -> Result<(), SkillsError>,
{
    let _guard = skill_mutex().lock().unwrap_or_else(|e| e.into_inner());
    apply_unlocked(project_root, input, hook)
}

#[cfg(all(test, unix))]
pub(super) fn load_with_hook<F>(
    project_root: &Path,
    slug: &str,
    hook: F,
) -> Result<SkillDocument, SkillsError>
where
    F: FnOnce(),
{
    use std::os::fd::AsRawFd;
    let _guard = skill_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let root = super::unix::open_root(project_root)?;
    let (_plume, skills) = super::unix::open_store(&root, false)?
        .ok_or_else(|| SkillsError("skill library does not exist".into()))?;
    super::unix::read_document_with_hook(skills.as_raw_fd(), slug, hook)
}
