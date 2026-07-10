//! Pure helpers for choosing the best StatusNotifierItem icon pixmap and
//! encoding it as a PNG data URI.
//!
//! StatusNotifierItem exposes icons through the `IconPixmap` property, whose
//! DBus signature is `a(iiay)`: an array of (width, height, bytes) triples.
//! The bytes are ARGB32 in network (big-endian) byte order, so each pixel is
//! four bytes laid out as `[alpha, red, green, blue]`. PNG expects RGBA, that
//! is `[red, green, blue, alpha]`, so encoding reorders every pixel.

use base64::Engine;

/// Choose the most suitable icon pixmap from the candidate set.
///
/// Preference order: the largest width that is at or below 64 pixels; if
/// every candidate is wider than 64 pixels, the smallest width overall.
/// Candidates whose width or height is not strictly positive, or whose byte
/// length is not exactly `width * height * 4`, are rejected outright.
/// Returns [`None`] when no valid candidate exists.
pub fn best_pixmap(pixmaps: &[(i32, i32, Vec<u8>)]) -> Option<&(i32, i32, Vec<u8>)> {
    let valid = pixmaps
        .iter()
        .filter(|(width, height, bytes)| has_expected_length(*width, *height, bytes.len()));

    let at_or_under_limit = valid
        .clone()
        .filter(|(width, _, _)| *width <= 64)
        .max_by_key(|(width, _, _)| *width);

    if let Some(best) = at_or_under_limit {
        return Some(best);
    }

    valid.min_by_key(|(width, _, _)| *width)
}

/// Encode an ARGB32 (big-endian) pixmap as a base64 PNG data URI.
///
/// The input is validated: `width` and `height` must be strictly positive and
/// `argb_big_endian.len()` must equal `width * height * 4`; otherwise this
/// returns [`None`]. Each source pixel `[alpha, red, green, blue]` is
/// reordered to `[red, green, blue, alpha]`, encoded as an 8-bit RGBA PNG, and
/// base64-encoded with the standard engine. Any encoding failure yields
/// [`None`] rather than a panic.
pub fn pixmap_to_data_uri(width: i32, height: i32, argb_big_endian: &[u8]) -> Option<String> {
    if !has_expected_length(width, height, argb_big_endian.len()) {
        return None;
    }

    let mut rgba = Vec::with_capacity(argb_big_endian.len());
    for pixel in argb_big_endian.chunks_exact(4) {
        let alpha = pixel[0];
        let red = pixel[1];
        let green = pixel[2];
        let blue = pixel[3];
        rgba.extend_from_slice(&[red, green, blue, alpha]);
    }

    let png_bytes = encode_png(width as u32, height as u32, &rgba)?;
    let payload = base64::engine::general_purpose::STANDARD.encode(png_bytes);
    Some(format!("data:image/png;base64,{payload}"))
}

/// Return whether `byte_length` matches `width * height * 4` for strictly
/// positive dimensions, computing the expected length in [`i64`] so the
/// multiplication cannot overflow before the comparison.
fn has_expected_length(width: i32, height: i32, byte_length: usize) -> bool {
    if width <= 0 || height <= 0 {
        return false;
    }
    let expected = (width as i64) * (height as i64) * 4;
    expected == byte_length as i64
}

/// Encode an 8-bit RGBA buffer as PNG bytes, returning [`None`] on any error.
fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    let mut png_bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut png_bytes, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);

    let mut writer = encoder.write_header().ok()?;
    writer.write_image_data(rgba).ok()?;
    writer.finish().ok()?;

    Some(png_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    fn argb(a: u8, r: u8, g: u8, b: u8) -> [u8; 4] {
        [a, r, g, b]
    }

    #[test]
    fn best_pixmap_prefers_largest_at_or_under_64() {
        let candidates = vec![
            (16, 16, vec![0u8; 16 * 16 * 4]),
            (48, 48, vec![0u8; 48 * 48 * 4]),
            (128, 128, vec![0u8; 128 * 128 * 4]),
        ];
        let best = best_pixmap(&candidates).expect("candidate");
        assert_eq!(best.0, 48);
    }

    #[test]
    fn best_pixmap_falls_back_to_smallest_when_all_over_64() {
        let candidates = vec![
            (128, 128, vec![0u8; 128 * 128 * 4]),
            (96, 96, vec![0u8; 96 * 96 * 4]),
        ];
        let best = best_pixmap(&candidates).expect("candidate");
        assert_eq!(best.0, 96);
    }

    #[test]
    fn best_pixmap_rejects_size_mismatch() {
        let candidates = vec![(16, 16, vec![0u8; 3])];
        assert!(best_pixmap(&candidates).is_none());
    }

    #[test]
    fn pixmap_round_trips_through_png() {
        let pixels: Vec<u8> = [
            argb(255, 255, 0, 0),
            argb(255, 0, 255, 0),
            argb(255, 0, 0, 255),
            argb(128, 10, 20, 30),
        ]
        .concat();
        let uri = pixmap_to_data_uri(2, 2, &pixels).expect("data uri");
        let payload = uri.strip_prefix("data:image/png;base64,").expect("prefix");
        let png_bytes = base64::engine::general_purpose::STANDARD
            .decode(payload)
            .expect("base64");
        let decoder = png::Decoder::new(std::io::Cursor::new(png_bytes));
        let mut reader = decoder.read_info().expect("png info");
        let mut buf = vec![0u8; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buf).expect("frame");
        assert_eq!((info.width, info.height), (2, 2));
        assert_eq!(&buf[0..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn pixmap_rejects_bad_length() {
        assert!(pixmap_to_data_uri(2, 2, &[0u8; 3]).is_none());
    }
}
