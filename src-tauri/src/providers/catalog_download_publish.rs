//! Descriptor-bound preparation, verification, and publication of catalog files.
//!
//! This child module keeps the high-risk output lifetime below the source-file
//! cap without widening the downloader's authority. Every prepared output fd
//! stays open from exclusive creation through the final no-follow re-open and
//! inode comparison immediately before the directory rename.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::sync::atomic::{AtomicBool, Ordering};

use sha2::{Digest, Sha256};

use super::*;

struct PreparedOutput {
    file: File,
    size: u64,
    sha256: String,
}

impl CatalogRoot {
    pub(crate) fn finalize(
        &self,
        staging: &mut StagingDir,
        manifest: &DownloadManifest,
        receipt: &InstallReceipt,
        cancel: &AtomicBool,
    ) -> Result<(), DownloadError> {
        check_cancelled(cancel)?;
        if self.install_exists()? {
            return Err(DownloadError::InstallExists);
        }
        self.remove_prepared_recovery()?;
        let prepared = open_or_create_directory(&self.directory, PREPARED_NAME)?;
        let result = finalize_prepared(self, staging, manifest, receipt, &prepared, cancel);
        if matches!(result, Err(DownloadError::Cancelled)) {
            // Cancellation never spends the verified source parts. Prepared
            // outputs are disposable copies, so recover them now; if an
            // outside mutation makes safe deletion impossible, next open will
            // refuse it rather than deleting through the unexpected link.
            let _ = self.remove_prepared_recovery();
        }
        result
    }
}

fn finalize_prepared(
    root: &CatalogRoot,
    staging: &mut StagingDir,
    manifest: &DownloadManifest,
    receipt: &InstallReceipt,
    prepared: &File,
    cancel: &AtomicBool,
) -> Result<(), DownloadError> {
    let mut outputs = BTreeMap::new();
    for expected in &manifest.files {
        check_cancelled(cancel)?;
        let source = staging.verified.get(&expected.path).ok_or_else(|| {
            DownloadError::UnexpectedStagingPath {
                path: expected.path.clone(),
            }
        })?;
        validate_regular_exact(source, expected, "verified staging part")?;
        outputs.insert(
            expected.path.clone(),
            copy_verified_part(source, prepared, expected, cancel)?,
        );
    }
    outputs.insert(
        RECEIPT_NAME.into(),
        write_receipt(prepared, receipt, cancel)?,
    );

    // Every created file is synced before the directory metadata is synced.
    // The check immediately before each sync/rename is the cancellation fence:
    // once rename begins, publication is the irreversible commit point.
    check_cancelled(cancel)?;
    sync_directory(prepared)?;
    check_cancelled(cancel)?;
    publication_hook("before-publish-validation", None);
    verify_prepared_outputs(prepared, &outputs, cancel)?;
    check_cancelled(cancel)?;
    require_same_directory(&root.directory, PREPARED_NAME, prepared)?;
    check_cancelled(cancel)?;
    rename_directory_no_replace(&root.directory, PREPARED_NAME, QWEN_REVISION)?;
    sync_directory(&root.directory)?;

    // Publication is already durable. Leaving resumable copies after a rare
    // cleanup failure is safe and recoverable, but reporting failure here
    // would falsely claim that a completed install did not exist.
    let _ = cleanup_staging_after_publish(root, staging, manifest);
    Ok(())
}

