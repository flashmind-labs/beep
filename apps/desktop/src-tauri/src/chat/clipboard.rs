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
use image::{
    ExtendedColorType, ImageEncoder, ImageFormat, ImageReader, Limits, codecs::png::PngEncoder,
};
use std::sync::Arc;
#[cfg(desktop)]
use std::{borrow::Cow, io::Cursor, sync::Mutex};
use tauri::State;

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

    fn read_png(&self) -> Result<Option<String>> {
        Ok(None)
    }
}

#[cfg(desktop)]
impl ImageClipboard {
    pub(crate) fn copy_png(&self, png: &str) -> Result<()> {
        let image = decode(png)?;
        self.with_clipboard(|clipboard| clipboard.set_image(image).context("copy image"))
    }

    fn read_png(&self) -> Result<Option<String>> {
        let image = self.with_clipboard(|clipboard| match clipboard.get_image() {
            Ok(image) => Ok(Some(image)),
            Err(arboard::Error::ContentNotAvailable) => Ok(None),
            Err(error) => Err(error).context("read clipboard image"),
        })?;
        let Some(image) = image else {
            return Ok(None);
        };
        let mut png = Vec::new();
        PngEncoder::new(&mut png).write_image(
            &image.bytes,
            u32::try_from(image.width)?,
            u32::try_from(image.height)?,
            ExtendedColorType::Rgba8,
        )?;

        Ok(Some(STANDARD.encode(png)))
    }

    fn with_clipboard<T>(&self, run: impl FnOnce(&mut Clipboard) -> Result<T>) -> Result<T> {
        let mut clipboard = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("clipboard lock poisoned"))?;
        if clipboard.is_none() {
            *clipboard = Some(Clipboard::new().context("open clipboard")?);
        }

        run(clipboard.as_mut().context("clipboard unavailable")?)
    }
}

/// Used when the webview's paste event omits the native clipboard image.
#[tauri::command]
pub(crate) async fn read_clipboard_image(
    clipboard: State<'_, Arc<ImageClipboard>>,
) -> crate::error::Result<Option<String>> {
    let clipboard = clipboard.inner().clone();
    let png = tauri::async_runtime::spawn_blocking(move || clipboard.read_png())
        .await
        .map_err(anyhow::Error::from)??;
    Ok(png)
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
