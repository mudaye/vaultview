#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use crate::constants::*;
use crate::error::Error;
use std::io::Cursor;
use std::time::Duration;

fn sample_image() -> image::DynamicImage {
    image::DynamicImage::ImageRgb8(image::RgbImage::from_fn(5, 4, |x, y| {
        image::Rgb([(x * 40) as u8, (y * 50) as u8, 99])
    }))
}

fn encode(img: &image::DynamicImage, format: image::ImageFormat) -> Vec<u8> {
    let mut buf = Cursor::new(Vec::new());
    img.write_to(&mut buf, format).unwrap();
    buf.into_inner()
}

fn gif_with_delays(delays_ms: [u32; 2]) -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let mut encoder = image::codecs::gif::GifEncoder::new(&mut buf);
        let frames = [0u8, 200].into_iter().zip(delays_ms).map(|(shade, delay)| {
            image::Frame::from_parts(
                image::RgbaImage::from_pixel(4, 4, image::Rgba([shade, shade, shade, 255])),
                0,
                0,
                image::Delay::from_numer_denom_ms(delay, 1),
            )
        });
        encoder.encode_frames(frames).unwrap();
    }
    buf
}

fn jpeg_with_orientation(big_endian: bool, value: u16) -> Vec<u8> {
    let mut tiff = Vec::new();
    if big_endian {
        tiff.extend_from_slice(b"MM\x00\x2A\x00\x00\x00\x08");
        tiff.extend_from_slice(&1u16.to_be_bytes());
        tiff.extend_from_slice(&0x0112u16.to_be_bytes());
        tiff.extend_from_slice(&3u16.to_be_bytes());
        tiff.extend_from_slice(&1u32.to_be_bytes());
        tiff.extend_from_slice(&value.to_be_bytes());
        tiff.extend_from_slice(&[0, 0]);
        tiff.extend_from_slice(&0u32.to_be_bytes());
    } else {
        tiff.extend_from_slice(b"II\x2A\x00\x08\x00\x00\x00");
        tiff.extend_from_slice(&1u16.to_le_bytes());
        tiff.extend_from_slice(&0x0112u16.to_le_bytes());
        tiff.extend_from_slice(&3u16.to_le_bytes());
        tiff.extend_from_slice(&1u32.to_le_bytes());
        tiff.extend_from_slice(&value.to_le_bytes());
        tiff.extend_from_slice(&[0, 0]);
        tiff.extend_from_slice(&0u32.to_le_bytes());
    }
    let mut out = vec![0xFF, 0xD8, 0xFF, 0xE1];
    let length = (2 + 6 + tiff.len()) as u16;
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(b"Exif\0\0");
    out.extend_from_slice(&tiff);
    out.extend_from_slice(&[0xFF, 0xD9]);
    out
}

// ---------- decoding ----------

#[test]
fn lossless_formats_decode_to_exact_rgba_pixels() {
    for format in [
        image::ImageFormat::Png,
        image::ImageFormat::Bmp,
        image::ImageFormat::Tiff,
        image::ImageFormat::WebP,
    ] {
        let bytes = encode(&sample_image(), format);
        match decode_image(&bytes).unwrap() {
            Decoded::Still(img) => {
                assert_eq!((img.width, img.height), (5, 4), "{format:?}");
                assert_eq!(img.rgba.len(), 5 * 4 * 4);
                assert_eq!(img.pixel(0, 0), [0, 0, 99, 255], "{format:?}");
                assert_eq!(img.pixel(4, 3), [160, 150, 99, 255], "{format:?}");
            }
            other => panic!("{format:?}: expected a still image, got {other:?}"),
        }
    }
}

#[test]
fn lossy_formats_decode_with_correct_dimensions() {
    for format in [image::ImageFormat::Jpeg, image::ImageFormat::Gif] {
        let bytes = encode(&sample_image(), format);
        match decode_image(&bytes).unwrap() {
            Decoded::Still(img) => assert_eq!((img.width, img.height), (5, 4), "{format:?}"),
            other => panic!("{format:?}: expected a still image, got {other:?}"),
        }
    }
}

