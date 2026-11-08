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
