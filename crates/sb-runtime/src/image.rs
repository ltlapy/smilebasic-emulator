//! Images in files: GRP files as SmileBASIC 4 data or PNG, and JPG files,
//! as pixels in SmileBASIC's `&HAARRGGBB` colors, row by row.

use std::io::Cursor;

use crate::graphics::GRP_SIZE;
use crate::sbfile::{self, DataValues};

/// An image as wide and high as given, row by row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u32>,
}

const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";
const JPEG_MAGIC: &[u8] = &[0xFF, 0xD8];
/// The largest image read: a whole graphic page.
const MAX_SIDE: usize = GRP_SIZE as usize;

/// Reads a GRP, JPG or 2D integer DAT file: PNG or JPEG as it is or in
/// SmileBASIC 4's container, or a `PCBN` array. GRP images store width
/// then height; a DAT array is `[height, width]`.
pub fn decode(bytes: &[u8]) -> Result<Image, String> {
    let image = if bytes.starts_with(PNG_MAGIC) {
        decode_png(bytes)?
    } else if bytes.starts_with(JPEG_MAGIC) {
        decode_jpeg(bytes)?
    } else if sbfile::is_container(bytes)
        && let Ok(file) = sbfile::parse(bytes)
        && (file.body.starts_with(PNG_MAGIC) || file.body.starts_with(JPEG_MAGIC))
    {
        decode(&file.body)?
    } else {
        let (data, image) = sbfile::read_data(bytes).map_err(|e| e.to_string())?;
        match (&data.dims[..], data.values) {
            (&[a, b], DataValues::Int(px)) => {
                let (width, height) = if image { (a, b) } else { (b, a) };
                Image {
                    width,
                    height,
                    pixels: px.into_iter().map(|p| p as u32).collect(),
                }
            }
            _ => return Err("not a 2D integer array".to_string()),
        }
    };
    check_size(&image)?;
    Ok(image)
}

/// Whether an image is whole and of a size the file formats and the graphic
/// pages hold.
fn check_size(image: &Image) -> Result<(), String> {
    let side = 1..=MAX_SIDE;
    if !side.contains(&image.width) || !side.contains(&image.height) {
        return Err(format!("image is {}x{}", image.width, image.height));
    }
    if image.pixels.len() != image.width * image.height {
        let (w, h) = (image.width, image.height);
        return Err(format!("{}x{h} image has {} pixels", w, image.pixels.len()));
    }
    Ok(())
}

fn decode_png(bytes: &[u8]) -> Result<Image, String> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let (width, height) = (reader.info().width, reader.info().height);
    if width as usize > MAX_SIDE || height as usize > MAX_SIDE {
        return Err(format!("image is {width}x{height}"));
    }
    let size = reader.output_buffer_size().ok_or("image too large")?;
    let mut buf = vec![0; size];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let (width, height) = (info.width as usize, info.height as usize);
    let rows = buf.chunks(info.line_size).take(height);
    let argb = |r: u8, g: u8, b: u8, a: u8| u32::from_be_bytes([a, r, g, b]);
    let pixels = match info.color_type {
        png::ColorType::Rgba => rows
            .flat_map(|row| row.chunks_exact(4).take(width))
            .map(|p| argb(p[0], p[1], p[2], p[3]))
            .collect(),
        png::ColorType::Rgb => rows
            .flat_map(|row| row.chunks_exact(3).take(width))
            .map(|p| argb(p[0], p[1], p[2], 255))
            .collect(),
        png::ColorType::GrayscaleAlpha => rows
            .flat_map(|row| row.chunks_exact(2).take(width))
            .map(|p| argb(p[0], p[0], p[0], p[1]))
            .collect(),
        png::ColorType::Grayscale => rows
            .flat_map(|row| row.iter().take(width))
            .map(|&v| argb(v, v, v, 255))
            .collect(),
        png::ColorType::Indexed => return Err("indexed PNG".to_string()),
    };
    Ok(Image {
        width,
        height,
        pixels,
    })
}

fn decode_jpeg(bytes: &[u8]) -> Result<Image, String> {
    use zune_jpeg::zune_core::colorspace::ColorSpace;
    use zune_jpeg::zune_core::options::DecoderOptions;
    let options = DecoderOptions::default()
        .jpeg_set_out_colorspace(ColorSpace::RGB)
        .set_max_width(MAX_SIDE)
        .set_max_height(MAX_SIDE);
    let mut decoder = zune_jpeg::JpegDecoder::new_with_options(Cursor::new(bytes), options);
    let rgb = decoder.decode().map_err(|e| e.to_string())?;
    let (width, height) = decoder.dimensions().ok_or("no JPEG header")?;
    let pixels = rgb
        .chunks_exact(3)
        .map(|p| u32::from_be_bytes([255, p[0], p[1], p[2]]))
        .collect();
    Ok(Image {
        width,
        height,
        pixels,
    })
}