#[test]
fn animated_gifs_keep_all_frames_and_their_delays() {
    let bytes = gif_with_delays([200, 0]);
    match decode_image(&bytes).unwrap() {
        Decoded::Animated(frames) => {
            assert_eq!(frames.len(), 2);
            assert_eq!((frames[0].image.width, frames[0].image.height), (4, 4));
            assert_eq!(frames[0].delay, Duration::from_millis(200));
            assert_eq!(
                frames[1].delay,
                Duration::from_millis(FRAME_DELAY_DEFAULT_MS)
            );
            assert_ne!(frames[0].image.pixel(0, 0), frames[1].image.pixel(0, 0));
        }
        other => panic!("expected an animation, got {other:?}"),
    }
}

#[test]
fn frame_delays_below_the_minimum_are_normalised() {
    let default = Duration::from_millis(FRAME_DELAY_DEFAULT_MS);
    assert_eq!(normalize_frame_delay(Duration::ZERO), default);
    assert_eq!(
        normalize_frame_delay(Duration::from_millis(FRAME_DELAY_MIN_MS - 1)),
        default
    );
    let at_min = Duration::from_millis(FRAME_DELAY_MIN_MS);
    assert_eq!(normalize_frame_delay(at_min), at_min);
    assert_eq!(
        normalize_frame_delay(Duration::from_millis(500)),
        Duration::from_millis(500)
    );
}

#[test]
fn undecodable_input_is_an_error_not_a_panic() {
    assert!(matches!(decode_image(&[]), Err(Error::Decode(_))));
    assert!(matches!(
        decode_image(b"this is not an image at all"),
        Err(Error::Decode(_))
    ));
    let png = encode(&sample_image(), image::ImageFormat::Png);
    assert!(matches!(decode_image(&png[..20]), Err(Error::Decode(_))));
    let mut corrupted = png.clone();
    let mid = corrupted.len() / 2;
    for b in &mut corrupted[mid..] {
        *b = 0xFF;
    }
    assert!(decode_image(&corrupted).is_err());
}

#[test]
fn dimension_limits_are_enforced_without_overflow() {
    assert!(check_dimensions(10_000, 10_000).is_ok());
    assert!(matches!(check_dimensions(0, 10), Err(Error::Decode(_))));
    assert!(matches!(check_dimensions(10, 0), Err(Error::Decode(_))));
    assert!(matches!(
        check_dimensions(20_000, 20_000),
        Err(Error::TooLarge { .. })
    ));
    assert!(matches!(
        check_dimensions(u32::MAX, u32::MAX),
        Err(Error::TooLarge { .. })
    ));
    assert!(check_dimensions(1, 1).is_ok());
}

// ---------- EXIF orientation ----------

#[test]
fn jpeg_orientation_is_read_from_both_byte_orders() {
    assert_eq!(
        jpeg_exif_orientation(&jpeg_with_orientation(true, 6)),
        Some(6)
    );
    assert_eq!(
        jpeg_exif_orientation(&jpeg_with_orientation(false, 8)),
        Some(8)
    );
    for v in 1..=8u16 {
        assert_eq!(
            jpeg_exif_orientation(&jpeg_with_orientation(true, v)),
            Some(v as u8)
        );
    }
}

#[test]
fn invalid_or_missing_orientation_is_ignored() {
    assert_eq!(jpeg_exif_orientation(&jpeg_with_orientation(true, 0)), None);
    assert_eq!(jpeg_exif_orientation(&jpeg_with_orientation(true, 9)), None);
    assert_eq!(
        jpeg_exif_orientation(&encode(&sample_image(), image::ImageFormat::Png)),
        None
    );
    assert_eq!(
        jpeg_exif_orientation(&[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00]),
        None
    );
    assert_eq!(
        jpeg_exif_orientation(&jpeg_with_orientation(true, 6)[..12]),
        None
    );
    assert_eq!(jpeg_exif_orientation(&[]), None);
    assert_eq!(jpeg_exif_orientation(&[0xFF; 64]), None);
}

