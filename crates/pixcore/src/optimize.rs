//! Original optimization at add-time (R7 storage side): decode once, cap the
//! long edge, re-encode with sane defaults. Never palette-reduced.

use std::fs;
use std::path::Path;

use image::{DynamicImage, GenericImageView, ImageEncoder, ImageFormat};

use crate::config::Optimize;
use crate::error::{PixError, Result};
use crate::pipeline::downscale;

pub struct Optimized {
    /// Encoded bytes for `library/<id>/original.<ext>`.
    pub bytes: Vec<u8>,
    pub ext: &'static str,
    pub width: u32,
    pub height: u32,
    pub note: String,
}

/// Decode → cap to `max_side` (iterative box halving) → encode (PNG when the
/// source carries alpha or *was* PNG; JPEG q87 otherwise).
pub fn optimize(src_path: &Path, cfg: &Optimize) -> Result<Optimized> {
    let bytes = fs::read(src_path).map_err(|e| PixError::io(src_path, e))?;
    let img = image::ImageReader::new(std::io::Cursor::new(&bytes))
        .with_guessed_format()
        .map_err(|e| PixError::Decode { path: src_path.to_path_buf(), reason: e.to_string() })?
        .decode()
        .map_err(|e| PixError::Decode { path: src_path.to_path_buf(), reason: e.to_string() })?;

    let (w, h) = img.dimensions();
    if w.min(h) < 16 {
        return Err(PixError::Unsupported {
            path: src_path.to_path_buf(),
            reason: format!("image too small ({w}×{h}); minimum side is 16 px"),
        });
    }

    let max_side = cfg.max_side.clamp(1080, 7680);
    let rgba = img.into_rgba8();
    let (w, h) = (rgba.width(), rgba.height());
    let capped = if w.max(h) > max_side {
        let scale = max_side as f64 / w.max(h) as f64;
        let tw = ((w as f64 * scale).round() as u32).max(1);
        let th = ((h as f64 * scale).round() as u32).max(1);
        downscale(&rgba, tw, th)
    } else {
        rgba
    };
    let (cw, ch) = (capped.width(), capped.height());

    let has_alpha = capped.pixels().any(|p| p.0[3] != 255);
    let was_png = matches!(
        image::ImageReader::new(std::io::Cursor::new(&bytes))
            .with_guessed_format()
            .ok()
            .and_then(|r| r.format()),
        Some(ImageFormat::Png)
    );

    let mut out: Vec<u8> = Vec::new();
    let (ext, note) = if has_alpha || was_png {
        let enc = image::codecs::png::PngEncoder::new(&mut out);
        enc
            .write_image(capped.as_raw(), cw, ch, image::ColorType::Rgba8)
            .map_err(|e| PixError::other(format!("png encode: {e}")))?;
        let why = if has_alpha { "png(alpha)" } else { "png" };
        ("png", why.to_string())
    } else {
        let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, cfg.jpeg_quality);
        // JPEG has no alpha; flatten onto black for any stray transparency.
        let opaque = strip_alpha(&capped);
        enc.write_image(opaque.as_raw(), cw, ch, image::ColorType::Rgb8)
            .map_err(|e| PixError::other(format!("jpeg encode: {e}")))?;
        ("jpg", format!("jpeg q{}", cfg.jpeg_quality))
    };

    Ok(Optimized { bytes: out, ext, width: cw, height: ch, note })
}

fn strip_alpha(img: &image::RgbaImage) -> image::RgbImage {
    image::ImageBuffer::from_fn(img.width(), img.height(), |x, y| {
        let p = img.get_pixel(x, y).0;
        let a = p[3] as u32;
        image::Rgb([
            ((p[0] as u32 * a) / 255) as u8,
            ((p[1] as u32 * a) / 255) as u8,
            ((p[2] as u32 * a) / 255) as u8,
        ])
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::testsrc;
    use image::ImageEncoder as _;

    fn write_tmp(img: &image::RgbaImage, fmt: ImageFormat) -> tempfile::NamedTempFile {
        let ext = match fmt { ImageFormat::Png => "png", _ => "jpg" };
        let f = tempfile::NamedTempFile::new_in(std::env::temp_dir()).unwrap_or_else(|_| tempfile::NamedTempFile::new().unwrap());
        let mut buf = Vec::new();
        if fmt == ImageFormat::Png {
            let enc = image::codecs::png::PngEncoder::new(&mut buf);
            enc.write_image(img.as_raw(), img.width(), img.height(), image::ColorType::Rgba8).unwrap();
        } else {
            let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 90);
            let rgb = strip_alpha(img);
            enc.write_image(rgb.as_raw(), rgb.width(), rgb.height(), image::ColorType::Rgb8).unwrap();
        }
        std::fs::write(f.path(), &buf).unwrap();
        let _ = ext;
        f
    }

    #[test]
    fn png_with_alpha_stays_png() {
        let f = write_tmp(&testsrc::gradient64(), ImageFormat::Png); // has alpha band
        let o = optimize(f.path(), &Optimize::default()).unwrap();
        assert_eq!(o.ext, "png");
        assert!(o.note.contains("alpha"));
    }

    #[test]
    fn photo_goes_jpeg_and_caps() {
        let big = image::ImageBuffer::from_fn(5000, 2500, |x, y| {
            image::Rgba([(x % 200) as u8, (y % 100) as u8, 50, 255])
        });
        let f = write_tmp(&big, ImageFormat::Jpeg);
        let o = optimize(f.path(), &Optimize { max_side: 3840, jpeg_quality: 87 }).unwrap();
        assert_eq!(o.ext, "jpg");
        assert!(o.width.max(o.height) <= 3840);
        assert!(o.width >= 3830); // rounding
    }

    #[test]
    fn tiny_images_rejected() {
        let img = image::ImageBuffer::from_pixel(8, 8, image::Rgba([0, 0, 0, 255]));
        let f = write_tmp(&img, ImageFormat::Png);
        let err = optimize(f.path(), &Optimize::default()).unwrap_err().to_string();
        assert!(err.contains("too small"), "{err}");
    }
}
