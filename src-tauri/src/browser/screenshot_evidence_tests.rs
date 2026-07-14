use std::fs;
use std::path::{Path, PathBuf};

use super::screenshot_evidence::{
    read_screenshot_evidence, store_screenshot_evidence, CapturedBrowserScreenshot,
    BROWSER_SCREENSHOT_BYTE_CAP,
};

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "plume-browser-screenshot-{label}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn png(width: u32, height: u32, trailing: usize) -> Vec<u8> {
    if width == 0 || height == 0 {
        let mut bytes = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR".to_vec();
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend(std::iter::repeat_n(0, trailing));
        return bytes;
    }
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Grayscale);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer
            .write_image_data(&vec![0; (width as usize) * (height as usize)])
            .unwrap();
    }
    bytes.extend(std::iter::repeat_n(0, trailing));
    bytes
}

fn capture(bytes: Vec<u8>, width: u32, height: u32) -> CapturedBrowserScreenshot {
    CapturedBrowserScreenshot {
        source_url: "https://example.com/private?token=nope#frag".into(),
        title: Some("Example page".into()),
        png_bytes: bytes,
        width,
        height,
    }
}

#[test]
fn screenshot_round_trip_uses_opaque_id_and_keeps_png_out_of_summary() {
    let td = TempDir::new("round-trip");
    let summary = store_screenshot_evidence(td.path(), capture(png(800, 600, 32), 800, 600))
        .expect("store screenshot");

    assert!(summary.evidence_id.starts_with("bs_"));
    assert_eq!(summary.evidence_id.len(), 35);
    assert_eq!(summary.source_url, "https://example.com/private");
    assert_eq!((summary.width, summary.height), (800, 600));
    assert_eq!(summary.bytes as usize, png(800, 600, 32).len());
    assert_eq!(summary.sha256.len(), 64);

    let stored = read_screenshot_evidence(td.path(), &summary.evidence_id)
        .unwrap()
        .expect("stored screenshot");
    assert_eq!(stored.metadata.id, summary.evidence_id);
    assert_eq!(stored.metadata.sha256, summary.sha256);
    assert_eq!(stored.png_bytes, png(800, 600, 32));
}

#[test]
fn screenshot_store_rejects_mismatched_or_unbounded_images() {
    let td = TempDir::new("bounds");
    assert!(store_screenshot_evidence(td.path(), capture(png(800, 600, 0), 801, 600)).is_err());
    assert!(store_screenshot_evidence(td.path(), capture(png(0, 600, 0), 0, 600)).is_err());
    assert!(store_screenshot_evidence(td.path(), capture(png(4097, 600, 0), 4097, 600)).is_err());
    assert!(store_screenshot_evidence(
        td.path(),
        capture(png(1, 1, BROWSER_SCREENSHOT_BYTE_CAP), 1, 1),
    )
    .is_err());
}

#[test]
fn screenshot_read_rejects_tampered_png_or_metadata() {
    let td = TempDir::new("tamper");
    let summary =
        store_screenshot_evidence(td.path(), capture(png(320, 200, 8), 320, 200)).unwrap();
    let dir = td.path().join(".plume/browser-evidence/screenshots");
    let png_path = dir.join(format!("{}.png", summary.evidence_id));
    let mut replacement = png(320, 200, 8);
    *replacement.last_mut().unwrap() = 1;
    assert_eq!(replacement.len(), summary.bytes as usize);
    fs::write(&png_path, replacement).unwrap();
    assert!(read_screenshot_evidence(td.path(), &summary.evidence_id).is_err());

    fs::write(&png_path, png(320, 200, 8)).unwrap();
    let metadata_path = dir.join(format!("{}.json", summary.evidence_id));
    let raw = fs::read_to_string(&metadata_path).unwrap();
    fs::write(
        &metadata_path,
        raw.replace("Example page", "sk-aaaaaaaaaaaaaaaaaaaaaaaa"),
    )
    .unwrap();
    assert!(read_screenshot_evidence(td.path(), &summary.evidence_id).is_err());
}

#[cfg(unix)]
#[test]
fn screenshot_read_refuses_symlinks_and_hardlinks() {
    use std::os::unix::fs::symlink;

    let td = TempDir::new("links");
    let summary = store_screenshot_evidence(td.path(), capture(png(10, 10, 8), 10, 10)).unwrap();
    let dir = td.path().join(".plume/browser-evidence/screenshots");
    let png_path = dir.join(format!("{}.png", summary.evidence_id));
    let saved = fs::read(&png_path).unwrap();
    fs::remove_file(&png_path).unwrap();
    let outside = td.path().join("outside.png");
    fs::write(&outside, &saved).unwrap();
    symlink(&outside, &png_path).unwrap();
    assert!(read_screenshot_evidence(td.path(), &summary.evidence_id).is_err());

    fs::remove_file(&png_path).unwrap();
    fs::hard_link(&outside, &png_path).unwrap();
    assert!(read_screenshot_evidence(td.path(), &summary.evidence_id).is_err());
}
