//! Image decoding: stills, animations, EXIF orientation and display scaling.

use std::io::Cursor;
use std::time::Duration;

use image::codecs::gif::GifDecoder;
use image::codecs::webp::WebPDecoder;
use image::{AnimationDecoder, ImageFormat};

use crate::constants::{FRAME_DELAY_DEFAULT_MS, FRAME_DELAY_MIN_MS, MAX_IMAGE_PIXELS};
use crate::error::{Error, Result};

/// A decoded RGBA8 image (non-premultiplied, row-major).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageData {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height * 4` bytes of RGBA data.
    pub rgba: Vec<u8>,
}

impl ImageData {
    /// The RGBA value at (`x`, `y`), or transparent black when out of range.
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let index = (y as usize * self.width as usize + x as usize) * 4;
        self.rgba
            .get(index..index + 4)
            .map(|p| [p[0], p[1], p[2], p[3]])
            .unwrap_or([0; 4])
    }
}

/// One frame of an animation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// The frame's pixels (already composed onto the full canvas).
    pub image: ImageData,
    /// How long to show the frame.
    pub delay: Duration,
}

/// The result of decoding an image file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decoded {
    /// A single still image.
    Still(ImageData),
    /// An animation with at least two frames.
    Animated(Vec<Frame>),
}

fn decode_err(error: image::ImageError) -> Error {
    Error::Decode(error.to_string())
}

/// Rejects empty images and images with more pixels than the configured limit.
pub fn check_dimensions(width: u32, height: u32) -> Result<()> {
    if width == 0 || height == 0 {
        return Err(Error::Decode("the image has no pixels".to_string()));
    }
    if u64::from(width) * u64::from(height) > MAX_IMAGE_PIXELS {
        return Err(Error::TooLarge {
            limit: MAX_IMAGE_PIXELS,
        });
    }
    Ok(())
}

/// Replaces missing or tiny animation delays with the default delay.
pub fn normalize_frame_delay(delay: Duration) -> Duration {
    if delay < Duration::from_millis(FRAME_DELAY_MIN_MS) {
        Duration::from_millis(FRAME_DELAY_DEFAULT_MS)
    } else {
        delay
    }
}

fn webp_is_animated(bytes: &[u8]) -> bool {
    bytes.len() > 20
        && &bytes[0..4] == b"RIFF"
        && &bytes[8..12] == b"WEBP"
        && &bytes[12..16] == b"VP8X"
        && bytes[20] & 0x02 != 0
}

fn collect_frames(frames: image::Frames<'_>) -> Result<Decoded> {
    let mut out: Vec<Frame> = Vec::new();
    let mut pixels: u64 = 0;
    for frame in frames {
        let frame = frame.map_err(decode_err)?;
        let delay = normalize_frame_delay(Duration::from(frame.delay()));
        let buffer = frame.into_buffer();
        let (width, height) = buffer.dimensions();
        pixels += u64::from(width) * u64::from(height);
        if pixels > MAX_IMAGE_PIXELS {
            return Err(Error::TooLarge {
                limit: MAX_IMAGE_PIXELS,
            });
        }
        out.push(Frame {
            image: ImageData {
                width,
                height,
                rgba: buffer.into_raw(),
            },
            delay,
        });
    }
    match out.len() {
        0 => Err(Error::Decode("the image contains no frames".to_string())),
        1 => out
            .pop()
            .map(|f| Decoded::Still(f.image))
            .ok_or_else(|| Error::Decode("the image contains no frames".to_string())),
        _ => Ok(Decoded::Animated(out)),
    }
}

/// Decodes an image from memory, applying JPEG EXIF orientation.
///
/// Dimensions are checked before pixel data is decoded.
pub fn decode_image(bytes: &[u8]) -> Result<Decoded> {
    let format = image::guess_format(bytes).map_err(decode_err)?;
    let (width, height) = image::ImageReader::with_format(Cursor::new(bytes), format)
        .into_dimensions()
        .map_err(decode_err)?;
    check_dimensions(width, height)?;

    match format {
        ImageFormat::Gif => {
            let decoder = GifDecoder::new(Cursor::new(bytes)).map_err(decode_err)?;
            collect_frames(decoder.into_frames())
        }
        ImageFormat::WebP if webp_is_animated(bytes) => {
            let decoder = WebPDecoder::new(Cursor::new(bytes)).map_err(decode_err)?;
            collect_frames(decoder.into_frames())
        }
        _ => {
            let decoded = image::load_from_memory_with_format(bytes, format).map_err(decode_err)?;
            let rgba = decoded.into_rgba8();
            let (width, height) = rgba.dimensions();
            let mut data = ImageData {
                width,
                height,
                rgba: rgba.into_raw(),
            };
            if format == ImageFormat::Jpeg {
                if let Some(orientation) = jpeg_exif_orientation(bytes) {
                    data = apply_exif_orientation(data, orientation);
                }
            }
            Ok(Decoded::Still(data))
        }
    }
}

