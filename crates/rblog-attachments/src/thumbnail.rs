//! Image thumbnailing.
//!
//! We keep this dirt-simple: feed in raw image bytes + a list of profiles
//! and get back the resized JPEG bytes. The caller owns where the
//! thumbnails are uploaded.

use std::io::Cursor;

use bytes::Bytes;
use image::imageops::FilterType;
use image::ImageFormat;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ThumbnailError {
    #[error("decode: {0}")]
    Decode(#[from] image::ImageError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone)]
pub struct ThumbnailProfile {
    /// Identifier — written into [`AttachmentStatus::thumbnails`] under this key.
    pub name: String,
    /// Maximum width in pixels. Aspect ratio is preserved.
    pub max_width: u32,
    /// JPEG quality (0–100). Default 80 is a good speed/size tradeoff.
    pub quality: u8,
}

impl ThumbnailProfile {
    #[must_use]
    pub fn new(name: impl Into<String>, max_width: u32) -> Self {
        Self {
            name: name.into(),
            max_width,
            quality: 80,
        }
    }
}

/// Result of one rendered thumbnail.
#[derive(Debug, Clone)]
pub struct ThumbnailResult {
    pub name: String,
    pub bytes: Bytes,
    pub width: u32,
    pub height: u32,
}

/// Engine — holds a list of profiles. Cheap to clone.
#[derive(Debug, Clone)]
pub struct ThumbnailEngine {
    profiles: Vec<ThumbnailProfile>,
}

impl Default for ThumbnailEngine {
    fn default() -> Self {
        Self {
            profiles: vec![ThumbnailProfile::new("thumb", 320)],
        }
    }
}

impl ThumbnailEngine {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn empty() -> Self {
        Self { profiles: vec![] }
    }

    #[must_use]
    pub fn with_profile(mut self, profile: ThumbnailProfile) -> Self {
        self.profiles.push(profile);
        self
    }

    pub fn add_profile(&mut self, profile: ThumbnailProfile) {
        self.profiles.push(profile);
    }

    #[must_use]
    pub fn profiles(&self) -> &[ThumbnailProfile] {
        &self.profiles
    }

    /// Decode `bytes`, then render every profile. Returns one entry per
    /// profile in the order they were registered.
    pub fn render(&self, bytes: &[u8]) -> Result<Vec<ThumbnailResult>, ThumbnailError> {
        if self.profiles.is_empty() {
            return Ok(Vec::new());
        }
        let img = image::load_from_memory(bytes)?;
        let mut out = Vec::with_capacity(self.profiles.len());
        for profile in &self.profiles {
            let scaled = if img.width() > profile.max_width {
                img.resize(profile.max_width, u32::MAX, FilterType::Triangle)
            } else {
                img.clone()
            };
            let width = scaled.width();
            let height = scaled.height();
            let mut buf = Vec::with_capacity((width * height) as usize / 4);
            let cursor = Cursor::new(&mut buf);
            let mut writer = std::io::BufWriter::new(cursor);
            scaled.to_rgb8().write_to(&mut writer, ImageFormat::Jpeg)?;
            drop(writer);
            out.push(ThumbnailResult {
                name: profile.name.clone(),
                bytes: Bytes::from(buf),
                width,
                height,
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgb};

    fn sample_png() -> Vec<u8> {
        #[allow(clippy::cast_possible_truncation)]
        let img: ImageBuffer<Rgb<u8>, _> =
            ImageBuffer::from_fn(800, 600, |x, y| Rgb([((x ^ y) as u8), 0, 0]));
        let mut buf = Vec::new();
        img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
            .unwrap();
        buf
    }

    #[test]
    fn default_engine_produces_one_thumb() {
        let engine = ThumbnailEngine::default();
        let bytes = sample_png();
        let thumbs = engine.render(&bytes).unwrap();
        assert_eq!(thumbs.len(), 1);
        assert_eq!(thumbs[0].name, "thumb");
        assert!(thumbs[0].width <= 320);
        assert!(thumbs[0].height > 0);
    }

    #[test]
    fn small_images_are_passed_through() {
        let mut buf = Vec::new();
        let small: ImageBuffer<Rgb<u8>, _> = ImageBuffer::from_fn(64, 32, |_, _| Rgb([0, 0, 0]));
        small
            .write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
            .unwrap();
        let thumbs = ThumbnailEngine::default().render(&buf).unwrap();
        assert_eq!(thumbs[0].width, 64);
        assert_eq!(thumbs[0].height, 32);
    }

    #[test]
    fn empty_engine_returns_no_thumbnails() {
        let thumbs = ThumbnailEngine::empty().render(&sample_png()).unwrap();
        assert!(thumbs.is_empty());
    }
}
