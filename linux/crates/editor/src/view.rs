use crate::model::Point;
use sharek_common::image::RgbImage;
use std::sync::Arc;
use tiny_skia::Transform;

pub const MIN_ZOOM: f32 = 0.05;
pub const MAX_ZOOM: f32 = 16.0;
pub const BACKGROUND: [u8; 3] = [0x13, 0x15, 0x17];
pub const FRAME: [u8; 3] = [0x3f, 0x44, 0x4b];
pub const MARGIN: f32 = 24.0;

#[derive(Clone, Copy, Debug)]
pub struct View {
    pub zoom: f32,
    pub scroll: (f32, f32),
    pub size: (f32, f32),
    pub device: f32,
}

impl Default for View {
    fn default() -> View {
        View { zoom: 1.0, scroll: (0.0, 0.0), size: (1.0, 1.0), device: 1.0 }
    }
}

impl View {
    fn origin(&self, canvas: (f32, f32)) -> (f32, f32) {
        let content = (canvas.0 * self.zoom, canvas.1 * self.zoom);
        (((self.size.0 - content.0) / 2.0).max(0.0), ((self.size.1 - content.1) / 2.0).max(0.0))
    }

    pub fn clamp(&mut self, canvas: (f32, f32)) {
        let max_x = (canvas.0 * self.zoom - self.size.0).max(0.0);
        let max_y = (canvas.1 * self.zoom - self.size.1).max(0.0);
        self.scroll = (self.scroll.0.clamp(0.0, max_x), self.scroll.1.clamp(0.0, max_y));
    }

    pub fn to_canvas(self, canvas: (f32, f32), x: f32, y: f32) -> Point {
        let origin = self.origin(canvas);
        Point::new((x + self.scroll.0 - origin.0) / self.zoom, (y + self.scroll.1 - origin.1) / self.zoom)
    }

    pub fn to_view(self, canvas: (f32, f32), point: Point) -> (f32, f32) {
        let origin = self.origin(canvas);
        (point.x * self.zoom - self.scroll.0 + origin.0, point.y * self.zoom - self.scroll.1 + origin.1)
    }

    pub fn device_transform(&self, canvas: (f32, f32)) -> Transform {
        let origin = self.origin(canvas);
        let k = self.zoom * self.device;
        Transform::from_row(k, 0.0, 0.0, k, (origin.0 - self.scroll.0) * self.device, (origin.1 - self.scroll.1) * self.device)
    }

    pub fn fit(&mut self, canvas: (f32, f32)) {
        let room = ((self.size.0 - 2.0 * MARGIN).max(1.0), (self.size.1 - 2.0 * MARGIN).max(1.0));
        self.zoom = (room.0 / canvas.0).min(room.1 / canvas.1).clamp(MIN_ZOOM, MAX_ZOOM);
        self.scroll = (0.0, 0.0);
    }

    pub fn fits_at_actual_size(&self, canvas: (f32, f32)) -> bool {
        canvas.0 + 2.0 * MARGIN <= self.size.0 && canvas.1 + 2.0 * MARGIN <= self.size.1
    }

    pub fn zoom_at(&mut self, canvas: (f32, f32), zoom: f32, anchor: (f32, f32)) {
        let point = self.to_canvas(canvas, anchor.0, anchor.1);
        self.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        let origin = self.origin(canvas);
        self.scroll = (point.x * self.zoom + origin.0 - anchor.0, point.y * self.zoom + origin.1 - anchor.1);
        self.clamp(canvas);
    }

    pub fn device_size(&self) -> (u32, u32) {
        ((self.size.0 * self.device).round().max(1.0) as u32, (self.size.1 * self.device).round().max(1.0) as u32)
    }
}

pub struct Mips {
    base: usize,
    levels: Vec<RgbImage>,
}

impl Mips {
    pub fn new() -> Mips {
        Mips { base: 0, levels: Vec::new() }
    }

    fn level<'a>(&'a mut self, base: &'a Arc<RgbImage>, wanted: usize) -> (&'a RgbImage, usize) {
        let pointer = Arc::as_ptr(base) as usize;
        if self.base != pointer {
            self.base = pointer;
            self.levels.clear();
        }
        while self.levels.len() < wanted {
            let source = self.levels.last().unwrap_or(base);
            if source.width < 2 || source.height < 2 {
                break;
            }
            let next = halve(source);
            self.levels.push(next.on_disk());
        }
        let index = wanted.min(self.levels.len());
        (if index == 0 { base } else { &self.levels[index - 1] }, index)
    }
}

fn halve(source: &RgbImage) -> RgbImage {
    let (width, height) = (source.width / 2, source.height / 2);
    let stride = source.width as usize * 3;
    let mut data = Vec::with_capacity(width as usize * height as usize * 3);
    for y in 0..height as usize {
        let top = 2 * y * stride;
        let bottom = top + stride;
        for x in 0..width as usize {
            let left = 6 * x;
            for c in 0..3 {
                let sum = u16::from(source.data[top + left + c])
                    + u16::from(source.data[top + left + 3 + c])
                    + u16::from(source.data[bottom + left + c])
                    + u16::from(source.data[bottom + left + 3 + c]);
                data.push(((sum + 2) / 4) as u8);
            }
        }
    }
    RgbImage { width, height, data: data.into() }
}

struct Axis {
    first: usize,
    second: usize,
    weight: u16,
    inside: bool,
    edge: bool,
}

