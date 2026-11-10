use crate::mapped::Bytes;
use std::fs::File;
use std::io::{self, BufWriter};
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct RgbImage {
    pub width: u32,
    pub height: u32,
    pub data: Bytes,
}

impl RgbImage {
    pub fn new(width: u32, height: u32) -> RgbImage {
        RgbImage { width, height, data: vec![0; width as usize * height as usize * 3].into() }
    }

    pub fn on_disk(self) -> RgbImage {
        RgbImage { width: self.width, height: self.height, data: self.data.on_disk() }
    }

    pub fn byte_size(&self) -> usize {
        self.data.len()
    }

    pub fn pixel(&self, x: u32, y: u32) -> [u8; 3] {
        let offset = (y as usize * self.width as usize + x as usize) * 3;
        [self.data[offset], self.data[offset + 1], self.data[offset + 2]]
    }

    pub fn crop(&self, x: i64, y: i64, width: i64, height: i64) -> Option<RgbImage> {
        RgbImage::crop_from(&self.data, self.width, self.height, x, y, width, height)
    }

    pub fn crop_from(data: &[u8], source_width: u32, source_height: u32, x: i64, y: i64, width: i64, height: i64) -> Option<RgbImage> {
        let left = x.clamp(0, source_width as i64);
        let top = y.clamp(0, source_height as i64);
        let right = (x + width).clamp(0, source_width as i64);
        let bottom = (y + height).clamp(0, source_height as i64);
        if right <= left || bottom <= top {
            return None;
        }
        let (width, height) = ((right - left) as usize, (bottom - top) as usize);
        let mut cropped = Vec::with_capacity(width * height * 3);
        let stride = source_width as usize * 3;
        for row in top as usize..bottom as usize {
            let start = row * stride + left as usize * 3;
            cropped.extend_from_slice(&data[start..start + width * 3]);
        }
        Some(RgbImage { width: width as u32, height: height as u32, data: cropped.into() })
    }

    pub fn cut_out(&self, lower: i64, upper: i64, columns: bool) -> Option<RgbImage> {
        let limit = if columns { self.width } else { self.height } as i64;
        let (lower, upper) = (lower.max(0), upper.min(limit));
        if upper <= lower || upper - lower >= limit {
            return None;
        }
        let (width, height) = (self.width as usize, self.height as usize);
        let removed = (upper - lower) as usize;
        let (lower, upper) = (lower as usize, upper as usize);
        if columns {
            let mut data = Vec::with_capacity((width - removed) * height * 3);
            for row in self.data.chunks_exact(width * 3) {
                data.extend_from_slice(&row[..lower * 3]);
                data.extend_from_slice(&row[upper * 3..]);
            }
            Some(RgbImage { width: (width - removed) as u32, height: self.height, data: data.into() })
        } else {
            let stride = width * 3;
            let mut data = Vec::with_capacity(width * (height - removed) * 3);
            data.extend_from_slice(&self.data[..lower * stride]);
            data.extend_from_slice(&self.data[upper * stride..]);
            Some(RgbImage { width: self.width, height: (height - removed) as u32, data: data.into() })
        }
    }

    pub fn rotated(&self, clockwise: bool) -> RgbImage {
        let (width, height) = (self.width as usize, self.height as usize);
        let mut data = vec![0; self.data.len()];
        for y in 0..height {
            for x in 0..width {
                let (nx, ny) = if clockwise { (height - 1 - y, x) } else { (y, width - 1 - x) };
                let target = (ny * height + nx) * 3;
                let source = (y * width + x) * 3;
                data[target..target + 3].copy_from_slice(&self.data[source..source + 3]);
            }
        }
        RgbImage { width: self.height, height: self.width, data: data.into() }
    }

