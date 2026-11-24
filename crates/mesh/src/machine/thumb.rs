//! Pictures of a coding agent session's log made small for the log to draw.

use image::{
    DynamicImage, ImageReader, ImageResult, Limits, RgbImage, Rgba, RgbaImage,
    codecs::jpeg::JpegEncoder, imageops::overlay,
};
use std::{io::Cursor, sync::Arc};
use tokio::{sync::Semaphore, task::spawn_blocking};

pub use super::sessions::Picture;

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
#[path = "../tests/machine_thumb.rs"]
mod tests;
