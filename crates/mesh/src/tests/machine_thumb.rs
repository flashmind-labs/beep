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
