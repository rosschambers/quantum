//! MIME classification and thumbnail generation for clipboard capture.
//!
//! Two pure helpers used by the clipboard watcher: [`classify`] maps a set of
//! offered MIME types to a [`ClipKind`], and [`thumbnail`] decodes image bytes
//! and produces a small `data:` URI preview.

use base64::Engine as _;

/// The broad category a clipboard selection falls into, derived from the MIME
/// types the source offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipKind {
    Image,
    File,
    Text,
    Binary,
}

/// Classify a clipboard selection from the list of MIME types it offers.
///
/// Precedence: an `image/*` type (preferring an explicit `image/png`) makes it
/// an [`ClipKind::Image`]; a `text/uri-list` makes it a [`ClipKind::File`]; any
/// other `text/*` makes it [`ClipKind::Text`]; anything else is
/// [`ClipKind::Binary`].
pub fn classify(types: &[String]) -> ClipKind {
    if types.iter().any(|mime| mime == "image/png") {
        return ClipKind::Image;
    }
    if types.iter().any(|mime| mime.starts_with("image/")) {
        return ClipKind::Image;
    }
    if types.iter().any(|mime| mime == "text/uri-list") {
        return ClipKind::File;
    }
    if types.iter().any(|mime| mime.starts_with("text/")) {
        return ClipKind::Text;
    }
    ClipKind::Binary
}

/// The maximum length of a thumbnail's longest edge, in pixels.
const THUMBNAIL_MAX_EDGE: u32 = 128;

/// Decode `bytes` as an image, downscale it so its longest edge is at most
/// [`THUMBNAIL_MAX_EDGE`] (never upscaling), re-encode as PNG, and return a
/// base64 `data:image/png;base64,...` URI. Returns `None` when the bytes cannot
/// be decoded as an image.
pub fn thumbnail(bytes: &[u8]) -> Option<String> {
    use image::GenericImageView as _;

    let image = match image::load_from_memory(bytes) {
        Ok(image) => image,
        Err(error) => {
            tracing::warn!(%error, "failed to decode clipboard image for thumbnail");
            return None;
        }
    };

    let (width, height) = image.dimensions();
    let scaled = if width.max(height) > THUMBNAIL_MAX_EDGE {
        image.resize(
            THUMBNAIL_MAX_EDGE,
            THUMBNAIL_MAX_EDGE,
            image::imageops::FilterType::Triangle,
        )
    } else {
        image
    };

    let mut buffer = std::io::Cursor::new(Vec::new());
    if let Err(error) = scaled.write_to(&mut buffer, image::ImageFormat::Png) {
        tracing::warn!(%error, "failed to encode clipboard thumbnail as PNG");
        return None;
    }

    let encoded = base64::engine::general_purpose::STANDARD.encode(buffer.get_ref());
    Some(format!("data:image/png;base64,{encoded}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn types(list: &[&str]) -> Vec<String> {
        list.iter().map(|mime| mime.to_string()).collect()
    }

    #[test]
    fn classify_prefers_image_png() {
        assert_eq!(
            classify(&types(&["text/plain", "image/png"])),
            ClipKind::Image
        );
    }

    #[test]
    fn classify_other_image_is_image() {
        assert_eq!(classify(&types(&["image/jpeg"])), ClipKind::Image);
    }

    #[test]
    fn classify_uri_list_is_file() {
        assert_eq!(
            classify(&types(&["text/uri-list", "text/plain"])),
            ClipKind::File
        );
    }

    #[test]
    fn classify_plain_text_is_text() {
        assert_eq!(
            classify(&types(&["text/plain;charset=utf-8"])),
            ClipKind::Text
        );
    }

    #[test]
    fn classify_unknown_is_binary() {
        assert_eq!(
            classify(&types(&["application/octet-stream"])),
            ClipKind::Binary
        );
    }

    #[test]
    fn classify_empty_is_binary() {
        assert_eq!(classify(&[]), ClipKind::Binary);
    }

    /// Build a tiny 2x2 red PNG in memory for the thumbnail tests.
    fn tiny_png() -> Vec<u8> {
        use image::{ImageBuffer, Rgba};
        let image: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_pixel(2, 2, Rgba([255, 0, 0, 255]));
        let mut buffer = std::io::Cursor::new(Vec::new());
        image
            .write_to(&mut buffer, image::ImageFormat::Png)
            .expect("encode tiny png");
        buffer.into_inner()
    }

    #[test]
    fn thumbnail_of_tiny_png_is_data_uri() {
        let bytes = tiny_png();
        let uri = thumbnail(&bytes).expect("thumbnail of a valid png");
        assert!(uri.starts_with("data:image/png;base64,"));
    }

    #[test]
    fn thumbnail_of_non_image_is_none() {
        assert_eq!(thumbnail(b"not an image at all"), None);
    }
}
