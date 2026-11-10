use crate::model::{Point, Rect, Shape, ShapeKind, ShapeStyle};
use sharek_common::text::Font;
use tiny_skia::{Path, PathBuilder};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    Corner(u8),
    Edge(u8),
    Start,
    End,
    Tail,
}

pub fn padding(shape: &Shape) -> (f32, f32) {
    if shape.kind == ShapeKind::TextOutline {
        let inset = shape.style.line_width + 2.0;
        return (inset, inset);
    }
    ((shape.style.font_size * 0.5).round(), (shape.style.font_size * 0.3).round())
}

pub fn text_size(font: &Font, shape: &Shape) -> (f32, f32) {
    let mut text = if shape.text.is_empty() { " ".to_string() } else { shape.text.clone() };
    if text.ends_with('\n') {
        text.push(' ');
    }
    let lines: Vec<&str> = text.split('\n').collect();
    let width = lines.iter().map(|line| font.measure(line, shape.style.font_size)).fold(0.0, f32::max);
    (width.ceil() + 2.0, (font.line_height(shape.style.font_size) * lines.len() as f32).ceil())
}

pub fn fit(font: &Font, shape: &mut Shape) {
    let (pad_x, pad_y) = padding(shape);
    let (width, height) = text_size(font, shape);
    shape.end = Point::new(shape.start.x + width + pad_x * 2.0, shape.start.y + height + pad_y * 2.0);
}

pub fn text_rect(font: &Font, shape: &Shape) -> Rect {
    let (pad_x, pad_y) = padding(shape);
    let (width, height) = text_size(font, shape);
    Rect { x: shape.start.x + pad_x, y: shape.start.y + pad_y, width, height }
}

pub fn step_radius(style: &ShapeStyle) -> f32 {
    (style.font_size * 0.8 + 4.0).round()
}

pub fn bounds(shape: &Shape) -> Rect {
    match shape.kind {
        ShapeKind::Freehand => {
            let Some(first) = shape.points.first() else { return Rect::default() };
            shape.points.iter().fold(Rect { x: first.x, y: first.y, width: 0.0, height: 0.0 }, |rect, point| rect.union_point(*point))
        }
        ShapeKind::Step => {
            let radius = step_radius(&shape.style);
            Rect { x: shape.start.x - radius, y: shape.start.y - radius, width: radius * 2.0, height: radius * 2.0 }
        }
        ShapeKind::SpeechBalloon => shape.rect().union_point(shape.tail),
        _ => shape.rect(),
    }
}

pub fn segment_distance(point: Point, a: Point, b: Point) -> f32 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let length_squared = dx * dx + dy * dy;
    if length_squared <= 0.0 {
        return point.distance(a);
    }
    let t = (((point.x - a.x) * dx + (point.y - a.y) * dy) / length_squared).clamp(0.0, 1.0);
    point.distance(Point::new(a.x + t * dx, a.y + t * dy))
}

pub fn contains(shape: &Shape, point: Point, tolerance: f32) -> bool {
    let reach = shape.style.line_width / 2.0 + tolerance;
    match shape.kind {
        ShapeKind::Line | ShapeKind::Arrow => segment_distance(point, shape.start, shape.end) <= reach,
        ShapeKind::Freehand => {
            if shape.points.len() < 2 {
                return point.distance(shape.start) <= reach;
            }
            shape.points.windows(2).any(|pair| segment_distance(point, pair[0], pair[1]) <= reach)
        }
        ShapeKind::Ellipse => {
            let rect = shape.rect().inset(-tolerance, -tolerance);
            let (rx, ry) = (rect.width / 2.0, rect.height / 2.0);
            if rx <= 0.0 || ry <= 0.0 {
                return false;
            }
            let center = rect.mid();
            ((point.x - center.x) / rx).powi(2) + ((point.y - center.y) / ry).powi(2) <= 1.0
        }
        ShapeKind::Step => point.distance(shape.start) <= step_radius(&shape.style) + tolerance,
        ShapeKind::SpeechBalloon => shape.rect().inset(-tolerance, -tolerance).contains(point) || point.distance(shape.tail) <= tolerance * 2.0,
        _ => shape.rect().inset(-tolerance, -tolerance).contains(point),
    }
}

