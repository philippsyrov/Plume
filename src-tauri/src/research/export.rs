//! Explicit Markdown export boundary for staged research artifacts.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;
use tauri::AppHandle;

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExportChoice {
    Cancelled,
    Save {
        path: PathBuf,
        overwrite_confirmed: bool,
    },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(
    tag = "status",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum ExportOutcome {
    Cancelled,
    Saved { file_name: String },
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ExportError {
    #[error("the selected export file already exists without overwrite consent")]
    Exists,
    #[error("the selected export path was refused: {0}")]
    Refused(String),
    #[error("could not write the research note: {0}")]
    Write(String),
    #[error("the native Save dialog failed: {0}")]
    Dialog(String),
}

pub(crate) trait ExportFilePort {
    fn write(&self, path: &Path, bytes: &[u8], overwrite: bool) -> Result<(), ExportError>;
}

pub(crate) struct AtomicExportFilePort;

impl ExportFilePort for AtomicExportFilePort {
    fn write(&self, path: &Path, bytes: &[u8], overwrite: bool) -> Result<(), ExportError> {
        write_markdown_atomic(path, bytes, overwrite)
    }
}

pub(crate) fn default_markdown_name() -> &'static str {
    "research-note.md"
}

pub(crate) fn choose_native_markdown_path(app: &AppHandle) -> Result<ExportChoice, ExportError> {
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    app.run_on_main_thread(move || {
        let result = show_native_save_panel();
        let _ = sender.send(result);
    })
    .map_err(|error| ExportError::Dialog(format!("schedule Save dialog: {error}")))?;
    receiver
        .recv_timeout(std::time::Duration::from_secs(15 * 60))
        .map_err(|error| ExportError::Dialog(format!("wait for Save dialog: {error}")))?
}

#[cfg(target_os = "macos")]
fn show_native_save_panel() -> Result<ExportChoice, ExportError> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSModalResponseCancel, NSModalResponseOK, NSSavePanel};
    use objc2_foundation::NSString;

    let marker = MainThreadMarker::new()
        .ok_or_else(|| ExportError::Dialog("Save dialog was not on the main thread".into()))?;
    let panel = NSSavePanel::savePanel(marker);
    panel.setNameFieldStringValue(&NSString::from_str(default_markdown_name()));
    panel.setTitle(Some(&NSString::from_str("Export research note")));
    panel.setMessage(Some(&NSString::from_str(
        "Choose where to save this Markdown note.",
    )));
    panel.setCanCreateDirectories(true);
    panel.setExtensionHidden(false);
    match panel.runModal() {
        response if response == NSModalResponseCancel => Ok(ExportChoice::Cancelled),
        response if response == NSModalResponseOK => {
            let path = panel
                .URL()
                .and_then(|url| url.path())
                .map(|path| PathBuf::from(path.to_string()))
                .ok_or_else(|| ExportError::Dialog("Save dialog returned no file path".into()))?;
            Ok(ExportChoice::Save {
                path,
                overwrite_confirmed: true,
            })
        }
        response => Err(ExportError::Dialog(format!(
            "Save dialog returned response {response}"
        ))),
    }
}

#[cfg(not(target_os = "macos"))]
fn show_native_save_panel() -> Result<ExportChoice, ExportError> {
    Err(ExportError::Dialog(
        "native Markdown export is available on macOS".into(),
    ))
}

pub(crate) fn export_choice(
    choice: ExportChoice,
    markdown: &[u8],
    files: &impl ExportFilePort,
) -> Result<ExportOutcome, ExportError> {
    let ExportChoice::Save {
        path,
        overwrite_confirmed,
    } = choice
    else {
        return Ok(ExportOutcome::Cancelled);
    };
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| ExportError::Refused("selected path has no file name".into()))?;
    files.write(&path, markdown, overwrite_confirmed)?;
    Ok(ExportOutcome::Saved { file_name })
}

pub(crate) fn write_markdown_atomic(
    target: &Path,
    bytes: &[u8],
    overwrite: bool,
) -> Result<(), ExportError> {
    write_markdown_atomic_with(target, bytes, overwrite, |from, to| fs::rename(from, to))
}

pub(crate) fn write_markdown_atomic_with(
    target: &Path,
    bytes: &[u8],
    overwrite: bool,
    replace: impl FnOnce(&Path, &Path) -> io::Result<()>,
) -> Result<(), ExportError> {
    if !target.is_absolute() {
        return Err(ExportError::Refused(
            "native Save dialog did not return an absolute path".into(),
        ));
    }
    let parent = target
        .parent()
        .ok_or_else(|| ExportError::Refused("selected path has no parent".into()))?;
    let parent_meta = fs::metadata(parent)
        .map_err(|error| ExportError::Write(format!("open destination folder: {error}")))?;
    if !parent_meta.is_dir() {
        return Err(ExportError::Refused(
            "selected destination folder is not a directory".into(),
        ));
    }
    inspect_existing_target(target, overwrite)?;

    let temp_path = mint_temp_path(parent, target)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut temp = options
        .open(&temp_path)
        .map_err(|error| ExportError::Write(format!("create temporary export: {error}")))?;
    let mut cleanup = TempCleanup::new(temp_path.clone());
    temp.write_all(bytes)
        .and_then(|()| temp.sync_all())
        .map_err(|error| ExportError::Write(format!("write temporary export: {error}")))?;
    drop(temp);

    if overwrite {
        replace(&temp_path, target)
            .map_err(|error| ExportError::Write(format!("replace destination: {error}")))?;
    } else {
        fs::hard_link(&temp_path, target).map_err(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                ExportError::Exists
            } else {
                ExportError::Write(format!("publish destination: {error}"))
            }
        })?;
        fs::remove_file(&temp_path)
            .map_err(|error| ExportError::Write(format!("finish destination: {error}")))?;
    }
    cleanup.disarm();
    Ok(())
}

fn inspect_existing_target(target: &Path, overwrite: bool) -> Result<(), ExportError> {
    let metadata = match fs::symlink_metadata(target) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(ExportError::Write(format!("inspect destination: {error}"))),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(ExportError::Refused(
            "existing destination is not a regular non-symlink file".into(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() > 1 {
            return Err(ExportError::Refused(
                "existing destination has multiple hardlink aliases".into(),
            ));
        }
    }
    if !overwrite {
        return Err(ExportError::Exists);
    }
    Ok(())
}

fn mint_temp_path(parent: &Path, target: &Path) -> Result<PathBuf, ExportError> {
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| ExportError::Refused("selected file name is not valid UTF-8".into()))?;
    let nonce = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
    Ok(parent.join(format!(
        ".{name}.plume-export-{}-{nonce}.tmp",
        std::process::id()
    )))
}

struct TempCleanup {
    path: Option<PathBuf>,
}

impl TempCleanup {
    fn new(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    fn disarm(&mut self) {
        self.path = None;
    }
}

impl Drop for TempCleanup {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            let _ = File::open(&path).and_then(|file| file.sync_all());
            let _ = fs::remove_file(path);
        }
    }
}