fn axis(count: u32, k: f32, t: f32, scale: f32, level_size: u32, nearest: bool) -> Vec<Axis> {
    (0..count)
        .map(|d| {
            let position = ((d as f32 + 0.5 - t) / k) * scale;
            let inside = position >= 0.0 && position < level_size as f32 && level_size > 0;
            let step = scale / k;
            let edge = !inside && position >= -step && position < level_size as f32 + step;
            if nearest {
                let index = (position.floor().max(0.0) as usize).min(level_size.saturating_sub(1) as usize);
                return Axis { first: index, second: index, weight: 0, inside, edge };
            }
            let sample = position - 0.5;
            let floor = sample.floor();
            let fraction = ((sample - floor) * 256.0) as u16;
            let last = level_size.saturating_sub(1) as i64;
            let first = (floor as i64).clamp(0, last) as usize;
            let second = (floor as i64 + 1).clamp(0, last) as usize;
            Axis { first, second, weight: fraction, inside, edge }
        })
        .collect()
}

pub fn render_base(base: &Arc<RgbImage>, scale: f32, mips: &mut Mips, transform: Transform, target: &mut [u8], width: u32, height: u32) {
    let k = transform.sx;
    let ratio = scale / k;
    let wanted = if ratio > 1.0 { ratio.log2().floor() as usize } else { 0 };
    let (level, index) = mips.level(base, wanted);
    let level_scale = scale / (1u32 << index) as f32;
    let nearest = ratio < 1.0 / 3.0;
    let columns = axis(width, k, transform.tx, level_scale, level.width, nearest);
    let rows = axis(height, k, transform.ty, level_scale, level.height, nearest);
    let stride = level.width as usize * 3;
    let data = &level.data;
    for (y, row) in rows.iter().enumerate() {
        let line = &mut target[y * width as usize * 4..(y + 1) * width as usize * 4];
        if !row.inside {
            for (pixel, column) in line.as_chunks_mut::<4>().0.iter_mut().zip(&columns) {
                let color = if row.edge && (column.inside || column.edge) { FRAME } else { BACKGROUND };
                pixel.copy_from_slice(&[color[0], color[1], color[2], 255]);
            }
            continue;
        }
        let (top, bottom) = (row.first * stride, row.second * stride);
        let wy = u32::from(row.weight);
        for (pixel, column) in line.as_chunks_mut::<4>().0.iter_mut().zip(&columns) {
            if !column.inside {
                let color = if column.edge { FRAME } else { BACKGROUND };
                pixel.copy_from_slice(&[color[0], color[1], color[2], 255]);
                continue;
            }
            let (left, right) = (column.first * 3, column.second * 3);
            let wx = u32::from(column.weight);
            for c in 0..3 {
                let a = u32::from(data[top + left + c]);
                let b = u32::from(data[top + right + c]);
                let d = u32::from(data[bottom + left + c]);
                let e = u32::from(data[bottom + right + c]);
                let upper = a * (256 - wx) + b * wx;
                let lower = d * (256 - wx) + e * wx;
                pixel[c] = ((upper * (256 - wy) + lower * wy + 32768) >> 16) as u8;
            }
            pixel[3] = 255;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_and_canvas_coordinates_invert_each_other_while_zoomed() {
        let canvas = (800.0, 600.0);
        let mut view = View { zoom: 2.0, scroll: (100.0, 50.0), size: (400.0, 300.0), device: 1.5 };
        view.clamp(canvas);
        let point = view.to_canvas(canvas, 120.0, 80.0);
        let (x, y) = view.to_view(canvas, point);
        assert!((x - 120.0).abs() < 1e-3 && (y - 80.0).abs() < 1e-3);
    }

    #[test]
    fn a_small_canvas_is_centered_in_the_viewport() {
        let view = View { zoom: 1.0, scroll: (0.0, 0.0), size: (400.0, 300.0), device: 1.0 };
        assert_eq!(view.to_canvas((200.0, 100.0), 100.0, 100.0), Point::new(0.0, 0.0));
    }

    #[test]
    fn zooming_keeps_the_anchor_point_under_the_pointer() {
        let canvas = (2000.0, 2000.0);
        let mut view = View { zoom: 1.0, scroll: (300.0, 300.0), size: (500.0, 500.0), device: 1.0 };
        let before = view.to_canvas(canvas, 250.0, 100.0);
        view.zoom_at(canvas, 2.0, (250.0, 100.0));
        let after = view.to_canvas(canvas, 250.0, 100.0);
        assert!((before.x - after.x).abs() < 1e-3 && (before.y - after.y).abs() < 1e-3);
    }

    #[test]
    fn actual_size_maps_image_pixels_one_to_one() {
        let base = Arc::new(RgbImage { width: 4, height: 1, data: vec![10, 0, 0, 20, 0, 0, 30, 0, 0, 40, 0, 0].into() });
        let mut target = vec![0u8; 4 * 4];
        render_base(&base, 1.0, &mut Mips::new(), Transform::identity(), &mut target, 4, 1);
        let reds: Vec<u8> = target.as_chunks::<4>().0.iter().map(|pixel| pixel[0]).collect();
        assert_eq!(reds, vec![10, 20, 30, 40]);
    }

    #[test]
    fn the_image_is_framed_by_one_device_pixel() {
        let base = Arc::new(RgbImage { width: 2, height: 1, data: vec![200, 0, 0, 200, 0, 0].into() });
        let mut target = vec![0u8; 5 * 3 * 4];
        render_base(&base, 1.0, &mut Mips::new(), Transform::from_translate(1.0, 1.0), &mut target, 5, 3);
        let (f, b, i) = (FRAME[0], BACKGROUND[0], 200);
        let reds: Vec<u8> = target.as_chunks::<4>().0.iter().map(|pixel| pixel[0]).collect();
        assert_eq!(reds, vec![f, f, f, f, b, f, i, i, f, b, f, f, f, f, b]);
    }
}