    pub fn flipped(&self, horizontal: bool) -> RgbImage {
        let (width, height) = (self.width as usize, self.height as usize);
        let stride = width * 3;
        let mut data = Vec::with_capacity(self.data.len());
        for y in 0..height {
            let row = if horizontal { y } else { height - 1 - y };
            let line = &self.data[row * stride..(row + 1) * stride];
            if horizontal {
                for pixel in line.as_chunks::<3>().0.iter().rev() {
                    data.extend_from_slice(pixel);
                }
            } else {
                data.extend_from_slice(line);
            }
        }
        RgbImage { width: self.width, height: self.height, data: data.into() }
    }

    pub fn from_rgba_over(width: u32, height: u32, rgba: &[u8], background: [u8; 3]) -> RgbImage {
        let mut data = Vec::with_capacity(width as usize * height as usize * 3);
        for pixel in rgba.as_chunks::<4>().0 {
            let alpha = u32::from(pixel[3]);
            for channel in 0..3 {
                let value = (u32::from(pixel[channel]) * alpha + u32::from(background[channel]) * (255 - alpha) + 127) / 255;
                data.push(value as u8);
            }
        }
        RgbImage { width, height, data: data.into() }
    }
}

pub struct Decoded {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub scale: f64,
}

fn scale_from_dpi(dpi: f64) -> f64 {
    if dpi > 0.0 { ((dpi / 72.0) * 100.0).round().max(100.0) / 100.0 } else { 1.0 }
}

fn dpi(scale: f64) -> f64 {
    72.0 * scale
}

pub fn encode_png(image: &RgbImage, scale: f64, compression: png::Compression) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(image.byte_size() / 3);
    write_png(&mut bytes, image, scale, compression)?;
    Ok(bytes)
}

fn write_png<W: io::Write>(writer: W, image: &RgbImage, scale: f64, compression: png::Compression) -> io::Result<()> {
    let mut encoder = png::Encoder::new(writer, image.width, image.height);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(compression);
    let pixels_per_meter = (dpi(scale) / 0.0254).round() as u32;
    encoder.set_pixel_dims(Some(png::PixelDimensions { xppu: pixels_per_meter, yppu: pixels_per_meter, unit: png::Unit::Meter }));
    let mut writer = encoder.write_header().map_err(io::Error::other)?;
    writer.write_image_data(&image.data).map_err(io::Error::other)?;
    writer.finish().map_err(io::Error::other)
}

pub fn is_jpeg(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("jpg") || e.eq_ignore_ascii_case("jpeg"))
}

pub fn write(image: &RgbImage, scale: f64, path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if is_jpeg(path) {
        let mut encoder = jpeg_encoder::Encoder::new_file(path, 90).map_err(io::Error::other)?;
        encoder.set_density(jpeg_encoder::PixelDensity::dpi(dpi(scale).round() as u16));
        encoder
            .encode(&image.data, image.width as u16, image.height as u16, jpeg_encoder::ColorType::Rgb)
            .map_err(io::Error::other)
    } else {
        write_png(BufWriter::new(File::create(path)?), image, scale, png::Compression::Balanced)
    }
}

pub fn write_bytes(bytes: &[u8], path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, bytes)
}

pub fn decode(bytes: &[u8]) -> Option<Decoded> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) { decode_png(bytes) } else { decode_jpeg(bytes) }
}

pub fn read(path: &Path) -> Option<Decoded> {
    decode(&std::fs::read(path).ok()?)
}

