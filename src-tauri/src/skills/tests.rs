use super::*;
use std::fs;
use std::sync::{Arc, Barrier};
use std::thread;

struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "plume-skills-test-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir_all(&path)?;
        Ok(Self(fs::canonicalize(path)?))
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn input(slug: &str) -> SkillInput {
    SkillInput {
        slug: slug.into(),
        name: "Explain tests".into(),
        description: "Explains a focused test failure.".into(),
        body: "# Steps\n\nRead the failure first.\n".into(),
    }
}

#[test]
fn preview_is_canonical_and_does_not_write() {
    let dir = TempDir::new().unwrap();
    let preview = preview(dir.path(), &input("explain-tests")).unwrap();
    assert!(!preview.exists);
    assert_eq!(preview.content, "---\nname: \"Explain tests\"\ndescription: \"Explains a focused test failure.\"\n---\n\n# Steps\n\nRead the failure first.\n");
    assert!(!dir.path().join(".plume").exists());
}

#[test]
fn apply_then_list_metadata_and_load_exact_content() {
    let dir = TempDir::new().unwrap();
    let expected = preview(dir.path(), &input("explain-tests"))
        .unwrap()
        .content;
    assert!(apply(dir.path(), &input("explain-tests")).unwrap().ok);
    let index = list(dir.path()).unwrap();
    assert_eq!(index.skills.len(), 1);
    assert!(index.invalid.is_empty());
    assert_eq!(index.skills[0].slug, "explain-tests");
    assert_eq!(index.skills[0].name, "Explain tests");
    let loaded = load(dir.path(), "explain-tests").unwrap();
    assert_eq!(loaded.content, expected);
    assert_eq!(loaded.body, input("x").body);
    let names: Vec<_> = fs::read_dir(dir.path().join(".plume/skills/explain-tests"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["SKILL.md"]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let installed = dir.path().join(".plume/skills/explain-tests/SKILL.md");
        assert_eq!(fs::metadata(installed).unwrap().nlink(), 1);
    }
}

#[test]
fn second_apply_preserves_existing_bytes() {
    let dir = TempDir::new().unwrap();
    apply(dir.path(), &input("stable")).unwrap();
    let path = dir.path().join(".plume/skills/stable/SKILL.md");
    let before = fs::read(&path).unwrap();
    let mut changed = input("stable");
    changed.body = "replacement".into();
    let result = apply(dir.path(), &changed).unwrap();
    assert!(!result.ok);
    assert_eq!(result.reason.as_deref(), Some("alreadyExists"));
    assert_eq!(fs::read(path).unwrap(), before);
}

#[test]
fn invalid_frontmatter_is_reported_not_hidden() {
    let dir = TempDir::new().unwrap();
    let skill = dir.path().join(".plume/skills/broken");
    fs::create_dir_all(&skill).unwrap();
    fs::write(
        skill.join("SKILL.md"),
        "---\nname: \"x\"\nunknown: \"y\"\ndescription: \"z\"\n---\n\nbody",
    )
    .unwrap();
    let index = list(dir.path()).unwrap();
    assert!(index.skills.is_empty());
    assert_eq!(index.invalid.len(), 1);
    assert_eq!(index.invalid[0].slug, "broken");
}

#[test]
fn duplicate_malformed_and_invalid_utf8_documents_are_reported() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join(".plume/skills");
    for (slug, bytes) in [
        (
            "duplicate",
            b"---\nname: \"x\"\nname: \"y\"\ndescription: \"z\"\n---\n\nbody".as_slice(),
        ),
        (
            "malformed",
            b"---\nname=x\ndescription: \"z\"\n---\n\nbody".as_slice(),
        ),
        ("bad-utf8", &[0xff, 0xfe][..]),
    ] {
        let path = root.join(slug);
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("SKILL.md"), bytes).unwrap();
    }
    let index = list(dir.path()).unwrap();
    assert_eq!(index.invalid.len(), 3);
}

