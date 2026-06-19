//! Image clipboard support for the desktop backend.
//!
//! arboard hands us raw RGBA. We turn that into a [`ClipboardItem`] whose
//! `display_content` is a PNG **data-URL** (the UI renders it as `<img>`) and
//! whose `thumbnail` is an 18×18 RGBA blob the tray uses as a per-item icon.
//! `raw_content` is the bare PNG base64, so copying an image back to the system
//! clipboard is a decode + `set_image`.
//!
//! Images are never classified as sensitive — there is no text to scan — and
//! their `detected_type` is irrelevant (the UI branches on `content_type`).

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use image::{ImageBuffer, ImageFormat, Rgba};
use lapacho_core::types::{ClipboardItem, DetectedType, Sensitivity};

/// Side of the square tray thumbnail, in pixels (matches the tray icon size).
const THUMB: u32 = 18;

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Encodes raw RGBA to a base64 PNG. `None` on invalid dimensions / encode error.
fn encode_png_b64(width: u32, height: u32, rgba: &[u8]) -> Option<String> {
    let buf: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::from_raw(width, height, rgba.to_vec())?;
    let mut png = Vec::new();
    buf.write_to(&mut std::io::Cursor::new(&mut png), ImageFormat::Png)
        .ok()?;
    Some(B64.encode(png))
}

/// Builds an 18×18 RGBA thumbnail (raw bytes, base64) for the tray icon.
fn thumbnail_b64(width: u32, height: u32, rgba: &[u8]) -> Option<String> {
    let buf: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::from_raw(width, height, rgba.to_vec())?;
    let small = image::imageops::resize(&buf, THUMB, THUMB, image::imageops::FilterType::Lanczos3);
    Some(B64.encode(small.into_raw()))
}

/// Turns raw RGBA clipboard data into a complete image [`ClipboardItem`].
/// Returns `None` if the bytes don't form a valid image of `width`×`height`.
pub fn process_image(width: usize, height: usize, rgba: &[u8]) -> Option<ClipboardItem> {
    let (w, h) = (width as u32, height as u32);
    let png_b64 = encode_png_b64(w, h, rgba)?;
    let thumbnail = thumbnail_b64(w, h, rgba);
    Some(ClipboardItem {
        id: uuid::Uuid::new_v4().to_string(),
        display_content: format!("data:image/png;base64,{png_b64}"),
        raw_content: png_b64,
        content_type: "image".to_string(),
        sensitivity: Sensitivity::None,
        detected_type: DetectedType::Text,
        timestamp: now_secs(),
        thumbnail,
    })
}

/// Decodes the stored base64 PNG (`raw_content`) back into owned RGBA so it can
/// be written to the clipboard with `set_image`.
pub fn image_data_from_b64(png_b64: &str) -> Option<arboard::ImageData<'static>> {
    let bytes = B64.decode(png_b64).ok()?;
    let img = image::load_from_memory(&bytes).ok()?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    Some(arboard::ImageData {
        width: w as usize,
        height: h as usize,
        bytes: std::borrow::Cow::Owned(rgba.into_raw()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 2×2 opaque-red RGBA buffer round-trips through PNG encode → decode and
    /// produces an image item with a data-URL display and a tray thumbnail.
    #[test]
    fn process_and_decode_roundtrip() {
        let rgba: Vec<u8> = [[255u8, 0, 0, 255]; 4].concat();
        let item = process_image(2, 2, &rgba).expect("encode");
        assert_eq!(item.content_type, "image");
        assert!(item.display_content.starts_with("data:image/png;base64,"));
        assert!(item.thumbnail.is_some());

        // raw_content (bare PNG base64) decodes back to a valid image.
        let data = image_data_from_b64(&item.raw_content).expect("decode");
        assert_eq!((data.width, data.height), (2, 2));
    }

    #[test]
    fn rejects_bad_dimensions() {
        // 3 bytes can't be a 2×2 RGBA image (needs 16).
        assert!(process_image(2, 2, &[0, 0, 0]).is_none());
    }
}
