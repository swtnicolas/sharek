use crate::geometry;
use crate::model::{EditorState, Frame, Point, Rect, Shape, ShapeKind, ShapeStyle};
use sharek_common::image::RgbImage;
use sharek_common::text::{Align, Font};
use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::ops::Range;
use std::sync::Arc;
use tiny_skia::{
    BlendMode, Color, FillRule, FilterQuality, LineCap, LineJoin, Paint, Path, PathBuilder, Pattern, Pixmap, PixmapMut, PixmapPaint, SpreadMode,
    Stroke, Transform,
};

#[derive(PartialEq)]
struct EffectKey {
    kind: ShapeKind,
    pixels: (i64, i64, i64, i64),
    strength: u32,
    base: usize,
}

struct Effect {
    key: EffectKey,
    pixmap: Pixmap,
}

pub struct Renderer {
    pub font: Font,
    effects: HashMap<u64, Effect>,
}

fn paint_color(color: Color) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = true;
    paint
}

fn stroke_style(width: f32) -> Stroke {
    Stroke { width, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Default::default() }
}

pub fn rgb_to_pixmap(data: &[u8], width: u32, height: u32) -> Option<Pixmap> {
    let mut pixmap = Pixmap::new(width, height)?;
    for (target, source) in pixmap.data_mut().as_chunks_mut::<4>().0.iter_mut().zip(data.as_chunks::<3>().0) {
        target.copy_from_slice(&[source[0], source[1], source[2], 255]);
    }
    Some(pixmap)
}

fn crop_pixels(base: &RgbImage, pixels: (i64, i64, i64, i64)) -> Option<RgbImage> {
    base.crop(pixels.0, pixels.1, pixels.2, pixels.3)
}

fn pixel_rect(rect: &Rect, scale: f32, base: &RgbImage) -> Option<((i64, i64, i64, i64), Rect)> {
    let device = Rect { x: rect.x * scale, y: rect.y * scale, width: rect.width * scale, height: rect.height * scale }.integral();
    let bounds = Rect { x: 0.0, y: 0.0, width: base.width as f32, height: base.height as f32 };
    let clipped = device.intersection(&bounds)?;
    let pixels = (clipped.x as i64, clipped.y as i64, clipped.width as i64, clipped.height as i64);
    let placement = Rect { x: clipped.x / scale, y: clipped.y / scale, width: clipped.width / scale, height: clipped.height / scale };
    Some((pixels, placement))
}

fn box_sizes(sigma: f32) -> [usize; 3] {
    let ideal = (12.0 * sigma * sigma / 3.0 + 1.0).sqrt();
    let mut lower = ideal.floor() as i64;
    if lower % 2 == 0 {
        lower -= 1;
    }
    let upper = lower + 2;
    let m = ((12.0 * sigma * sigma - 3.0 * (lower * lower) as f32 - 12.0 * lower as f32 - 9.0) / (-4.0 * lower as f32 - 4.0)).round() as i64;
    std::array::from_fn(|i| (((if (i as i64) < m { lower } else { upper }) - 1) / 2).max(0) as usize)
}

fn reciprocal(radius: usize) -> u32 {
    (1 << 24) / (2 * radius as u32 + 1) + 1
}

fn box_rows<const C: usize>(source: &[u8], target: &mut [u8], width: usize, radius: usize) {
    let scale = reciprocal(radius);
    let last = width - 1;
    for (input, output) in source.chunks_exact(width * C).zip(target.chunks_exact_mut(width * C)) {
        let mut sums = [0u32; C];
        for k in 0..=2 * radius {
            let at = k.saturating_sub(radius).min(last) * C;
            for c in 0..C {
                sums[c] += u32::from(input[at + c]);
            }
        }
        for x in 0..width {
            let (leaving, entering) = (x.saturating_sub(radius) * C, (x + radius + 1).min(last) * C);
            for c in 0..C {
                output[x * C + c] = ((sums[c] * scale) >> 24) as u8;
                sums[c] = sums[c] + u32::from(input[entering + c]) - u32::from(input[leaving + c]);
            }
        }
    }
}