#[test]
fn metadata_limits_count_unicode_characters_not_bytes() {
    let dir = TempDir::new().unwrap();
    let mut valid = input("unicode");
    valid.name = "é".repeat(80);
    valid.description = "🪶".repeat(240);
    assert!(preview(dir.path(), &valid).is_ok());
    valid.name.push('x');
    assert!(preview(dir.path(), &valid).is_err());
}

#[test]
fn rejects_bad_slugs_and_unicode_byte_overflow() {
    let dir = TempDir::new().unwrap();
    for slug in ["../escape", "UPPER", "two--parts", "-lead", "trail-"] {
        assert!(preview(dir.path(), &input(slug)).is_err(), "{slug}");
    }
    let mut too_long = input("valid");
    too_long.body = "🪶".repeat((MAX_BODY_BYTES / 4) + 1);
    assert!(preview(dir.path(), &too_long).is_err());
}

#[cfg(unix)]
#[test]
fn rejects_symlinks_at_each_storage_ancestor_and_hardlinked_file() {
    use std::os::unix::fs::symlink;
    for rel in [".plume", ".plume/skills", ".plume/skills/demo"] {
        let dir = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let link = dir.path().join(rel);
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        symlink(outside.path(), link).unwrap();
        assert!(list(dir.path()).is_err(), "accepted symlink at {rel}");
    }
    let dir = TempDir::new().unwrap();
    let skill = dir.path().join(".plume/skills/demo");
    fs::create_dir_all(&skill).unwrap();
    let outside = dir.path().join("outside.md");
    fs::write(
        &outside,
        "---\nname: \"n\"\ndescription: \"d\"\n---\n\nbody",
    )
    .unwrap();
    fs::hard_link(&outside, skill.join("SKILL.md")).unwrap();
    assert!(load(dir.path(), "demo").is_err());
    assert!(preview(dir.path(), &input("demo")).is_err());
}

#[test]
fn cap_is_enforced_inside_mutex_under_concurrency() {
    let dir = Arc::new(TempDir::new().unwrap());
    for n in 0..(MAX_SKILLS - 1) {
        apply(dir.path(), &input(&format!("s{n}"))).unwrap();
    }
    let barrier = Arc::new(Barrier::new(3));
    let mut joins = Vec::new();
    for slug in ["last-a", "last-b"] {
        let dir = Arc::clone(&dir);
        let barrier = Arc::clone(&barrier);
        joins.push(thread::spawn(move || {
            barrier.wait();
            apply(dir.path(), &input(slug)).unwrap().ok
        }));
    }
    barrier.wait();
    let successes = joins
        .into_iter()
        .map(|j| j.join().unwrap())
        .filter(|ok| *ok)
        .count();
    assert_eq!(successes, 1);
    assert_eq!(list(dir.path()).unwrap().skills.len(), MAX_SKILLS);
}

#[test]
fn no_clobber_install_preserves_a_final_file_that_appears_before_install() {
    let dir = TempDir::new().unwrap();
    let final_path = dir.path().join(".plume/skills/raced/SKILL.md");
    let result = super::store::apply_with_hook(dir.path(), &input("raced"), || {
        fs::write(&final_path, b"external bytes").unwrap();
        Ok(())
    });
    assert!(result.is_err());
    assert_eq!(fs::read(&final_path).unwrap(), b"external bytes");
    let names: Vec<_> = fs::read_dir(final_path.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["SKILL.md"]);
}

#[test]
fn pre_link_failure_releases_empty_slug_for_retry() {
    let dir = TempDir::new().unwrap();
    let failed = super::store::apply_with_hook(dir.path(), &input("retryable"), || {
        Err(SkillsError("injected pre-link failure".into()))
    });
    assert!(failed.is_err());
    assert!(!dir.path().join(".plume/skills/retryable").exists());
    assert!(apply(dir.path(), &input("retryable")).unwrap().ok);
}

