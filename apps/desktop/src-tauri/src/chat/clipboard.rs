//! The native image clipboard, kept alive while another app requests its contents.

use anyhow::{Context, Result, ensure};
use arboard::{Clipboard, ImageData};
use base64::{Engine, engine::general_purpose::STANDARD};
use image::{ImageFormat, ImageReader, Limits};
use std::{borrow::Cow, io::Cursor, sync::Mutex};

#[derive(Default)]
pub(crate) struct ImageClipboard(Mutex<Option<Clipboard>>);

impl ImageClipboard {
    pub(crate) fn copy_png(&self, png: &str) -> Result<()> {
        let image = decode(png)?;
        let mut clipboard = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("clipboard lock poisoned"))?;
        if clipboard.is_none() {
            *clipboard = Some(Clipboard::new().context("open clipboard")?);
        }
        clipboard
            .as_mut()
            .context("clipboard unavailable")?
            .set_image(image)
            .context("copy image")
    }
}

fn decode(png: &str) -> Result<ImageData<'static>> {
    ensure!(png.len() <= 24_000_000, "image too large");
    let bytes = STANDARD.decode(png).context("decode image")?;
    let mut reader = ImageReader::with_format(Cursor::new(bytes), ImageFormat::Png);
    let mut limits = Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().context("read PNG")?.into_rgba8();
    Ok(ImageData {
        width: usize::try_from(image.width())?,
        height: usize::try_from(image.height())?,
        bytes: Cow::Owned(image.into_raw()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, Rgba, RgbaImage};

    fn png(width: u32, height: u32) -> String {
        let image = DynamicImage::ImageRgba8(RgbaImage::from_pixel(
            width,
            height,
            Rgba([20, 40, 60, 255]),
        ));
        let mut out = Cursor::new(Vec::new());
        image.write_to(&mut out, ImageFormat::Png).unwrap();
        STANDARD.encode(out.into_inner())
    }

    #[test]
    fn preserves_the_pixels_for_the_native_clipboard() {
        let image = decode(&png(2, 1)).unwrap();
        assert_eq!((image.width, image.height), (2, 1));
        assert_eq!(image.bytes.as_ref(), &[20, 40, 60, 255, 20, 40, 60, 255]);
    }

    #[test]
    fn rejects_invalid_or_oversized_images() {
        assert!(decode("not base64").is_err());
        assert!(decode(&STANDARD.encode(b"not a PNG")).is_err());
        assert!(decode(&png(4097, 1)).is_err());
        assert!(decode(&png(1, 4097)).is_err());
        assert!(decode(&"A".repeat(24_000_001)).is_err());
    }
}
