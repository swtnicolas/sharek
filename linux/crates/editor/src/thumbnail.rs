use sharek_common::history::{self, MediaKind};
use sharek_common::{image, paths};
use std::collections::HashSet;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::UNIX_EPOCH;

pub struct Thumbnail {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub fn fit(width: u32, height: u32, max: (u32, u32)) -> (u32, u32) {
    if width == 0 || height == 0 {
        return (1, 1);
    }
    let scale = (f64::from(max.0) / f64::from(width)).min(f64::from(max.1) / f64::from(height)).min(1.0);
    (((f64::from(width) * scale).round() as u32).max(1), ((f64::from(height) * scale).round() as u32).max(1))
}

pub fn downscale(rgba: &[u8], width: u32, height: u32, target: (u32, u32)) -> Vec<u8> {
    let (width, height) = (width as usize, height as usize);
    let (target_width, target_height) = (target.0 as usize, target.1 as usize);
    let mut output = vec![0u8; target_width * target_height * 4];
    for ty in 0..target_height {
        let top = ty * height / target_height;
        let bottom = ((ty + 1) * height / target_height).max(top + 1);
        for tx in 0..target_width {
            let left = tx * width / target_width;
            let right = ((tx + 1) * width / target_width).max(left + 1);
            let (mut r, mut g, mut b, mut alpha, mut count) = (0u64, 0u64, 0u64, 0u64, 0u64);
            for y in top..bottom {
                for x in left..right {
                    let offset = (y * width + x) * 4;
                    let a = u64::from(rgba[offset + 3]);
                    r += u64::from(rgba[offset]) * a;
                    g += u64::from(rgba[offset + 1]) * a;
                    b += u64::from(rgba[offset + 2]) * a;
                    alpha += a;
                    count += 1;
                }
            }
            let offset = (ty * target_width + tx) * 4;
            for (channel, sum) in [r, g, b].into_iter().enumerate() {
                if let Some(value) = (sum + alpha / 2).checked_div(alpha) {
                    output[offset + channel] = value as u8;
                }
            }
            output[offset + 3] = ((alpha + count / 2) / count) as u8;
        }
    }
    output
}

fn shrink(width: u32, height: u32, rgba: Vec<u8>, max: (u32, u32)) -> Thumbnail {
    let size = fit(width, height, max);
    let rgba = if size == (width, height) { rgba } else { downscale(&rgba, width, height, size) };
    Thumbnail { width: size.0, height: size.1, rgba }
}

fn first_gif_frame(path: &Path) -> Option<(u32, u32, Vec<u8>)> {
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::RGBA);
    let mut decoder = options.read_info(File::open(path).ok()?).ok()?;
    let (width, height) = (usize::from(decoder.width()), usize::from(decoder.height()));
    let frame = decoder.read_next_frame().ok()??;
    let (left, top, frame_width) = (usize::from(frame.left), usize::from(frame.top), usize::from(frame.width));
    let mut canvas = vec![0u8; width * height * 4];
    for (row, line) in frame.buffer.chunks_exact(frame_width * 4).enumerate() {
        let y = top + row;
        if y >= height {
            break;
        }
        let visible = frame_width.min(width.saturating_sub(left));
        let start = (y * width + left) * 4;
        canvas[start..start + visible * 4].copy_from_slice(&line[..visible * 4]);
    }
    Some((width as u32, height as u32, canvas))
}

fn first_video_frame(path: &Path, max: (u32, u32)) -> Option<image::Decoded> {
    let output = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-frames:v", "1", "-vf", &format!("scale={}:{}:force_original_aspect_ratio=decrease", max.0, max.1), "-f", "image2pipe", "-vcodec", "png", "-"])
        .output()
        .ok()?;
    image::decode(&output.stdout)
}

pub fn generate(path: &Path, max: (u32, u32)) -> Option<Thumbnail> {
    match history::media_kind(path) {
        Some(MediaKind::Gif) => first_gif_frame(path).map(|(width, height, rgba)| shrink(width, height, rgba, max)),
        Some(MediaKind::Video) => first_video_frame(path, max).map(|decoded| shrink(decoded.width, decoded.height, decoded.rgba, max)),
        None => image::read(path).map(|decoded| shrink(decoded.width, decoded.height, decoded.rgba, max)),
    }
}