/// A PNG of the image, keeping its alpha.
pub fn encode_png(image: &Image) -> Result<Vec<u8>, String> {
    check_size(image)?;
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, image.width as u32, image.height as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let data: Vec<u8> = (image.pixels.iter())
        .flat_map(|&p| {
            let [a, r, g, b] = p.to_be_bytes();
            [r, g, b, a]
        })
        .collect();
    let error = |e: png::EncodingError| e.to_string();
    let mut writer = encoder.write_header().map_err(error)?;
    writer.write_image_data(&data).map_err(error)?;
    writer.finish().map_err(error)?;
    Ok(out)
}

/// A JPEG of the image at `quality` (1 to 100); JPEG has no alpha, so
/// transparent pixels lose it.
pub fn encode_jpeg(image: &Image, quality: u8) -> Result<Vec<u8>, String> {
    check_size(image)?;
    let mut out = Vec::new();
    let data: Vec<u8> = (image.pixels.iter())
        .flat_map(|&p| {
            let [_, r, g, b] = p.to_be_bytes();
            [r, g, b]
        })
        .collect();
    jpeg_encoder::Encoder::new(&mut out, quality)
        .encode(
            &data,
            image.width as u16,
            image.height as u16,
            jpeg_encoder::ColorType::Rgb,
        )
        .map_err(|e| e.to_string())?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::FileKind;

    fn sample() -> Image {
        Image {
            width: 3,
            height: 2,
            pixels: vec![
                0xFFFF_0000,
                0x8000_FF00,
                0x0000_00FF,
                0xFFFF_FFFF,
                0xFF00_0000,
                0x4012_3456,
            ],
        }
    }

    #[test]
    fn png_keeps_every_pixel() {
        assert_eq!(decode(&encode_png(&sample()).unwrap()).unwrap(), sample());
    }

    #[test]
    fn jpeg_reads_back_opaque_and_close() {
        let mut image = sample();
        image.pixels = vec![0xFFC0_4020; 6];
        let back = decode(&encode_jpeg(&image, 90).unwrap()).unwrap();
        assert_eq!((back.width, back.height), (3, 2));
        for p in back.pixels {
            let [a, r, g, b] = p.to_be_bytes();
            assert_eq!(a, 255);
            for (got, want) in [(r, 0xC0), (g, 0x40), (b, 0x20)] {
                assert!(got.abs_diff(want) < 8, "{p:08X}");
            }
        }
    }

    #[test]
    fn rejects_other_bytes() {
        assert!(decode(b"hello").is_err());
        assert!(decode(PNG_MAGIC).is_err());
    }

    fn array(dims: [u32; 2], pixels: u32) -> Vec<u8> {
        let mut body = sbfile::DATA_MAGIC.to_vec();
        body.extend_from_slice(&4u16.to_le_bytes());
        body.extend_from_slice(&2u16.to_le_bytes());
        for d in [dims[0], dims[1], 0, 0] {
            body.extend_from_slice(&d.to_le_bytes());
        }
        body.resize(body.len() + 4 * pixels as usize, 0);
        body
    }

    #[test]
    fn empty_images_are_refused_on_the_way_in_and_out() {
        // What crashed import: a GRP of 0x16 pixels.
        let date = sbfile::Date::at(std::time::UNIX_EPOCH);
        for dims in [[0, 16], [16, 0], [0, 0]] {
            let body = array(dims, 0);
            let file = sbfile::container(sbfile::Kind::Grp, sbfile::usage::GRP, date, &body, true);
            assert!(decode(&body).is_err(), "{dims:?}");
            assert!(decode(&file).is_err(), "{dims:?}");
            assert!(crate::transfer::import_file(FileKind::Grp, &file).is_err());
        }
        for (width, height) in [(0, 16), (16, 0), (0, 0)] {
            let empty = Image {
                width,
                height,
                pixels: Vec::new(),
            };
            assert!(encode_png(&empty).is_err());
            assert!(encode_jpeg(&empty, 60).is_err());
        }
    }

    #[test]
    fn images_whose_pixels_do_not_fill_them_cannot_be_encoded() {
        let mut short = sample();
        short.pixels.pop();
        assert!(encode_png(&short).is_err());
        assert!(encode_jpeg(&short, 60).is_err());
        // Nor those too large to be a page, whose sides JPEG would cut.
        let wide = Image {
            width: 65536 + 1,
            height: 1,
            pixels: vec![0; 65536 + 1],
        };
        assert!(encode_jpeg(&wide, 60).is_err());
    }

    #[test]
    fn a_png_too_large_for_a_page_is_refused_before_its_memory_is_taken() {
        let mut png = encode_png(&sample()).unwrap();
        // IHDR's width and height, then the chunk's checksum.
        png[16..20].copy_from_slice(&4000u32.to_be_bytes());
        png[20..24].copy_from_slice(&4000u32.to_be_bytes());
        let crc = !png[12..29].iter().fold(!0u32, |mut crc, &b| {
            crc ^= u32::from(b);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xEDB8_8320 & (crc & 1).wrapping_neg());
            }
            crc
        });
        png[29..33].copy_from_slice(&crc.to_be_bytes());
        let error = decode(&png).unwrap_err();
        assert_eq!(error, "image is 4000x4000");
    }
}