#[test]
fn failed_slug_cleanup_is_reported_without_hiding_install_error() {
    let install = SkillsError("install failed".into());
    let reserved = super::store::with_cleanup_result(install, Ok(false));
    assert!(reserved.0.contains("install failed"));
    assert!(reserved.0.contains("may remain reserved"));
    let cleanup_error = super::store::with_cleanup_result(
        SkillsError("install failed".into()),
        Err(SkillsError("fsync cleanup failed".into())),
    );
    assert!(cleanup_error.0.contains("install failed"));
    assert!(cleanup_error.0.contains("fsync cleanup failed"));
}

#[cfg(unix)]
#[test]
fn root_walk_refuses_intermediate_ancestor_swapped_to_outside_symlink() {
    use std::os::unix::fs::symlink;
    let base = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    let ancestor = base.path().join("ancestor");
    let project = ancestor.join("project");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(outside.path().join("project")).unwrap();
    let sentinel = outside.path().join("sentinel");
    fs::write(&sentinel, b"outside unchanged").unwrap();
    let mut swapped = false;
    let result = super::unix::open_root_with_hook(&project, |_, component| {
        if component == std::ffi::OsStr::new("ancestor") && !swapped {
            fs::rename(&ancestor, base.path().join("held-ancestor")).unwrap();
            symlink(outside.path(), &ancestor).unwrap();
            swapped = true;
        }
    });
    assert!(result.is_err());
    assert_eq!(fs::read(sentinel).unwrap(), b"outside unchanged");
}

#[cfg(unix)]
#[test]
fn apply_stays_in_held_directory_chain_when_skills_ancestor_is_swapped() {
    use std::os::unix::fs::symlink;
    let dir = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    let sentinel = outside.path().join("sentinel");
    fs::write(&sentinel, b"outside unchanged").unwrap();
    let plume = dir.path().join(".plume");
    let skills = plume.join("skills");
    let result = super::store::apply_with_hook(dir.path(), &input("race-safe"), || {
        fs::rename(&skills, plume.join("held-skills")).unwrap();
        symlink(outside.path(), &skills).unwrap();
        Ok(())
    })
    .unwrap();
    assert!(result.ok);
    assert_eq!(fs::read(&sentinel).unwrap(), b"outside unchanged");
    assert!(plume.join("held-skills/race-safe/SKILL.md").is_file());
    assert!(!outside.path().join("race-safe/SKILL.md").exists());
}

#[cfg(unix)]
#[test]
fn load_reads_held_in_project_file_when_slug_path_is_swapped_to_symlink() {
    use std::os::unix::fs::symlink;
    let dir = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    apply(dir.path(), &input("read-safe")).unwrap();
    let skills = dir.path().join(".plume/skills");
    let original = preview(dir.path(), &input("read-safe")).unwrap().content;
    let external = outside.path().join("SKILL.md");
    fs::write(&external, b"external sentinel").unwrap();
    let loaded = super::store::load_with_hook(dir.path(), "read-safe", || {
        fs::rename(skills.join("read-safe"), skills.join("held-read")).unwrap();
        symlink(outside.path(), skills.join("read-safe")).unwrap();
    })
    .unwrap();
    assert_eq!(loaded.content, original);
    assert_eq!(fs::read(external).unwrap(), b"external sentinel");
}

#[test]
fn list_waits_for_writer_instead_of_observing_a_partial_skill() {
    let dir = Arc::new(TempDir::new().unwrap());
    let guard = super::store::skill_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let skill_dir = dir.path().join(".plume/skills/partial");
    fs::create_dir_all(&skill_dir).unwrap();
    let (sent, received) = std::sync::mpsc::channel();
    let reader_dir = Arc::clone(&dir);
    let reader = thread::spawn(move || {
        let result = list(reader_dir.path());
        sent.send(result).unwrap();
    });
    assert!(received
        .recv_timeout(std::time::Duration::from_millis(50))
        .is_err());
    let content = super::parser::canonical(&input("partial")).unwrap();
    fs::write(skill_dir.join("SKILL.md"), content).unwrap();
    drop(guard);
    let index = received
        .recv_timeout(std::time::Duration::from_secs(1))
        .unwrap()
        .unwrap();
    reader.join().unwrap();
    assert_eq!(index.skills.len(), 1);
    assert!(index.invalid.is_empty());
}
