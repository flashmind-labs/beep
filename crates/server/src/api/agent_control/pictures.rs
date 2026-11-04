//! The pictures of a coding agent session's log, whole or made small for it.

use super::live_host;
use crate::{
    account::user::User,
    api::{Reply, router::Cx},
    protocol::*,
    store::{AgentImageBytes, AgentImageOf},
};
use image::{
    DynamicImage, ImageReader, ImageResult, Limits, RgbImage, Rgba, RgbaImage,
    codecs::jpeg::JpegEncoder, imageops::overlay,
};
use std::{io::Cursor, sync::Arc};
use tokio::{sync::Semaphore, task::spawn_blocking};

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

/// Copies being kept at once...
static SAVING: Semaphore = Semaphore::const_new(2);
/// ...and the MiB of copies waiting or being kept. Past this a copy is not
/// kept: the node still has the picture for the next time it is asked for.
static SAVE_ROOM: Semaphore = Semaphore::const_new(256);

/// The kept copy of the picture, or else the node's, which is then kept.
pub(in crate::api) async fn get_agent_image(cx: Cx, get: GetAgentImage) -> Reply<AgentImageData> {
    let image = full_picture(&cx, &get.machine_id, &get.session, &get.key).await?;
    Ok(AgentImageData {
        key: get.key,
        mime: image.mime,
        data: image.data,
    })
}

/// `get_agent_image`'s picture, made small.
pub(in crate::api) async fn get_agent_thumb(cx: Cx, get: GetAgentThumb) -> Reply<AgentImageData> {
    let image = full_picture(&cx, &get.machine_id, &get.session, &get.key).await?;
    let image = thumb_of(image).await;
    Ok(AgentImageData {
        key: get.key,
        mime: image.mime,
        data: image.data,
    })
}

async fn full_picture(
    cx: &Cx,
    machine_id: &str,
    session: &str,
    key: &str,
) -> Result<AgentImageBytes, ApiError> {
    if !is_image_key(key) {
        return Err(ApiError::bad_request("That is not an image key."));
    }
    match cx.user.agent_image(key).await {
        Ok(Some(kept)) => return Ok(kept),
        Ok(None) => {}
        // The node may still have it.
        Err(e) => log::warn!("agent image not read; key={key} error={e:#}"),
    }
    let live = live_host(cx, machine_id).await?;
    let image = live
        .online
        .agent_image(session, key)
        .await
        .map_err(|e| {
            // A node from before images by key closes the stream.
            ApiError::bad_request(format!(
                "Could not read the image from {}: {e:#}. Updating beep-node there may help.",
                live.host.machine_name
            ))
        })?
        .ok_or(ApiError::NotFound {})?;
    if image_key(&image.data) != key {
        log::warn!("node gave another image; session={session} key={key}");
        return Err(ApiError::NotFound {});
    }
    let image = AgentImageBytes {
        mime: image.mime,
        data: image.data,
    };
    keep_later(&cx.user, machine_id, session, key, &image);
    Ok(image)
}

/// Keeps a copy of `image` without holding up the reply. The task owns all it
/// needs, ends with the save and logs its failure.
fn keep_later(
    user: &Arc<User>,
    machine_id: &str,
    session: &str,
    key: &str,
    image: &AgentImageBytes,
) {
    let mib = u32::try_from(image.data.len() >> 20)
        .unwrap_or(u32::MAX)
        .saturating_add(1);
    let Ok(room) = SAVE_ROOM.try_acquire_many(mib) else {
        log::warn!("agent image not kept, too many waiting; key={key}");
        return;
    };
    let (user, machine_id, session, key, image) = (
        user.clone(),
        machine_id.to_owned(),
        session.to_owned(),
        key.to_owned(),
        image.clone(),
    );
    tokio::spawn(async move {
        let _room = room;
        let saved = match SAVING.acquire().await {
            Ok(_saving) => {
                let of = AgentImageOf {
                    machine_id: &machine_id,
                    session: &session,
                };
                user.save_agent_image(of, &image).await
            }
            Err(e) => Err(e.into()),
        };
        if let Err(e) = saved {
            log::warn!("agent image not kept; key={key} error={e:#}");
        }
    });
}

/// Whether `key` is what `image_key` makes: 16 lowercase hex digits.
fn is_image_key(key: &str) -> bool {
    key.len() == 16 && key.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// `image` made small, off the async workers, a few at a time. One already
/// small, or that cannot be made small, stays as it is.
async fn thumb_of(image: AgentImageBytes) -> AgentImageBytes {
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
        Ok(Ok(Some(data))) => AgentImageBytes {
            mime: "image/jpeg".to_owned(),
            data,
        },
        Ok(Ok(None)) => Arc::unwrap_or_clone(image),
        Ok(Err(e)) => {
            log::debug!("agent image sent whole: {e}");
            Arc::unwrap_or_clone(image)
        }
        Err(e) => {
            log::warn!("the thumbnail task died: {e}");
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
    fn noise(width: u32, height: u32) -> AgentImageBytes {
        let pixels = RgbImage::from_fn(width, height, |x, y| {
            let [r, g, b, _] = (x ^ (y << 12)).wrapping_mul(2_654_435_761).to_be_bytes();
            Rgb([r, g, b])
        });
        let mut png = Cursor::new(Vec::new());
        pixels.write_to(&mut png, ImageFormat::Png).unwrap();
        AgentImageBytes {
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
        let full = AgentImageBytes {
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
        let full = AgentImageBytes {
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