#[test]
fn orientation_is_found_after_other_segments() {
    let mut jpeg = jpeg_with_orientation(true, 3);
    let app0 = [
        0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F', 0, 1, 1, 0, 0, 1, 0, 1, 0, 0,
    ];
    jpeg.splice(2..2, app0);
    assert_eq!(jpeg_exif_orientation(&jpeg), Some(3));
}

fn marked_image() -> ImageData {
    let (w, h) = (2u32, 3u32);
    let mut rgba = Vec::new();
    for y in 0..h {
        for x in 0..w {
            rgba.extend_from_slice(&[x as u8, y as u8, 0, 255]);
        }
    }
    ImageData {
        width: w,
        height: h,
        rgba,
    }
}

#[test]
fn every_exif_orientation_maps_pixels_correctly() {
    let (w, h) = (2u32, 3u32);
    for orientation in 1..=8u8 {
        let out = apply_exif_orientation(marked_image(), orientation);
        let swapped = (5..=8).contains(&orientation);
        let (ew, eh) = if swapped { (h, w) } else { (w, h) };
        assert_eq!(
            (out.width, out.height),
            (ew, eh),
            "orientation {orientation}"
        );
        assert_eq!(out.rgba.len(), (ew * eh * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let (nx, ny) = match orientation {
                    1 => (x, y),
                    2 => (w - 1 - x, y),
                    3 => (w - 1 - x, h - 1 - y),
                    4 => (x, h - 1 - y),
                    5 => (y, x),
                    6 => (h - 1 - y, x),
                    7 => (h - 1 - y, w - 1 - x),
                    _ => (y, w - 1 - x),
                };
                assert_eq!(
                    out.pixel(nx, ny),
                    [x as u8, y as u8, 0, 255],
                    "orientation {orientation}, source ({x},{y})"
                );
            }
        }
    }
}

#[test]
fn unknown_orientation_values_leave_the_image_unchanged() {
    for value in [0u8, 9, 255] {
        let out = apply_exif_orientation(marked_image(), value);
        assert_eq!(out, marked_image());
    }
}

#[test]
fn decoding_applies_the_jpeg_orientation_tag() {
    let jpeg = encode(&sample_image(), image::ImageFormat::Jpeg);
    let mut rotated = jpeg.clone();
    let exif = jpeg_with_orientation(true, 6);
    let app1 = &exif[2..exif.len() - 2];
    rotated.splice(2..2, app1.iter().copied());

    match decode_image(&rotated).unwrap() {
        Decoded::Still(img) => assert_eq!((img.width, img.height), (4, 5)),
        other => panic!("expected a still image, got {other:?}"),
    }
    match decode_image(&jpeg).unwrap() {
        Decoded::Still(img) => assert_eq!((img.width, img.height), (5, 4)),
        other => panic!("expected a still image, got {other:?}"),
    }
}

// ---------- display scaling ----------

#[test]
fn fit_within_preserves_aspect_ratio_and_never_upscales() {
    assert_eq!(fit_within(100, 50, 25), (25, 13));
    assert_eq!(fit_within(50, 100, 25), (13, 25));
    assert_eq!(fit_within(10, 10, 25), (10, 10));
    assert_eq!(fit_within(25, 25, 25), (25, 25));
    assert_eq!(fit_within(1, 1000, 100), (1, 100));
}

#[test]
fn downscaling_produces_a_buffer_of_the_expected_size() {
    let big = ImageData {
        width: 100,
        height: 50,
        rgba: vec![200; 100 * 50 * 4],
    };
    let small = downscale_to_fit(big, 25);
    assert_eq!((small.width, small.height), (25, 13));
    assert_eq!(small.rgba.len(), 25 * 13 * 4);

    let already_small = marked_image();
    let unchanged = downscale_to_fit(already_small.clone(), MAX_TEXTURE_SIDE);
    assert_eq!(unchanged, already_small);
}
