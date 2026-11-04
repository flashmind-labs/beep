//! Pictures of a coding agent session's log made small for the log to draw.

use image::{
    DynamicImage, ImageReader, ImageResult, Limits, RgbImage, Rgba, RgbaImage,
    codecs::jpeg::JpegEncoder, imageops::overlay,
};
use std::{io::Cursor, sync::Arc};
use tokio::{sync::Semaphore, task::spawn_blocking};

/// A picture as a node gave it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    pub mime: String,
    pub data: Vec<u8>,
}

/// The longest edge of a thumbnail. The log draws its largest 160 CSS pixels
/// high and as wide as they come, so 320 on a 2x display, which keeps a
/// picture up to 2.5 times as wide as it is high sharp.
const THUMB_EDGE: u32 = 800;
const THUMB_QUALITY: u8 = 80;
/// Thumbnails made at once; each holds a decoded picture.
static THUMBS: Semaphore = Semaphore::const_new(4);
/// The most one decoded picture may take, which fits a 6K screenshot, and
/// the longest edge read.
const DECODED_BYTES: u64 = 128 * 1024 * 1024;
const DECODED_EDGE: u32 = 16 * 1024;

/// `image` made small, off the async workers, a few at a time. One already
/// small, or that cannot be made small, stays as it is.
pub async fn thumb_of(image: Picture) -> Picture {
    let Ok(slot) = THUMBS.acquire().await else {
        return image;
    };
    let image = Arc::new(image);
    let reading = image.clone();
    let made = spawn_blocking(move || {
        let _slot = slot;
        thumbnail(&reading.data)
    })
    .await;
    match made {
        Ok(Ok(Some(data))) => Picture {
            mime: "image/jpeg".to_owned(),
            data,
        },
        Ok(Ok(None)) => Arc::unwrap_or_clone(image),
        Ok(Err(e)) => {
            tracing::debug!("agent image sent whole: {e}");
            Arc::unwrap_or_clone(image)
        }
        Err(e) => {
            tracing::warn!("the thumbnail task died: {e}");
            Arc::unwrap_or_clone(image)
        }
    }
}

/// The JPEG thumbnail of the picture `data`. `None` when it would not be
/// smaller, in pixels or in bytes.
fn thumbnail(data: &[u8]) -> ImageResult<Option<Vec<u8>>> {
    let Some(small) = small_picture(data)? else {
        return Ok(None);
    };
    let mut jpeg = Vec::new();
    small.write_with_encoder(JpegEncoder::new_with_quality(&mut jpeg, THUMB_QUALITY))?;
    Ok((jpeg.len() < data.len()).then_some(jpeg))
}

/// The picture `data` no larger than `THUMB_EDGE`, on white where it is
/// transparent; of a GIF, its first frame. `None` when it is already small.
/// One that would decode past the limits is an error before its pixels are
/// allocated.
fn small_picture(data: &[u8]) -> ImageResult<Option<RgbImage>> {
    let (width, height) = ImageReader::new(Cursor::new(data))
        .with_guessed_format()?
        .into_dimensions()?;
    if width.max(height) <= THUMB_EDGE {
        return Ok(None);
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(DECODED_EDGE);
    limits.max_image_height = Some(DECODED_EDGE);
    limits.max_alloc = Some(DECODED_BYTES);
    let mut reader = ImageReader::new(Cursor::new(data)).with_guessed_format()?;
    reader.limits(limits);
    let small = reader.decode()?.thumbnail(THUMB_EDGE, THUMB_EDGE);
    if !small.color().has_alpha() {
        return Ok(Some(small.to_rgb8()));
    }
    let mut flat = RgbaImage::from_pixel(small.width(), small.height(), Rgba([255; 4]));
    overlay(&mut flat, &small.to_rgba8(), 0, 0);
    Ok(Some(DynamicImage::ImageRgba8(flat).to_rgb8()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{
        ExtendedColorType, Frame, GenericImageView, ImageEncoder, ImageError, ImageFormat, Rgb,
        codecs::{gif::GifEncoder, png::PngEncoder},
        load_from_memory,
    };

    /// A PNG of noise, which does not compress: its JPEG is smaller.
    fn noise(width: u32, height: u32) -> Picture {
        let pixels = RgbImage::from_fn(width, height, |x, y| {
            let [r, g, b, _] = (x ^ (y << 12)).wrapping_mul(2_654_435_761).to_be_bytes();
            Rgb([r, g, b])
        });
        let mut png = Cursor::new(Vec::new());
        pixels.write_to(&mut png, ImageFormat::Png).unwrap();
        Picture {
            mime: "image/png".into(),
            data: png.into_inner(),
        }
    }

    #[tokio::test]
    async fn a_large_picture_becomes_a_smaller_jpeg() {
        let full = noise(1600, 900);
        let thumb = thumb_of(full.clone()).await;
        assert_eq!(thumb.mime, "image/jpeg");
        assert!(thumb.data.len() < full.data.len());
        let drawn = load_from_memory(&thumb.data).unwrap();
        assert_eq!(drawn.dimensions(), (800, 450));
    }

    #[tokio::test]
    async fn a_picture_already_small_stays_as_it_is() {
        let full = noise(800, 600);
        assert_eq!(thumb_of(full.clone()).await, full);
    }

    #[tokio::test]
    async fn bad_bytes_stay_as_they_are() {
        let full = Picture {
            mime: "image/png".into(),
            data: b"\x89PNG\r\n\x1a\nnot really".to_vec(),
        };
        assert_eq!(thumb_of(full.clone()).await, full);
    }

    #[tokio::test]
    async fn a_decompression_bomb_is_refused_and_sent_as_it_is() {
        // 144 MB of pixels in little space.
        let mut data = Vec::new();
        PngEncoder::new(&mut data)
            .write_image(
                &vec![0; 16_000 * 9_000],
                16_000,
                9_000,
                ExtendedColorType::L8,
            )
            .unwrap();
        assert!(data.len() < 1024 * 1024, "{} bytes", data.len());
        assert!(matches!(small_picture(&data), Err(ImageError::Limits(_))));
        let full = Picture {
            mime: "image/png".into(),
            data,
        };
        assert_eq!(thumb_of(full.clone()).await, full);
    }

    #[test]
    fn transparency_turns_white() {
        let mut png = Cursor::new(Vec::new());
        RgbaImage::from_pixel(1600, 900, Rgba([0, 0, 0, 0]))
            .write_to(&mut png, ImageFormat::Png)
            .unwrap();
        let small = small_picture(png.get_ref()).unwrap().unwrap();
        assert_eq!(small.dimensions(), (800, 450));
        assert_eq!(small.get_pixel(100, 100), &Rgb([255, 255, 255]));
    }

    #[test]
    fn a_gif_gives_its_first_frame() {
        let frame = |fill| Frame::new(RgbaImage::from_pixel(1000, 2, Rgba(fill)));
        let mut gif = Vec::new();
        let mut encoder = GifEncoder::new(&mut gif);
        encoder.encode_frame(frame([255, 0, 0, 255])).unwrap();
        encoder.encode_frame(frame([0, 0, 255, 255])).unwrap();
        drop(encoder);
        let small = small_picture(&gif).unwrap().unwrap();
        let [r, _, b] = small.get_pixel(0, 0).0;
        assert!(r > 200 && b < 50, "red, not {r} {b}");
    }
}
