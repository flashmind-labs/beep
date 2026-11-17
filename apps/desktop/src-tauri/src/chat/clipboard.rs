//! The native image clipboard, kept alive while another app requests its
//! contents. Phones use the webview's clipboard.

use anyhow::Result;
#[cfg(desktop)]
use anyhow::{Context, ensure};
#[cfg(desktop)]
use arboard::{Clipboard, ImageData};
#[cfg(desktop)]
use base64::{Engine, engine::general_purpose::STANDARD};
#[cfg(desktop)]
use image::{ImageFormat, ImageReader, Limits};
#[cfg(desktop)]
use std::{borrow::Cow, io::Cursor, sync::Mutex};

#[cfg(desktop)]
#[derive(Default)]
pub(crate) struct ImageClipboard(Mutex<Option<Clipboard>>);

#[cfg(mobile)]
#[derive(Default)]
pub(crate) struct ImageClipboard {}

#[cfg(mobile)]
impl ImageClipboard {
    pub(crate) fn copy_png(&self, _png: &str) -> Result<()> {
        anyhow::bail!("image copying is not included in this build")
    }
}

#[cfg(desktop)]
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

#[cfg(desktop)]
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

#[cfg(all(test, desktop))]
#[path = "../tests/chat_clipboard.rs"]
mod tests;