fn decode_png(bytes: &[u8]) -> Option<Decoded> {
    let mut decoder = png::Decoder::new(io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8() | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().ok()?;
    let scale = reader
        .info()
        .pixel_dims
        .filter(|dims| dims.unit == png::Unit::Meter)
        .map_or(1.0, |dims| scale_from_dpi(f64::from(dims.xppu) * 0.0254));
    let mut buffer = vec![0; reader.output_buffer_size()?];
    let frame = reader.next_frame(&mut buffer).ok()?;
    buffer.truncate(frame.buffer_size());
    let rgba = match frame.color_type {
        png::ColorType::Rgba => buffer,
        png::ColorType::GrayscaleAlpha => buffer.as_chunks::<2>().0.iter().flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        _ => return None,
    };
    Some(Decoded { width: frame.width, height: frame.height, rgba, scale })
}

fn decode_jpeg(bytes: &[u8]) -> Option<Decoded> {
    let options = zune_core::options::DecoderOptions::default().jpeg_set_out_colorspace(zune_core::colorspace::ColorSpace::RGBA);
    let mut decoder = zune_jpeg::JpegDecoder::new_with_options(zune_core::bytestream::ZCursor::new(bytes), options);
    let rgba = decoder.decode().ok()?;
    let info = decoder.info()?;
    let scale = if info.pixel_density == 1 { scale_from_dpi(f64::from(info.x_density)) } else { 1.0 };
    Some(Decoded { width: u32::from(info.width), height: u32::from(info.height), rgba, scale })
}

pub fn unique_path(directory: &Path, seconds: i64, extension: &str) -> PathBuf {
    let stem = format!("ShareK_{}", crate::time::file_stamp(seconds));
    let mut path = directory.join(format!("{stem}.{extension}"));
    let mut index = 2;
    while path.exists() {
        path = directory.join(format!("{stem}_{index}.{extension}"));
        index += 1;
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numbered(width: u32, height: u32) -> RgbImage {
        let data: Vec<u8> = (0..width * height).flat_map(|i| [i as u8, (i >> 8) as u8, 7]).collect();
        RgbImage { width, height, data: data.into() }
    }

    #[test]
    fn png_round_trip_keeps_pixels_and_fractional_scale() {
        let image = numbered(5, 3);
        let decoded = decode(&encode_png(&image, 1.45, png::Compression::Fast).unwrap()).unwrap();
        assert_eq!((decoded.width, decoded.height, decoded.scale), (5, 3, 1.45));
        assert_eq!(RgbImage::from_rgba_over(5, 3, &decoded.rgba, [0, 0, 0]).data, image.data);
    }

    #[test]
    fn crop_clamps_to_the_image() {
        let cropped = numbered(4, 4).crop(2, -1, 5, 2).unwrap();
        assert_eq!((cropped.width, cropped.height), (2, 1));
        assert_eq!(cropped.pixel(0, 0), numbered(4, 4).pixel(2, 0));
        assert!(numbered(4, 4).crop(4, 0, 2, 2).is_none());
    }

    #[test]
    fn cut_out_joins_the_remaining_strips() {
        let image = numbered(4, 3);
        let rows = image.cut_out(1, 2, false).unwrap();
        assert_eq!((rows.width, rows.height), (4, 2));
        assert_eq!(rows.pixel(0, 1), image.pixel(0, 2));
        let columns = image.cut_out(0, 3, true).unwrap();
        assert_eq!((columns.width, columns.pixel(0, 2)), (1, image.pixel(3, 2)));
        assert!(image.cut_out(0, 4, true).is_none());
    }

    #[test]
    fn rotating_moves_the_top_left_pixel_to_the_matching_corner() {
        let image = numbered(3, 2);
        let clockwise = image.rotated(true);
        assert_eq!((clockwise.width, clockwise.height), (2, 3));
        assert_eq!(clockwise.pixel(1, 0), image.pixel(0, 0));
        assert_eq!(clockwise.pixel(0, 2), image.pixel(2, 1));
        let back = image.rotated(false);
        assert_eq!(back.pixel(0, 2), image.pixel(0, 0));
        assert_eq!(clockwise.rotated(false).data, image.data);
    }

    #[test]
    fn flipping_mirrors_rows_or_columns() {
        let image = numbered(3, 2);
        assert_eq!(image.flipped(true).pixel(0, 1), image.pixel(2, 1));
        assert_eq!(image.flipped(false).pixel(2, 0), image.pixel(2, 1));
    }
}
