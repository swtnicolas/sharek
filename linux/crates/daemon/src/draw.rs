use sharek_common::text::{Align, Font};
use tiny_skia::{BlendMode, Color, FillRule, Paint, Path, PathBuilder, PixmapMut, Rect, Stroke, Transform};

pub fn bgra(r: u8, g: u8, b: u8, a: u8) -> Color {
    Color::from_rgba8(b, g, r, a)
}

pub fn paint(color: Color) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = true;
    paint
}

pub fn rounded_rect(x: f32, y: f32, width: f32, height: f32, radius: f32) -> Option<Path> {
    let radius = radius.min(width / 2.0).min(height / 2.0);
    let k = radius * 0.447_715;
    let (right, bottom) = (x + width, y + height);
    let mut builder = PathBuilder::new();
    builder.move_to(x + radius, y);
    builder.line_to(right - radius, y);
    builder.cubic_to(right - k, y, right, y + k, right, y + radius);
    builder.line_to(right, bottom - radius);
    builder.cubic_to(right, bottom - k, right - k, bottom, right - radius, bottom);
    builder.line_to(x + radius, bottom);
    builder.cubic_to(x + k, bottom, x, bottom - k, x, bottom - radius);
    builder.line_to(x, y + radius);
    builder.cubic_to(x, y + k, x + k, y, x + radius, y);
    builder.close();
    builder.finish()
}

pub struct Label {
    pub size: (u32, u32),
    text: Option<Path>,
    radius: f32,
    padding: (f32, f32),
}

pub fn label(font: &Font, text: &str, point_size: f64, radius: f64, scale: f64) -> Label {
    let layout = font.layout(text, (point_size * scale) as f32, Align::Center);
    let padding = ((2.0 * scale) as f32, (2.0 * scale) as f32);
    let width = (layout.width + 2.0 * padding.0).ceil().max(1.0) as u32;
    let height = (layout.height + 2.0 * padding.1).ceil().max(1.0) as u32;
    Label { size: (width, height), text: layout.path, radius: (radius * scale) as f32, padding }
}

impl Label {
    pub fn paint(&self, pixmap: &mut PixmapMut) {
        let (width, height) = (self.size.0 as f32, self.size.1 as f32);
        if let Some(background) = rounded_rect(0.0, 0.0, width, height, self.radius) {
            pixmap.fill_path(&background, &paint(bgra(0, 0, 0, 191)), FillRule::Winding, Transform::identity(), None);
        }
        if let Some(text) = &self.text {
            pixmap.fill_path(text, &paint(bgra(255, 255, 255, 255)), FillRule::Winding, Transform::from_translate(self.padding.0, self.padding.1), None);
        }
    }
}

pub fn magnifier(pixmap: &mut PixmapMut, side: u32, scale: f64, sample: impl Fn(i64, i64) -> [u8; 3], pixels: i64) {
    let side_px = side as usize;
    let data = pixmap.data_mut();
    for y in 0..side_px {
        let source_y = (y as i64 * pixels) / side as i64;
        for x in 0..side_px {
            let source_x = (x as i64 * pixels) / side as i64;
            let [r, g, b] = sample(source_x, source_y);
            let offset = (y * side_px + x) * 4;
            data[offset..offset + 4].copy_from_slice(&[b, g, r, 255]);
        }
    }
    let side = side as f32;
    let radius = (6.0 * scale) as f32;
    let Some(rounded) = rounded_rect(0.0, 0.0, side, side, radius) else { return };
    let mut corners = PathBuilder::new();
    corners.push_rect(Rect::from_xywh(0.0, 0.0, side, side).unwrap());
    let mut corners_path = corners.finish();
    if let Some(path) = corners_path.take() {
        let mut builder = PathBuilder::new();
        builder.push_path(&path);
        builder.push_path(&rounded);
        if let Some(mask) = builder.finish() {
            let mut clear = paint(Color::TRANSPARENT);
            clear.blend_mode = BlendMode::Clear;
            pixmap.fill_path(&mask, &clear, FillRule::EvenOdd, Transform::identity(), None);
        }
    }
    let border = (1.5 * scale) as f32;
    if let Some(inset) = rounded_rect(border / 2.0, border / 2.0, side - border, side - border, radius - border / 2.0) {
        pixmap.stroke_path(&inset, &paint(bgra(255, 255, 255, 255)), &Stroke { width: border, ..Default::default() }, Transform::identity(), None);
    }
    let cell = side / pixels as f32;
    let start = (pixels / 2) as f32 * cell;
    if let Some(center) = Rect::from_xywh(start, start, cell, cell) {
        let path = PathBuilder::from_rect(center);
        pixmap.stroke_path(&path, &paint(bgra(255, 59, 48, 255)), &Stroke { width: scale as f32, ..Default::default() }, Transform::identity(), None);
    }
}