pub fn handles(shape: &Shape) -> Vec<(Handle, Point)> {
    match shape.kind {
        ShapeKind::Line | ShapeKind::Arrow => vec![(Handle::Start, shape.start), (Handle::End, shape.end)],
        ShapeKind::SpeechBalloon => vec![(Handle::Tail, shape.tail)],
        ShapeKind::Freehand | ShapeKind::Step | ShapeKind::TextOutline | ShapeKind::TextBackground => Vec::new(),
        _ => rect_handles(&shape.rect()),
    }
}

pub fn rect_handles(rect: &Rect) -> Vec<(Handle, Point)> {
    let mid = rect.mid();
    vec![
        (Handle::Corner(0), Point::new(rect.x, rect.y)),
        (Handle::Corner(1), Point::new(rect.max_x(), rect.y)),
        (Handle::Corner(2), Point::new(rect.max_x(), rect.max_y())),
        (Handle::Corner(3), Point::new(rect.x, rect.max_y())),
        (Handle::Edge(0), Point::new(mid.x, rect.y)),
        (Handle::Edge(1), Point::new(rect.max_x(), mid.y)),
        (Handle::Edge(2), Point::new(mid.x, rect.max_y())),
        (Handle::Edge(3), Point::new(rect.x, mid.y)),
    ]
}

pub fn resize(rect: &Rect, handle: Handle, point: Point) -> Rect {
    let (mut min_x, mut max_x, mut min_y, mut max_y) = (rect.x, rect.max_x(), rect.y, rect.max_y());
    match handle {
        Handle::Corner(0) => (min_x, min_y) = (point.x, point.y),
        Handle::Corner(1) => (max_x, min_y) = (point.x, point.y),
        Handle::Corner(2) => (max_x, max_y) = (point.x, point.y),
        Handle::Corner(3) => (min_x, max_y) = (point.x, point.y),
        Handle::Edge(0) => min_y = point.y,
        Handle::Edge(1) => max_x = point.x,
        Handle::Edge(2) => max_y = point.y,
        Handle::Edge(3) => min_x = point.x,
        _ => {}
    }
    Rect { x: min_x.min(max_x), y: min_y.min(max_y), width: (max_x - min_x).abs(), height: (max_y - min_y).abs() }
}

pub fn apply(handle: Handle, point: Point, shape: &mut Shape, constrained: bool) {
    match handle {
        Handle::Start => shape.start = if constrained { snap_angle(point, shape.end) } else { point },
        Handle::End => shape.end = if constrained { snap_angle(point, shape.start) } else { point },
        Handle::Tail => shape.tail = point,
        Handle::Corner(_) | Handle::Edge(_) => {
            let rect = resize(&shape.rect(), handle, point);
            shape.start = Point::new(rect.x, rect.y);
            shape.end = Point::new(rect.max_x(), rect.max_y());
        }
    }
}

pub fn snap_angle(point: Point, origin: Point) -> Point {
    let (dx, dy) = (point.x - origin.x, point.y - origin.y);
    let length = dx.hypot(dy);
    let step = std::f32::consts::FRAC_PI_4;
    let angle = (dy.atan2(dx) / step).round() * step;
    Point::new(origin.x + angle.cos() * length, origin.y + angle.sin() * length)
}

pub fn square(point: Point, origin: Point) -> Point {
    let (dx, dy) = (point.x - origin.x, point.y - origin.y);
    let side = dx.abs().max(dy.abs());
    Point::new(origin.x + if dx < 0.0 { -side } else { side }, origin.y + if dy < 0.0 { -side } else { side })
}