fn copy_verified_part(
    source: &File,
    target_directory: &File,
    expected: &ManifestFile,
    cancel: &AtomicBool,
) -> Result<PreparedOutput, DownloadError> {
    validate_regular_exact(source, expected, "verified staging part")?;
    let mut input = source
        .try_clone()
        .map_err(|error| io_error(&expected.path, error))?;
    input
        .seek(SeekFrom::Start(0))
        .map_err(|error| io_error(&expected.path, error))?;
    publication_hook("before-output-create", Some(&expected.path));
    let mut output = create_regular(target_directory, &expected.path)?;
    publication_hook("before-output-validation", Some(&expected.path));
    // This is before the first output write. An attacker can create a hardlink
    // only as a second name; nlink therefore catches the deterministic race
    // without ever writing a byte to that external name.
    validate_regular_unique(&output, &expected.path)?;
    check_cancelled(cancel)?;

    let mut copied = 0u64;
    let mut buffer = [0u8; COPY_BUFFER_BYTES];
    loop {
        check_cancelled(cancel)?;
        let count = input
            .read(&mut buffer)
            .map_err(|error| io_error(&expected.path, error))?;
        if count == 0 {
            break;
        }
        copied = copied
            .checked_add(count as u64)
            .ok_or(DownloadError::ByteCeiling)?;
        if copied > expected.size {
            return Err(DownloadError::SizeMismatch {
                path: expected.path.clone(),
                expected: expected.size,
                actual: copied,
            });
        }
        check_cancelled(cancel)?;
        output
            .write_all(&buffer[..count])
            .map_err(|error| io_error(&expected.path, error))?;
    }
    check_cancelled(cancel)?;
    output
        .sync_all()
        .map_err(|error| io_error(&expected.path, error))?;
    validate_regular_exact(&output, expected, "prepared catalog file")?;
    if hash_with_cancellation(&mut output, &expected.path, cancel)? != expected.sha256 {
        return Err(DownloadError::HashMismatch {
            path: expected.path.clone(),
        });
    }
    Ok(PreparedOutput {
        file: output,
        size: expected.size,
        sha256: expected.sha256.clone(),
    })
}

fn write_receipt(
    directory: &File,
    receipt: &InstallReceipt,
    cancel: &AtomicBool,
) -> Result<PreparedOutput, DownloadError> {
    let bytes =
        serde_json::to_vec(receipt).map_err(|error| DownloadError::Manifest(error.to_string()))?;
    let mut receipt_file = create_regular(directory, RECEIPT_NAME)?;
    publication_hook("before-receipt-validation", None);
    // Receipt data is just as authoritative as a model file at removal time,
    // so it gets the same exclusive-create and single-link check before write.
    validate_regular_unique(&receipt_file, RECEIPT_NAME)?;
    check_cancelled(cancel)?;
    receipt_file
        .write_all(&bytes)
        .map_err(|error| io_error(RECEIPT_NAME, error))?;
    check_cancelled(cancel)?;
    receipt_file
        .sync_all()
        .map_err(|error| io_error(RECEIPT_NAME, error))?;
    Ok(PreparedOutput {
        file: receipt_file,
        size: bytes.len() as u64,
        sha256: format!("{:x}", Sha256::digest(&bytes)),
    })
}

fn verify_prepared_outputs(
    directory: &File,
    expected: &BTreeMap<String, PreparedOutput>,
    cancel: &AtomicBool,
) -> Result<(), DownloadError> {
    let names = directory_entries(directory)?;
    if names.len() != expected.len()
        || names
            .iter()
            .any(|name| !expected.contains_key(name.to_string_lossy().as_ref()))
    {
        return Err(DownloadError::UnexpectedStagingPath {
            path: "prepared catalog output".into(),
        });
    }
    for (name, output) in expected {
        check_cancelled(cancel)?;
        let mut reopened = open_regular(directory, std::ffi::OsStr::new(name))?;
        require_same_regular(&reopened, &output.file, name)?;
        let length = regular_len_unique(&reopened, name)?;
        if length != output.size {
            return Err(DownloadError::SizeMismatch {
                path: name.clone(),
                expected: output.size,
                actual: length,
            });
        }
        if hash_with_cancellation(&mut reopened, name, cancel)? != output.sha256 {
            return Err(DownloadError::HashMismatch { path: name.clone() });
        }
    }
    Ok(())
}