fn directory() -> PathBuf {
    paths::cache_dir().join("thumbnails")
}

pub fn key(path: &Path, max: (u32, u32)) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_nanos();
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let bytes = path.as_os_str().as_encoded_bytes().iter().copied().chain(modified.to_le_bytes()).chain(meta.len().to_le_bytes()).chain(max.0.to_le_bytes()).chain(max.1.to_le_bytes());
    for byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    Some(format!("{hash:016x}"))
}

fn store(file: &Path, thumbnail: &Thumbnail) {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, thumbnail.width, thumbnail.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast);
    let written = encoder.write_header().and_then(|mut writer| writer.write_image_data(&thumbnail.rgba).and_then(|()| writer.finish()));
    if written.is_ok() {
        let _ = std::fs::create_dir_all(directory());
        let _ = sharek_common::write_atomically(file, &bytes);
    }
}

pub fn load(path: &Path, max: (u32, u32)) -> Option<Thumbnail> {
    let file = directory().join(format!("{}.png", key(path, max)?));
    if let Some(decoded) = std::fs::read(&file).ok().and_then(|bytes| image::decode(&bytes)) {
        return Some(Thumbnail { width: decoded.width, height: decoded.height, rgba: decoded.rgba });
    }
    let thumbnail = generate(path, max)?;
    store(&file, &thumbnail);
    Some(thumbnail)
}

pub fn prune(keep: &HashSet<String>) {
    let Ok(entries) = std::fs::read_dir(directory()) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let stem = path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
        if !keep.contains(&stem) {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Scratch(PathBuf);

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn scratch(name: &str) -> Scratch {
        Scratch(std::env::temp_dir().join(format!("sharek-thumbnail-test-{}-{name}", std::process::id())))
    }

    #[test]
    fn thumbnails_keep_the_aspect_ratio_and_never_grow() {
        assert_eq!(fit(3840, 2160, (160, 108)), (160, 90));
        assert_eq!(fit(1000, 2000, (160, 108)), (54, 108));
        assert_eq!(fit(100, 50, (160, 108)), (100, 50));
    }

    #[test]
    fn downscaling_averages_each_source_block() {
        let mut rgba = Vec::new();
        for _ in 0..2 {
            rgba.extend_from_slice(&[200, 0, 0, 255, 200, 0, 0, 255, 0, 0, 100, 255, 0, 0, 100, 255]);
        }
        assert_eq!(downscale(&rgba, 4, 2, (2, 1)), vec![200, 0, 0, 255, 0, 0, 100, 255]);
    }

    #[test]
    fn transparent_pixels_do_not_darken_the_average() {
        let rgba = [240, 120, 60, 255, 0, 0, 0, 0];
        assert_eq!(downscale(&rgba, 2, 1, (1, 1)), vec![240, 120, 60, 128]);
    }

    #[test]
    fn an_image_file_becomes_a_fitted_thumbnail() {
        let scratch = scratch("wide.png");
        let path = &scratch.0;
        let source = image::RgbImage { width: 320, height: 90, data: [10u8, 200, 30].repeat(320 * 90).into() };
        image::write(&source, 1.0, path).unwrap();
        let thumbnail = generate(path, (160, 108)).unwrap();
        assert_eq!((thumbnail.width, thumbnail.height), (160, 45));
        assert_eq!(&thumbnail.rgba[..4], &[10, 200, 30, 255]);
    }

    #[test]
    fn the_cache_key_changes_when_the_file_changes() {
        let scratch = scratch("changing.bin");
        let path = &scratch.0;
        std::fs::write(path, [1u8; 10]).unwrap();
        let before = key(path, (160, 108)).unwrap();
        std::fs::write(path, [1u8; 20]).unwrap();
        assert_ne!(key(path, (160, 108)).unwrap(), before);
        assert_ne!(key(path, (224, 151)).unwrap(), key(path, (160, 108)).unwrap());
    }
}
