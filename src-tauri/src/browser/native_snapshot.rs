//! macOS WKWebView visible-viewport snapshot bridge.

use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::mpsc::SyncSender;

use block2::RcBlock;
use objc2::runtime::AnyObject;
use objc2_app_kit::{
    NSBitmapImageFileType, NSBitmapImageRep, NSBitmapImageRepPropertyKey, NSImage,
};
use objc2_foundation::{NSDictionary, NSError};
use objc2_web_kit::WKWebView;
use tauri::WebviewWindow;

use super::screenshot_evidence::{BROWSER_SCREENSHOT_BYTE_CAP, BROWSER_SCREENSHOT_DIMENSION_CAP};

#[derive(Debug)]
pub struct NativeBrowserSnapshot {
    pub png_bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub title: Option<String>,
}

pub fn request_visible_snapshot(
    window: &WebviewWindow,
    sender: SyncSender<Result<NativeBrowserSnapshot, &'static str>>,
) -> Result<(), tauri::Error> {
    window.with_webview(move |webview| unsafe {
        let view: &WKWebView = &*webview.inner().cast();
        let title = view.title().map(|value| value.to_string());
        let completion: RcBlock<dyn Fn(*mut NSImage, *mut NSError)> =
            RcBlock::new(move |image: *mut NSImage, error: *mut NSError| {
                let result = if !error.is_null() || image.is_null() {
                    Err("browser.snapshotFailed")
                } else {
                    encode_png(&*image, title.clone())
                };
                let _ = sender.send(result);
            });
        view.takeSnapshotWithConfiguration_completionHandler(None, &completion);
    })
}

unsafe fn encode_png(
    image: &NSImage,
    title: Option<String>,
) -> Result<NativeBrowserSnapshot, &'static str> {
    let tiff = image
        .TIFFRepresentation()
        .ok_or("browser.snapshotEncodingFailed")?;
    let representation =
        NSBitmapImageRep::imageRepWithData(&tiff).ok_or("browser.snapshotEncodingFailed")?;
    let width: u32 = representation
        .pixelsWide()
        .try_into()
        .map_err(|_| "browser.snapshotDimensionsInvalid")?;
    let height: u32 = representation
        .pixelsHigh()
        .try_into()
        .map_err(|_| "browser.snapshotDimensionsInvalid")?;
    if width == 0
        || height == 0
        || width > BROWSER_SCREENSHOT_DIMENSION_CAP
        || height > BROWSER_SCREENSHOT_DIMENSION_CAP
    {
        return Err("browser.snapshotDimensionsInvalid");
    }
    let properties: objc2::rc::Retained<NSDictionary<NSBitmapImageRepPropertyKey, AnyObject>> =
        NSDictionary::new();
    let png = representation
        .representationUsingType_properties(NSBitmapImageFileType::PNG, &properties)
        .ok_or("browser.snapshotEncodingFailed")?;
    let length = png.length();
    if length == 0 || length > BROWSER_SCREENSHOT_BYTE_CAP {
        return Err("browser.snapshotTooLarge");
    }
    let mut png_bytes = vec![0_u8; length];
    if length > 0 {
        let destination = NonNull::new(png_bytes.as_mut_ptr().cast::<c_void>())
            .ok_or("browser.snapshotEncodingFailed")?;
        png.getBytes_length(destination, length);
    }
    Ok(NativeBrowserSnapshot {
        png_bytes,
        width,
        height,
        title,
    })
}