fn require_same_regular(current: &File, expected: &File, name: &str) -> Result<(), DownloadError> {
    let current_metadata = current.metadata().map_err(|error| io_error(name, error))?;
    let expected_metadata = expected.metadata().map_err(|error| io_error(name, error))?;
    if !current_metadata.is_file()
        || !expected_metadata.is_file()
        || current_metadata.dev() != expected_metadata.dev()
        || current_metadata.ino() != expected_metadata.ino()
    {
        return Err(DownloadError::PathSwap { path: name.into() });
    }
    validate_regular_unique(current, name)?;
    validate_regular_unique(expected, name)
}

fn hash_with_cancellation(
    file: &mut File,
    label: &str,
    cancel: &AtomicBool,
) -> Result<String, DownloadError> {
    file.seek(SeekFrom::Start(0))
        .map_err(|error| io_error(label, error))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; COPY_BUFFER_BYTES];
    loop {
        check_cancelled(cancel)?;
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

fn cleanup_staging_after_publish(
    root: &CatalogRoot,
    staging: &StagingDir,
    manifest: &DownloadManifest,
) -> Result<(), DownloadError> {
    for file in &manifest.files {
        unlink_file(&staging.directory, &part_name(file))?;
    }
    sync_directory(&staging.directory)?;
    remove_directory_entry(&root.directory, STAGING_NAME, &staging.directory)?;
    sync_directory(&root.directory)
}

fn check_cancelled(cancel: &AtomicBool) -> Result<(), DownloadError> {
    if cancel.load(Ordering::Acquire) {
        Err(DownloadError::Cancelled)
    } else {
        Ok(())
    }
}

#[cfg(test)]
use std::cell::Cell;
#[cfg(test)]
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

#[cfg(test)]
type PublicationHook = Arc<dyn Fn(&str) + Send + Sync>;

#[cfg(test)]
thread_local! {
    static OWNS_PUBLICATION_HOOK: Cell<bool> = const { Cell::new(false) };
}

#[cfg(test)]
fn publication_hook_slot() -> &'static Mutex<Option<PublicationHook>> {
    static SLOT: OnceLock<Mutex<Option<PublicationHook>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

#[cfg(test)]
fn publication_hook_gate() -> &'static Mutex<()> {
    static GATE: OnceLock<Mutex<()>> = OnceLock::new();
    GATE.get_or_init(|| Mutex::new(()))
}

#[cfg(test)]
pub(crate) struct PublicationHookGuard {
    _gate: MutexGuard<'static, ()>,
}

#[cfg(test)]
impl Drop for PublicationHookGuard {
    fn drop(&mut self) {
        OWNS_PUBLICATION_HOOK.with(|owns| owns.set(false));
        *publication_hook_slot()
            .lock()
            .expect("publication hook mutex") = None;
    }
}

#[cfg(test)]
pub(crate) fn with_publication_hook_for_test(
    hook: impl Fn(&str) + Send + Sync + 'static,
) -> PublicationHookGuard {
    let gate = publication_hook_gate()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    *publication_hook_slot()
        .lock()
        .expect("publication hook mutex") = Some(Arc::new(hook));
    OWNS_PUBLICATION_HOOK.with(|owns| owns.set(true));
    PublicationHookGuard { _gate: gate }
}

fn publication_hook(point: &str, name: Option<&str>) {
    // The test guard owns this lock while a deterministic mutation is armed.
    // Other parallel test workers pause at the same hook point instead of
    // accidentally receiving that mutation against their own fixture.
    #[cfg(test)]
    if !OWNS_PUBLICATION_HOOK.with(|owns| owns.get()) {
        drop(
            publication_hook_gate()
                .lock()
                .unwrap_or_else(|error| error.into_inner()),
        );
    }
    #[cfg(test)]
    let hook = publication_hook_slot()
        .lock()
        .expect("publication hook mutex")
        .clone();
    #[cfg(test)]
    if let Some(hook) = hook {
        let point = name
            .map(|name| format!("{point}:{name}"))
            .unwrap_or_else(|| point.into());
        hook(&point);
    }
    #[cfg(not(test))]
    let _ = (point, name);
}