fn box_columns(source: &[u8], target: &mut [u8], stride: usize, height: usize, radius: usize) {
    let scale = reciprocal(radius);
    let row = |y: usize| &source[y.min(height - 1) * stride..(y.min(height - 1) + 1) * stride];
    let mut sums = vec![0u32; stride];
    for k in 0..=2 * radius {
        for (sum, value) in sums.iter_mut().zip(row(k.saturating_sub(radius))) {
            *sum += u32::from(*value);
        }
    }
    for (y, output) in target.chunks_exact_mut(stride).enumerate() {
        let (leaving, entering) = (row(y.saturating_sub(radius)), row(y + radius + 1));
        for (((value, sum), out), enter) in output.iter_mut().zip(sums.iter_mut()).zip(leaving).zip(entering) {
            *value = ((*sum * scale) >> 24) as u8;
            *sum = *sum + u32::from(*enter) - u32::from(*out);
        }
    }
}

fn blur_channels<const C: usize>(data: &mut [u8], width: usize, height: usize, sigma: f32) {
    if sigma < 0.5 || width == 0 || height == 0 {
        return;
    }
    let mut scratch = vec![0u8; data.len()];
    for radius in box_sizes(sigma) {
        box_rows::<C>(data, &mut scratch, width, radius);
        box_columns(&scratch, data, width * C, height, radius);
    }
}

pub fn blur(image: &mut RgbImage, sigma: f32) {
    let (width, height) = (image.width as usize, image.height as usize);
    blur_channels::<3>(&mut image.data, width, height, sigma);
}

pub fn pixelate(image: &mut RgbImage, block: usize) {
    let block = block.max(1);
    let (width, height) = (image.width as usize, image.height as usize);
    for top in (0..height).step_by(block) {
        for left in (0..width).step_by(block) {
            let (bottom, right) = ((top + block).min(height), (left + block).min(width));
            let mut sums = [0u64; 3];
            for y in top..bottom {
                for x in left..right {
                    let at = (y * width + x) * 3;
                    for (sum, value) in sums.iter_mut().zip(&image.data[at..at + 3]) {
                        *sum += u64::from(*value);
                    }
                }
            }
            let count = ((bottom - top) * (right - left)) as u64;
            let average = sums.map(|sum| (sum / count) as u8);
            for y in top..bottom {
                for x in left..right {
                    let at = (y * width + x) * 3;
                    image.data[at..at + 3].copy_from_slice(&average);
                }
            }
        }
    }
}

fn blurred_coverage(layer: &Pixmap, sigma: f32) -> Vec<u8> {
    let mut alpha: Vec<u8> = layer.data().as_chunks::<4>().0.iter().map(|pixel| pixel[3]).collect();
    blur_channels::<1>(&mut alpha, layer.width() as usize, layer.height() as usize, sigma);
    alpha
}

fn shadow_of(layer: &Pixmap, sigma: f32) -> Option<Pixmap> {
    let alpha = blurred_coverage(layer, sigma);
    let mut shadow = Pixmap::new(layer.width(), layer.height())?;
    for (target, value) in shadow.data_mut().as_chunks_mut::<4>().0.iter_mut().zip(&alpha) {
        target[3] = value / 2;
    }
    Some(shadow)
}

fn darken(pixel: &mut [u8; 4], alpha: u32) {
    let keep = 255 - alpha;
    for channel in pixel.iter_mut().take(3) {
        *channel = ((u32::from(*channel) * keep + 127) / 255) as u8;
    }
    pixel[3] = (alpha + (u32::from(pixel[3]) * keep + 127) / 255) as u8;
}

fn widened(coverage: &[u8], width: usize, reduction: usize) -> Vec<u16> {
    let mut wide = Vec::with_capacity(coverage.len() * reduction);
    for row in coverage.chunks_exact(width) {
        for (x, value) in row.iter().enumerate() {
            let value = u16::from(*value);
            if reduction == 1 {
                wide.push(value * 4);
            } else {
                wide.push(value * 3 + u16::from(row[x.saturating_sub(1)]));
                wide.push(value * 3 + u16::from(row[(x + 1).min(width - 1)]));
            }
        }
    }
    wide
}