/// Reads the EXIF orientation (1 to 8) from a JPEG, if present and valid.
pub fn jpeg_exif_orientation(bytes: &[u8]) -> Option<u8> {
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return None;
    }
    let mut pos = 2usize;
    loop {
        while bytes.get(pos) == Some(&0xFF) && bytes.get(pos + 1) == Some(&0xFF) {
            pos += 1;
        }
        if bytes.get(pos) != Some(&0xFF) {
            return None;
        }
        let marker = *bytes.get(pos + 1)?;
        if marker == 0xD9 || marker == 0xDA {
            return None;
        }
        if marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            pos += 2;
            continue;
        }
        let length = usize::from(u16::from_be_bytes([
            *bytes.get(pos + 2)?,
            *bytes.get(pos + 3)?,
        ]));
        if length < 2 || pos + 2 + length > bytes.len() {
            return None;
        }
        let data = &bytes[pos + 4..pos + 2 + length];
        if marker == 0xE1 && data.starts_with(b"Exif\0\0") {
            return tiff_orientation(&data[6..]);
        }
        pos += 2 + length;
    }
}

fn tiff_orientation(tiff: &[u8]) -> Option<u8> {
    let big_endian = match tiff.get(0..2)? {
        b"MM" => true,
        b"II" => false,
        _ => return None,
    };
    let u16_at = |offset: usize| -> Option<u16> {
        let raw = [*tiff.get(offset)?, *tiff.get(offset + 1)?];
        Some(if big_endian {
            u16::from_be_bytes(raw)
        } else {
            u16::from_le_bytes(raw)
        })
    };
    let u32_at = |offset: usize| -> Option<u32> {
        let raw = [
            *tiff.get(offset)?,
            *tiff.get(offset + 1)?,
            *tiff.get(offset + 2)?,
            *tiff.get(offset + 3)?,
        ];
        Some(if big_endian {
            u32::from_be_bytes(raw)
        } else {
            u32::from_le_bytes(raw)
        })
    };
    if u16_at(2)? != 42 {
        return None;
    }
    let ifd = usize::try_from(u32_at(4)?).ok()?;
    let count = usize::from(u16_at(ifd)?);
    for index in 0..count {
        let entry = ifd + 2 + index * 12;
        if u16_at(entry)? == 0x0112 {
            let value = u16_at(entry + 8)?;
            return u8::try_from(value).ok().filter(|v| (1..=8).contains(v));
        }
    }
    None
}

/// Rotates/mirrors an image according to an EXIF orientation value.
///
/// Values outside 2 to 8 (including the identity value 1) return the image unchanged.
pub fn apply_exif_orientation(data: ImageData, orientation: u8) -> ImageData {
    if !(2..=8).contains(&orientation) {
        return data;
    }
    let (w, h) = (data.width as usize, data.height as usize);
    if data.rgba.len() != w * h * 4 {
        return data;
    }
    let (nw, nh) = if orientation >= 5 { (h, w) } else { (w, h) };
    let mut out = vec![0u8; nw * nh * 4];
    for y in 0..h {
        for x in 0..w {
            let (nx, ny) = match orientation {
                2 => (w - 1 - x, y),
                3 => (w - 1 - x, h - 1 - y),
                4 => (x, h - 1 - y),
                5 => (y, x),
                6 => (h - 1 - y, x),
                7 => (h - 1 - y, w - 1 - x),
                _ => (y, w - 1 - x),
            };
            let src = (y * w + x) * 4;
            let dst = (ny * nw + nx) * 4;
            out[dst..dst + 4].copy_from_slice(&data.rgba[src..src + 4]);
        }
    }
    ImageData {
        width: nw as u32,
        height: nh as u32,
        rgba: out,
    }
}

/// Size an image must be scaled to so that neither side exceeds `max_side`,
/// preserving the aspect ratio. Never upscales.
pub fn fit_within(width: u32, height: u32, max_side: u32) -> (u32, u32) {
    if width <= max_side && height <= max_side {
        return (width, height);
    }
    let (w, h, m) = (u64::from(width), u64::from(height), u64::from(max_side));
    let scaled = |side: u64, longest: u64| -> u32 {
        u32::try_from(((side * m + longest / 2) / longest).max(1)).unwrap_or(max_side)
    };
    if width >= height {
        (max_side, scaled(h, w))
    } else {
        (scaled(w, h), max_side)
    }
}

/// Downscales an image so that neither side exceeds `max_side`.
pub fn downscale_to_fit(data: ImageData, max_side: u32) -> ImageData {
    let (new_width, new_height) = fit_within(data.width, data.height, max_side);
    if (new_width, new_height) == (data.width, data.height) {
        return data;
    }
    match image::RgbaImage::from_raw(data.width, data.height, data.rgba) {
        Some(buffer) => {
            let resized = image::imageops::resize(
                &buffer,
                new_width,
                new_height,
                image::imageops::FilterType::Triangle,
            );
            ImageData {
                width: new_width,
                height: new_height,
                rgba: resized.into_raw(),
            }
        }
        None => ImageData {
            width: 1,
            height: 1,
            rgba: vec![0; 4],
        },
    }
}

#[cfg(test)]
mod tests;