pub fn rounded_rect(rect: &Rect, radius: f32) -> Option<Path> {
    let radius = radius.min(rect.width / 2.0).min(rect.height / 2.0).max(0.0);
    if radius <= 0.0 {
        return rect.skia().map(PathBuilder::from_rect);
    }
    let k = radius * 0.447_715;
    let (x, y, right, bottom) = (rect.x, rect.y, rect.max_x(), rect.max_y());
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

pub fn ellipse(rect: &Rect) -> Option<Path> {
    rect.skia().and_then(PathBuilder::from_oval)
}

pub fn balloon_tail(shape: &Shape) -> Option<Path> {
    let rect = shape.rect();
    if rect.contains(shape.tail) {
        return None;
    }
    let center = rect.mid();
    let (dx, dy) = (shape.tail.x - center.x, shape.tail.y - center.y);
    let length = dx.hypot(dy);
    if length <= 0.0 {
        return None;
    }
    let half_base = (rect.width.min(rect.height) * 0.22).max(6.0);
    let (px, py) = (-dy / length * half_base, dx / length * half_base);
    let mut builder = PathBuilder::new();
    builder.move_to(center.x + px, center.y + py);
    builder.line_to(shape.tail.x, shape.tail.y);
    builder.line_to(center.x - px, center.y - py);
    builder.close();
    builder.finish()
}

pub fn arrow_paths(shape: &Shape) -> Option<(Path, Path)> {
    let (dx, dy) = (shape.end.x - shape.start.x, shape.end.y - shape.start.y);
    let length = dx.hypot(dy);
    if length <= 1.0 {
        return None;
    }
    let (ux, uy) = (dx / length, dy / length);
    let width = shape.style.line_width;
    let head_length = (width * 4.0).max(14.0).min(length);
    let half_head = (width * 2.0).max(7.0);
    let base = Point::new(shape.end.x - ux * head_length, shape.end.y - uy * head_length);
    let mut shaft = PathBuilder::new();
    shaft.move_to(shape.start.x, shape.start.y);
    shaft.line_to(shape.end.x - ux * head_length * 0.6, shape.end.y - uy * head_length * 0.6);
    let mut head = PathBuilder::new();
    head.move_to(shape.end.x, shape.end.y);
    head.line_to(base.x - uy * half_head, base.y + ux * half_head);
    head.line_to(base.x + uy * half_head, base.y - ux * half_head);
    head.close();
    Some((shaft.finish()?, head.finish()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ShapeKind;

    fn close(a: Point, b: Point) -> bool {
        (a.x - b.x).abs() < 1e-3 && (a.y - b.y).abs() < 1e-3
    }

    #[test]
    fn snap_angle_rounds_to_the_nearest_eighth_turn_keeping_length() {
        let snapped = snap_angle(Point::new(10.0, 3.0), Point::new(0.0, 0.0));
        assert!(close(snapped, Point::new(10.0f32.hypot(3.0), 0.0)));
        let diagonal = snap_angle(Point::new(10.0, 9.0), Point::new(0.0, 0.0));
        assert!((diagonal.x - diagonal.y).abs() < 1e-3);
    }

    #[test]
    fn square_uses_the_longer_side_and_keeps_direction() {
        assert!(close(square(Point::new(-3.0, 8.0), Point::new(0.0, 0.0)), Point::new(-8.0, 8.0)));
    }

    #[test]
    fn dragging_a_corner_past_the_opposite_edge_flips_the_rect() {
        let rect = Rect { x: 10.0, y: 10.0, width: 20.0, height: 20.0 };
        assert_eq!(resize(&rect, Handle::Corner(0), Point::new(40.0, 5.0)), Rect { x: 30.0, y: 5.0, width: 10.0, height: 25.0 });
        assert_eq!(resize(&rect, Handle::Edge(1), Point::new(15.0, 0.0)), Rect { x: 10.0, y: 10.0, width: 5.0, height: 20.0 });
    }

    #[test]
    fn lines_are_hit_within_half_their_width_plus_tolerance() {
        let mut shape = Shape::new(ShapeKind::Line, ShapeKind::Line.default_style(), Point::new(0.0, 0.0));
        shape.style.line_width = 4.0;
        shape.end = Point::new(100.0, 0.0);
        assert!(contains(&shape, Point::new(50.0, 6.9), 5.0));
        assert!(!contains(&shape, Point::new(50.0, 7.1), 5.0));
        assert!(!contains(&shape, Point::new(108.0, 0.0), 5.0));
    }

    #[test]
    fn ellipses_are_hit_inside_the_oval_only() {
        let mut shape = Shape::new(ShapeKind::Ellipse, ShapeKind::Ellipse.default_style(), Point::new(0.0, 0.0));
        shape.end = Point::new(100.0, 50.0);
        assert!(contains(&shape, Point::new(50.0, 25.0), 0.0));
        assert!(!contains(&shape, Point::new(2.0, 2.0), 0.0));
    }

    #[test]
    fn speech_balloon_bounds_include_the_tail() {
        let mut shape = Shape::new(ShapeKind::SpeechBalloon, ShapeKind::SpeechBalloon.default_style(), Point::new(0.0, 0.0));
        shape.end = Point::new(50.0, 20.0);
        shape.tail = Point::new(70.0, 60.0);
        assert_eq!(bounds(&shape), Rect { x: 0.0, y: 0.0, width: 70.0, height: 60.0 });
    }
}