fn composite_shadow(target: &mut PixmapMut, coverage: &[u8], size: (usize, usize), reduction: usize, origin: (i32, i32)) {
    let (width, height) = (target.width() as i32, target.height() as i32);
    let columns = size.0 * reduction;
    let wide = widened(coverage, size.0, reduction);
    let first = (-origin.0).max(0) as usize;
    let last = ((width - origin.0).max(0) as usize).min(columns);
    if first >= last {
        return;
    }
    let data = target.data_mut().as_chunks_mut::<4>().0;
    for y in 0..size.1 * reduction {
        let ty = origin.1 + y as i32;
        if ty < 0 || ty >= height {
            continue;
        }
        let source = y / reduction;
        let neighbor = match (reduction, y % 2) {
            (1, _) => source,
            (_, 0) => source.saturating_sub(1),
            _ => (source + 1).min(size.1 - 1),
        };
        let near = &wide[source * columns + first..source * columns + last];
        let far = &wide[neighbor * columns + first..neighbor * columns + last];
        let line = &mut data[ty as usize * width as usize + (origin.0 + first as i32) as usize..][..last - first];
        for ((pixel, a), b) in line.iter_mut().zip(near).zip(far) {
            let weight = u32::from(*a) * 3 + u32::from(*b);
            if weight != 0 {
                darken(pixel, (weight + 16) / 32);
            }
        }
    }
}

pub fn fingerprints(shapes: &[Shape]) -> Vec<u64> {
    let mut step = 0usize;
    shapes
        .iter()
        .map(|shape| {
            let mut hasher = DefaultHasher::new();
            shape.id.hash(&mut hasher);
            shape.kind.hash(&mut hasher);
            if shape.kind == ShapeKind::Step {
                step += 1;
                step.hash(&mut hasher);
            }
            let style = &shape.style;
            for color in [style.stroke, style.fill, style.text] {
                for value in [color.r, color.g, color.b, color.a] {
                    hasher.write_u32(value.to_bits());
                }
            }
            for value in [style.line_width, style.corner_radius, style.font_size, style.strength] {
                hasher.write_u32(value.to_bits());
            }
            style.shadow.hash(&mut hasher);
            for point in [shape.start, shape.end, shape.tail].iter().chain(&shape.points) {
                hasher.write_u32(point.x.to_bits());
                hasher.write_u32(point.y.to_bits());
            }
            shape.text.hash(&mut hasher);
            shape.image.as_ref().map(|image| Arc::as_ptr(image) as usize).hash(&mut hasher);
            hasher.finish()
        })
        .collect()
}

fn draw_pattern(target: &mut PixmapMut, source: &Pixmap, placement: &Rect, quality: FilterQuality, transform: Transform) {
    let Some(rect) = placement.skia() else { return };
    let mapping = Transform::from_row(
        placement.width / source.width() as f32,
        0.0,
        0.0,
        placement.height / source.height() as f32,
        placement.x,
        placement.y,
    );
    let paint = Paint {
        shader: Pattern::new(source.as_ref(), SpreadMode::Pad, quality, 1.0, mapping),
        anti_alias: true,
        ..Default::default()
    };
    target.fill_rect(rect, &paint, transform, None);
}

pub fn framed(base: &RgbImage, scale: f32, frame: &Frame) -> Option<(RgbImage, f32)> {
    let margin = (frame.effective_margin() * scale).round();
    let (width, height) = (base.width as f32 + 2.0 * margin, base.height as f32 + 2.0 * margin);
    let mut canvas = Pixmap::new(width as u32, height as u32)?;
    canvas.fill(frame.background.color());
    let image = Rect { x: margin, y: margin, width: base.width as f32, height: base.height as f32 };
    let outline = geometry::rounded_rect(&image, frame.radius * scale)?;
    if frame.shadow {
        let mut layer = Pixmap::new(canvas.width(), canvas.height())?;
        layer.fill_path(&outline, &paint_color(Color::BLACK), FillRule::Winding, Transform::identity(), None);
        if let Some(shadow) = shadow_of(&layer, 6.0 * scale) {
            canvas.draw_pixmap(0, (3.0 * scale).round() as i32, shadow.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
        }
    }
    let source = rgb_to_pixmap(&base.data, base.width, base.height)?;
    let paint = Paint {
        shader: Pattern::new(source.as_ref(), SpreadMode::Pad, FilterQuality::Nearest, 1.0, Transform::from_translate(margin, margin)),
        anti_alias: true,
        ..Default::default()
    };
    canvas.fill_path(&outline, &paint, FillRule::Winding, Transform::identity(), None);
    if frame.border > 0.0 && frame.border_color.is_visible() {
        let width = frame.border * scale;
        let inner = image.inset(width / 2.0, width / 2.0);
        if let Some(path) = geometry::rounded_rect(&inner, (frame.radius * scale - width / 2.0).max(0.0)) {
            canvas.stroke_path(&path, &paint_color(frame.border_color.color()), &Stroke { width, ..Default::default() }, Transform::identity(), None);
        }
    }
    let data: Vec<u8> = canvas.data().as_chunks::<4>().0.iter().flat_map(|p| [p[0], p[1], p[2]]).collect();
    Some((RgbImage { width: canvas.width(), height: canvas.height(), data: data.into() }, margin / scale))
}

impl Renderer {
    pub fn new(font: Font) -> Renderer {
        Renderer { font, effects: HashMap::new() }
    }

    pub fn draw(&mut self, state: &EditorState, scale: f32, target: &mut PixmapMut, transform: Transform) {
        self.draw_shapes(state, scale, target, transform, 0..state.shapes.len());
        self.forget_removed(state);
    }

    pub fn draw_shapes(&mut self, state: &EditorState, scale: f32, target: &mut PixmapMut, transform: Transform, range: Range<usize>) {
        let canvas = Rect { x: 0.0, y: 0.0, width: state.base.width as f32 / scale, height: state.base.height as f32 / scale };
        let mut step = state.shapes[..range.start].iter().filter(|shape| shape.kind == ShapeKind::Step).count();
        for shape in &state.shapes[range] {
            if shape.kind == ShapeKind::Step {
                step += 1;
            }
            if shape.style.shadow && !matches!(shape.kind, ShapeKind::SmartEraser | ShapeKind::Spotlight) && !shape.kind.is_effect() {
                self.draw_shadowed(shape, step, state, scale, &canvas, target, transform);
            } else {
                self.draw_shape(shape, step, state, scale, &canvas, target, transform);
            }
        }
    }

    pub fn forget_removed(&mut self, state: &EditorState) {
        let alive: HashSet<u64> = state.shapes.iter().map(|shape| shape.id).collect();
        self.effects.retain(|id, _| alive.contains(id));
    }

    pub fn export(&mut self, state: &EditorState, scale: f32) -> RgbImage {
        if state.shapes.is_empty() {
            return (*state.base).clone();
        }
        let base = &state.base;
        let Some(mut pixmap) = rgb_to_pixmap(&base.data, base.width, base.height) else { return (**base).clone() };
        self.draw(state, scale, &mut pixmap.as_mut(), Transform::from_scale(scale, scale));
        let data: Vec<u8> = pixmap.data().as_chunks::<4>().0.iter().flat_map(|p| [p[0], p[1], p[2]]).collect();
        RgbImage { width: base.width, height: base.height, data: data.into() }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_shadowed(&mut self, shape: &Shape, step: usize, state: &EditorState, scale: f32, canvas: &Rect, target: &mut PixmapMut, transform: Transform) {
        let zoom = transform.sx.abs().max(0.01);
        let margin = shape.style.line_width * 2.0 + 16.0;
        let bounds = geometry::bounds(shape).inset(-margin, -margin);
        let device = Rect {
            x: bounds.x * zoom + transform.tx,
            y: bounds.y * zoom + transform.ty,
            width: bounds.width * zoom,
            height: bounds.height * zoom,
        }
        .integral();
        let visible = Rect { x: 0.0, y: 0.0, width: target.width() as f32, height: target.height() as f32 };
        let Some(area) = device.intersection(&visible) else { return };
        let sigma = 2.5 * zoom;
        let reduction = if sigma >= 1.5 { 2 } else { 1 };
        let size = ((area.width as usize).div_ceil(reduction), (area.height as usize).div_ceil(reduction));
        if let Some(mut layer) = Pixmap::new(size.0 as u32, size.1 as u32) {
            let shrink = 1.0 / reduction as f32;
            let local = transform.post_translate(-area.x, -area.y).post_scale(shrink, shrink);
            self.draw_shape(shape, step, state, scale, canvas, &mut layer.as_mut(), local);
            let coverage = blurred_coverage(&layer, sigma * shrink);
            let origin = ((area.x + zoom).round() as i32, (area.y + 2.0 * zoom).round() as i32);
            composite_shadow(target, &coverage, size, reduction, origin);
        }
        self.draw_shape(shape, step, state, scale, canvas, target, transform);
    }

    fn paint(&self, path: &Path, style: &ShapeStyle, target: &mut PixmapMut, transform: Transform) {
        if style.fill.is_visible() {
            target.fill_path(path, &paint_color(style.fill.color()), FillRule::Winding, transform, None);
        }
        if style.line_width > 0.0 && style.stroke.is_visible() {
            target.stroke_path(path, &paint_color(style.stroke.color()), &stroke_style(style.line_width), transform, None);
        }
    }

    fn draw_text(&self, shape: &Shape, target: &mut PixmapMut, transform: Transform) {
        if shape.text.is_empty() {
            return;
        }
        let rect = geometry::text_rect(&self.font, shape);
        let layout = self.font.layout(&shape.text, shape.style.font_size, Align::Left);
        let Some(path) = layout.path else { return };
        let transform = transform.pre_translate(rect.x + 1.0, rect.y);
        if shape.kind == ShapeKind::TextOutline && shape.style.line_width > 0.0 {
            target.stroke_path(&path, &paint_color(shape.style.stroke.color()), &stroke_style(shape.style.line_width * 2.0), transform, None);
        }
        target.fill_path(&path, &paint_color(shape.style.text.color()), FillRule::Winding, transform, None);
    }

    fn effect(&mut self, shape: &Shape, base: &Arc<RgbImage>, scale: f32) -> Option<(&Pixmap, Rect)> {
        let (pixels, placement) = pixel_rect(&shape.rect(), scale, base)?;
        let key = EffectKey { kind: shape.kind, pixels, strength: shape.style.strength.to_bits(), base: Arc::as_ptr(base) as usize };
        if self.effects.get(&shape.id).is_none_or(|effect| effect.key != key) {
            let mut crop = crop_pixels(base, pixels)?;
            match shape.kind {
                ShapeKind::Blur => blur(&mut crop, shape.style.strength * scale / 2.0),
                ShapeKind::Pixelate => pixelate(&mut crop, (shape.style.strength * scale).max(1.0).round() as usize),
                ShapeKind::Magnify => {}
                _ => return None,
            }
            let pixmap = rgb_to_pixmap(&crop.data, crop.width, crop.height)?;
            self.effects.insert(shape.id, Effect { key, pixmap });
        }
        self.effects.get(&shape.id).map(|effect| (&effect.pixmap, placement))
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_shape(&mut self, shape: &Shape, step: usize, state: &EditorState, scale: f32, canvas: &Rect, target: &mut PixmapMut, transform: Transform) {
        let style = &shape.style;
        match shape.kind {
            ShapeKind::Rectangle => {
                if let Some(path) = geometry::rounded_rect(&shape.rect(), style.corner_radius) {
                    self.paint(&path, style, target, transform);
                }
            }
            ShapeKind::Ellipse => {
                if let Some(path) = geometry::ellipse(&shape.rect()) {
                    self.paint(&path, style, target, transform);
                }
            }
            ShapeKind::Freehand | ShapeKind::Line => {
                let points: Vec<Point> = if shape.kind == ShapeKind::Line {
                    vec![shape.start, shape.end]
                } else if shape.points.len() > 1 {
                    shape.points.clone()
                } else {
                    vec![shape.start, Point::new(shape.start.x + 0.01, shape.start.y)]
                };
                let mut builder = PathBuilder::new();
                builder.move_to(points[0].x, points[0].y);
                for point in &points[1..] {
                    builder.line_to(point.x, point.y);
                }
                if let Some(path) = builder.finish() {
                    target.stroke_path(&path, &paint_color(style.stroke.color()), &stroke_style(style.line_width), transform, None);
                }
            }
            ShapeKind::Arrow => {
                let Some((shaft, head)) = geometry::arrow_paths(shape) else { return };
                let paint = paint_color(style.stroke.color());
                target.stroke_path(&shaft, &paint, &stroke_style(style.line_width), transform, None);
                target.fill_path(&head, &paint, FillRule::Winding, transform, None);
                target.stroke_path(&head, &paint, &stroke_style((style.line_width / 3.0).max(1.0)), transform, None);
            }
            ShapeKind::TextOutline => self.draw_text(shape, target, transform),
            ShapeKind::TextBackground => {
                if let Some(path) = geometry::rounded_rect(&shape.rect(), style.corner_radius) {
                    self.paint(&path, style, target, transform);
                }
                self.draw_text(shape, target, transform);
            }
            ShapeKind::SpeechBalloon => {
                let body = geometry::rounded_rect(&shape.rect(), style.corner_radius);
                let tail = geometry::balloon_tail(shape);
                let parts: Vec<&Path> = body.iter().chain(tail.iter()).collect();
                if style.line_width > 0.0 && style.stroke.is_visible() {
                    let width = if style.fill.is_visible() { style.line_width * 2.0 } else { style.line_width };
                    for path in &parts {
                        target.stroke_path(path, &paint_color(style.stroke.color()), &stroke_style(width), transform, None);
                    }
                }
                if style.fill.is_visible() {
                    for path in &parts {
                        target.fill_path(path, &paint_color(style.fill.color()), FillRule::Winding, transform, None);
                    }
                }
                self.draw_text(shape, target, transform);
            }
            ShapeKind::Step => {
                let bounds = geometry::bounds(shape);
                if let Some(path) = geometry::ellipse(&bounds) {
                    self.paint(&path, style, target, transform);
                }
                let label = step.to_string();
                let layout = self.font.layout(&label, style.font_size, Align::Left);
                if let Some(path) = layout.path {
                    let center = bounds.mid();
                    let offset = transform.pre_translate(center.x - layout.width / 2.0, center.y - layout.height / 2.0);
                    target.fill_path(&path, &paint_color(style.text.color()), FillRule::Winding, offset, None);
                }
            }
            ShapeKind::Magnify => {
                let rect = shape.rect();
                let zoom = style.strength.max(1.0);
                if rect.width < 1.0 || rect.height < 1.0 {
                    return;
                }
                let source = Rect {
                    x: rect.mid().x - rect.width / zoom / 2.0,
                    y: rect.mid().y - rect.height / zoom / 2.0,
                    width: rect.width / zoom,
                    height: rect.height / zoom,
                };
                let mut probe = shape.clone();
                probe.start = Point::new(source.x, source.y);
                probe.end = Point::new(source.max_x(), source.max_y());
                let base = state.base.clone();
                let Some((pixmap, placement)) = self.effect(&probe, &base, scale).map(|(p, r)| (p.clone(), r)) else { return };
                let shown = Rect {
                    x: rect.x + (placement.x - source.x) * zoom,
                    y: rect.y + (placement.y - source.y) * zoom,
                    width: placement.width * zoom,
                    height: placement.height * zoom,
                };
                draw_pattern(target, &pixmap, &shown, FilterQuality::Bicubic, transform);
                if style.line_width > 0.0
                    && let Some(rect) = rect.skia() {
                        target.stroke_path(&PathBuilder::from_rect(rect), &paint_color(style.stroke.color()), &stroke_style(style.line_width), transform, None);
                    }
            }
            ShapeKind::SmartEraser => {
                if let Some(rect) = shape.rect().skia() {
                    let mut paint = paint_color(style.fill.color());
                    paint.anti_alias = false;
                    target.fill_rect(rect, &paint, transform, None);
                }
            }
            ShapeKind::Highlight => {
                if let Some(rect) = shape.rect().skia() {
                    let mut paint = paint_color(style.fill.color());
                    paint.blend_mode = BlendMode::Multiply;
                    target.fill_rect(rect, &paint, transform, None);
                }
            }
            ShapeKind::Blur | ShapeKind::Pixelate => {
                let base = state.base.clone();
                let quality = if shape.kind == ShapeKind::Pixelate { FilterQuality::Nearest } else { FilterQuality::Bilinear };
                if let Some((pixmap, placement)) = self.effect(shape, &base, scale) {
                    let pixmap = pixmap.clone();
                    draw_pattern(target, &pixmap, &placement, quality, transform);
                }
            }
            ShapeKind::Spotlight => {
                let mut builder = PathBuilder::new();
                if let Some(rect) = canvas.skia() {
                    builder.push_rect(rect);
                }
                if let Some(oval) = shape.rect().skia() {
                    builder.push_oval(oval);
                }
                if let Some(path) = builder.finish() {
                    let shade = Color::from_rgba(0.0, 0.0, 0.0, style.strength.clamp(0.0, 1.0)).unwrap_or(Color::BLACK);
                    target.fill_path(&path, &paint_color(shade), FillRule::EvenOdd, transform, None);
                }
            }
            ShapeKind::Image => {
                let Some(image) = &shape.image else { return };
                draw_pattern(target, &image.pixmap, &shape.rect(), FilterQuality::Bicubic, transform);
                if style.line_width > 0.0
                    && let Some(rect) = shape.rect().skia() {
                        target.stroke_path(&PathBuilder::from_rect(rect), &paint_color(style.stroke.color()), &stroke_style(style.line_width), transform, None);
                    }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Rgba;

    fn gradient(width: u32, height: u32) -> RgbImage {
        let data: Vec<u8> = (0..width * height).flat_map(|i| [(i % width * 255 / width) as u8, 0, 200]).collect();
        RgbImage { width, height, data: data.into() }
    }

    #[test]
    fn the_shadow_gives_the_values_the_macos_editor_is_tested_against() {
        let mut coverage = vec![0, 0, 0, 0, 0, 0, 0, 255, 255, 255, 0, 0, 0, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0];
        blur_channels::<1>(&mut coverage, 6, 4, 1.0);
        assert_eq!(coverage, [28, 56, 85, 56, 28, 0, 56, 113, 170, 113, 56, 0, 56, 113, 170, 113, 56, 0, 28, 56, 85, 56, 28, 0]);
        let mut target = Pixmap::new(14, 10).unwrap();
        target.fill(Color::from_rgba8(200, 150, 100, 255));
        composite_shadow(&mut target.as_mut(), &coverage, (6, 4), 2, (1, 1));
        let red: Vec<u8> = (0..14).map(|x| target.data()[(3 * 14 + x) * 4]).collect();
        assert_eq!(red, [200, 180, 176, 166, 156, 147, 147, 156, 166, 176, 186, 195, 200, 200]);
        let mut clear = Pixmap::new(14, 10).unwrap();
        composite_shadow(&mut clear.as_mut(), &coverage, (6, 4), 2, (1, 1));
        let alpha: Vec<u8> = (0..14).map(|x| clear.data()[(4 * 14 + x) * 4 + 3]).collect();
        assert_eq!(alpha, [0, 28, 35, 49, 64, 78, 78, 64, 49, 35, 21, 7, 0, 0]);
        let mut full = Pixmap::new(8, 6).unwrap();
        full.fill(Color::from_rgba8(200, 150, 100, 255));
        composite_shadow(&mut full.as_mut(), &coverage, (6, 4), 1, (1, 1));
        let red: Vec<u8> = (0..8).map(|x| full.data()[(2 * 8 + x) * 4]).collect();
        assert_eq!(red, [200, 178, 155, 133, 155, 178, 200, 200]);
        assert_eq!(box_sizes(2.5), [2, 2, 2]);
        assert_eq!(box_sizes(5.0), [4, 4, 5]);
    }

    #[test]
    fn pixelate_fills_each_block_with_its_average() {
        let mut image = gradient(4, 2);
        pixelate(&mut image, 2);
        assert_eq!(image.pixel(0, 0), image.pixel(1, 1));
        assert_eq!(image.pixel(0, 0)[0], (63 / 2) as u8);
        assert_ne!(image.pixel(1, 0), image.pixel(2, 0));
    }

    #[test]
    fn blur_keeps_a_flat_image_flat_and_softens_an_edge() {
        let mut flat = RgbImage { width: 8, height: 8, data: vec![90; 8 * 8 * 3].into() };
        blur(&mut flat, 3.0);
        assert!(flat.data.iter().all(|&value| value == 90));
        let mut edge = RgbImage { width: 10, height: 1, data: (0..10).flat_map(|x| if x < 5 { [0, 0, 0] } else { [255, 255, 255] }).collect::<Vec<u8>>().into() };
        blur(&mut edge, 1.5);
        assert!(edge.pixel(4, 0)[0] > 0 && edge.pixel(5, 0)[0] < 255);
    }

    #[test]
    fn a_frame_surrounds_the_image_with_its_background_and_rounds_the_corners() {
        let base = RgbImage { width: 40, height: 20, data: vec![0; 40 * 20 * 3].into() };
        let frame = Frame { margin: 10.0, background: Rgba::WHITE, radius: 8.0, shadow: false, border: 0.0, border_color: Rgba::BLACK };
        let (framed, offset) = framed(&base, 2.0, &frame).unwrap();
        assert_eq!((framed.width, framed.height, offset), (80, 60, 10.0));
        assert_eq!(framed.pixel(5, 5), [255, 255, 255]);
        assert_eq!(framed.pixel(20, 20), [255, 255, 255]);
        assert_eq!(framed.pixel(40, 30), [0, 0, 0]);
    }

    #[test]
    fn a_frame_with_shadow_keeps_room_for_it_even_without_margin() {
        let base = RgbImage { width: 10, height: 10, data: vec![0; 300].into() };
        let frame = Frame { margin: 0.0, shadow: true, ..Frame::default() };
        let (framed, offset) = framed(&base, 1.0, &frame).unwrap();
        assert_eq!(offset, 16.0);
        assert_eq!(framed.width, 42);
    }

    #[test]
    fn highlight_tints_the_shapes_beneath_it_instead_of_covering_them() {
        let base = RgbImage { width: 10, height: 10, data: vec![255; 300].into() };
        let mut red = Shape::new(ShapeKind::Rectangle, ShapeKind::Rectangle.default_style(), Point::new(0.0, 0.0));
        red.end = Point::new(5.0, 10.0);
        red.style.fill = Rgba::RED;
        red.style.line_width = 0.0;
        red.style.shadow = false;
        let mut highlight = Shape::new(ShapeKind::Highlight, ShapeKind::Highlight.default_style(), Point::new(0.0, 0.0));
        highlight.end = Point::new(10.0, 10.0);
        let state = EditorState { base: Arc::new(base), shapes: vec![red, highlight] };
        let font = Font::bold().expect("font");
        let exported = Renderer::new(font).export(&state, 1.0);
        assert_eq!(exported.pixel(2, 5), [242, 60, 0]);
        assert_eq!(exported.pixel(7, 5), [255, 255, 0]);
    }

    #[test]
    fn drawing_in_two_ranges_matches_drawing_everything_at_once() {
        let base = Arc::new(RgbImage { width: 60, height: 30, data: vec![200; 60 * 30 * 3].into() });
        let step = |x: f32| {
            let mut shape = Shape::new(ShapeKind::Step, ShapeKind::Step.default_style(), Point::new(x, 15.0));
            shape.style.font_size = 20.0;
            shape
        };
        let state = EditorState { base: base.clone(), shapes: vec![step(15.0), step(45.0)] };
        let mut renderer = Renderer::new(Font::bold().expect("font"));
        let mut whole = rgb_to_pixmap(&base.data, 60, 30).unwrap();
        renderer.draw(&state, 1.0, &mut whole.as_mut(), Transform::identity());
        let mut split = rgb_to_pixmap(&base.data, 60, 30).unwrap();
        renderer.draw_shapes(&state, 1.0, &mut split.as_mut(), Transform::identity(), 0..1);
        renderer.draw_shapes(&state, 1.0, &mut split.as_mut(), Transform::identity(), 1..2);
        assert!(whole.data() == split.data());
        let mut alone = rgb_to_pixmap(&base.data, 60, 30).unwrap();
        renderer.draw_shapes(&EditorState { base, shapes: vec![step(45.0)] }, 1.0, &mut alone.as_mut(), Transform::identity(), 0..1);
        assert!(alone.data() != split.data());
    }

    #[test]
    fn a_step_label_change_changes_its_fingerprint_but_not_its_neighbors() {
        let shape = |kind: ShapeKind| Shape::new(kind, kind.default_style(), Point::new(1.0, 1.0));
        let (first, rectangle, second) = (shape(ShapeKind::Step), shape(ShapeKind::Rectangle), shape(ShapeKind::Step));
        let before = fingerprints(&[first, rectangle.clone(), second.clone()]);
        let after = fingerprints(&[rectangle, second]);
        assert_eq!(before[1], after[0]);
        assert_ne!(before[2], after[1]);
    }
}
