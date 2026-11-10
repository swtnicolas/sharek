use crate::document::Document;
use crate::geometry::{self, Handle};
use crate::model::{EditorState, Frame, Point, Rect, Rgba, RgbaImage, Shape, ShapeKind, StyleStore, Tool};
use crate::view::{Mips, View};
use sharek_common::text::Font;
use std::sync::Arc;
use tiny_skia::{Color, FillRule, Paint, PathBuilder, PixmapMut, Stroke, StrokeDash, Transform};

const TEXT_SELECTION: [u8; 4] = [0x0a, 0x84, 0xff, 90];

#[derive(Clone, Copy)]
enum Drag {
    Create { id: u64, origin: Point },
    Move { id: u64, last: Point },
    Handle { id: u64, handle: Handle },
    Crop { origin: Point },
    CropHandle(Handle),
    Cut { start: Point },
    TextArea { origin: Point },
    Pan { last: (f32, f32) },
}

#[derive(Default)]
pub struct Changes {
    pub content: bool,
    pub selection: bool,
    pub tool: Option<Tool>,
    pub begin_edit: bool,
    pub end_edit: bool,
    pub redraw: bool,
    pub recognize: Option<Rect>,
}

pub struct CaptureTab {
    pub doc: Document,
    pub view: View,
    pub mips: Mips,
    pub tool: Tool,
    pub selected: Option<u64>,
    pub pending_crop: Option<Rect>,
    pub editing: Option<u64>,
    pub fitted: bool,
    pub auto_fit: bool,
    pub selecting_text: bool,
    pub caret: Option<(usize, usize)>,
    typed: bool,
    text_area: Option<Rect>,
    drag: Option<Drag>,
    before: Option<EditorState>,
    changed: bool,
    cut_preview: Option<(Point, Point)>,
    editing_before: Option<EditorState>,
}

impl CaptureTab {
    pub fn new(doc: Document, tool: Tool) -> CaptureTab {
        CaptureTab {
            doc,
            view: View::default(),
            mips: Mips::new(),
            tool,
            selected: None,
            pending_crop: None,
            editing: None,
            fitted: false,
            auto_fit: true,
            selecting_text: false,
            caret: None,
            typed: false,
            text_area: None,
            drag: None,
            before: None,
            changed: false,
            cut_preview: None,
            editing_before: None,
        }
    }

    fn tolerance(&self) -> f32 {
        5.0 / self.view.zoom
    }

    pub fn selected_shape(&self) -> Option<&Shape> {
        self.selected.and_then(|id| self.shape(id))
    }

    fn shape(&self, id: u64) -> Option<&Shape> {
        self.doc.state.shapes.iter().find(|shape| shape.id == id)
    }

    fn update(&mut self, id: u64, body: impl FnOnce(&mut Shape)) {
        if let Some(shape) = self.doc.state.shapes.iter_mut().find(|shape| shape.id == id) {
            body(shape);
        }
    }

    fn remove(&mut self, id: u64) {
        self.doc.state.shapes.retain(|shape| shape.id != id);
    }

    fn top_shape(&self, point: Point) -> Option<&Shape> {
        let tolerance = self.tolerance();
        self.doc.state.shapes.iter().rev().find(|shape| geometry::contains(shape, point, tolerance))
    }

    fn handle_at(&self, point: Point, handles: &[(Handle, Point)]) -> Option<Handle> {
        let reach = 7.0 / self.view.zoom;
        handles.iter().find(|(_, at)| (at.x - point.x).abs() <= reach && (at.y - point.y).abs() <= reach).map(|(handle, _)| *handle)
    }

    fn clamp(&self, point: Point) -> Point {
        let (width, height) = self.doc.canvas();
        Point::new(point.x.clamp(0.0, width), point.y.clamp(0.0, height))
    }

    fn select(&mut self, id: Option<u64>, changes: &mut Changes) {
        if self.selected != id {
            self.selected = id;
            changes.selection = true;
        }
        changes.redraw = true;
    }

    fn commit(&mut self, before: EditorState, name: &str, changes: &mut Changes) {
        self.doc.register(before, name);
        changes.content = true;
        changes.redraw = true;
    }

    pub fn set_tool(&mut self, tool: Tool, font: &Font) -> Changes {
        let mut changes = Changes::default();
        self.end_editing(true, font, &mut changes);
        self.cancel_crop(&mut changes);
        self.cancel_text_area(&mut changes);
        self.tool = tool;
        if tool != Tool::Select {
            self.select(None, &mut changes);
        }
        changes
    }

    pub fn press(&mut self, point: Point, view_point: (f32, f32), button: &str, shift: bool, styles: &StyleStore, font: &Font) -> Changes {
        let mut changes = Changes::default();
        if button == "middle" {
            self.drag = Some(Drag::Pan { last: view_point });
            return changes;
        }
        self.end_editing(true, font, &mut changes);
        if button == "right" {
            let hit = self.top_shape(point).map(|shape| shape.id);
            self.select(hit, &mut changes);
            return changes;
        }
        if button != "left" {
            return changes;
        }
        if self.selecting_text {
            self.drag = Some(Drag::TextArea { origin: self.clamp(point) });
            self.text_area = None;
            return changes;
        }
        let _ = shift;
        match self.tool {
            Tool::Crop => {
                let handle = self.pending_crop.and_then(|crop| self.handle_at(point, &geometry::rect_handles(&crop)));
                self.drag = Some(match handle {
                    Some(handle) => Drag::CropHandle(handle),
                    None => {
                        self.pending_crop = None;
                        Drag::Crop { origin: self.clamp(point) }
                    }
                });
                changes.redraw = true;
                return changes;
            }
            Tool::CutOut => {
                self.drag = Some(Drag::Cut { start: self.clamp(point) });
                return changes;
            }
            _ => {}
        }
        self.before = Some(self.doc.state.clone());
        self.changed = false;
        if let Some(selected) = self.selected_shape()
            && let Some(handle) = self.handle_at(point, &geometry::handles(selected)) {
                self.drag = Some(Drag::Handle { id: selected.id, handle });
                return changes;
            }
        if let Some(hit) = self.top_shape(point).map(|shape| shape.id)
            && (self.tool == Tool::Select || Some(hit) == self.selected) {
                self.select(Some(hit), &mut changes);
                self.drag = Some(Drag::Move { id: hit, last: point });
                return changes;
            }
        let Some(kind) = self.tool.kind() else {
            self.select(None, &mut changes);
            self.before = None;
            return changes;
        };
        self.create(kind, point, styles, font, &mut changes);
        changes
    }

    fn create(&mut self, kind: ShapeKind, point: Point, styles: &StyleStore, font: &Font, changes: &mut Changes) {
        let mut shape = Shape::new(kind, styles.style(kind), point);
        if kind == ShapeKind::SmartEraser {
            let base = &self.doc.state.base;
            let (x, y) = ((point.x * self.doc.scale) as i64, (point.y * self.doc.scale) as i64);
            shape.style.fill = if x >= 0 && y >= 0 && (x as u32) < base.width && (y as u32) < base.height {
                Rgba::from_rgb8(base.pixel(x as u32, y as u32))
            } else {
                Rgba::WHITE
            };
        }
        if kind.is_text() {
            geometry::fit(font, &mut shape);
            shape.tail = Point::new(shape.start.x + 24.0, shape.end.y + 36.0);
        }
        let id = shape.id;
        self.doc.state.shapes.push(shape);
        self.select(Some(id), changes);
        self.changed = true;
        if kind.is_text() {
            let snapshot = self.before.take().unwrap_or_else(|| self.doc.state.clone());
            self.drag = None;
            self.begin_editing(id, snapshot, changes);
        } else if kind == ShapeKind::Step {
            self.drag = Some(Drag::Move { id, last: point });
        } else {
            self.drag = Some(Drag::Create { id, origin: point });
        }
    }

    pub fn motion(&mut self, point: Point, view_point: (f32, f32), shift: bool) -> Changes {
        let mut changes = Changes::default();
        let Some(drag) = self.drag else { return changes };
        match drag {
            Drag::Create { id, origin } => {
                let zoom = self.view.zoom;
                self.update(id, |shape| match shape.kind {
                    ShapeKind::Freehand => {
                        if shape.points.last().is_some_and(|last| last.distance(point) >= 1.0 / zoom) {
                            shape.points.push(point);
                        }
                    }
                    ShapeKind::Line | ShapeKind::Arrow => shape.end = if shift { geometry::snap_angle(point, origin) } else { point },
                    _ => shape.end = if shift { geometry::square(point, origin) } else { point },
                });
                self.changed = true;
            }
            Drag::Move { id, last } => {
                let (dx, dy) = (point.x - last.x, point.y - last.y);
                self.update(id, |shape| shape.map(|p| Point::new(p.x + dx, p.y + dy)));
                self.drag = Some(Drag::Move { id, last: point });
                self.changed = true;
            }
            Drag::Handle { id, handle } => {
                self.update(id, |shape| geometry::apply(handle, point, shape, shift));
                self.changed = true;
            }
            Drag::Crop { origin } => {
                let end = self.clamp(if shift { geometry::square(point, origin) } else { point });
                self.pending_crop = Some(Rect::from_points(origin, end));
            }
            Drag::CropHandle(handle) => {
                if let Some(crop) = self.pending_crop {
                    self.pending_crop = Some(geometry::resize(&crop, handle, self.clamp(point)));
                }
            }
            Drag::Cut { start } => self.cut_preview = Some((start, self.clamp(point))),
            Drag::TextArea { origin } => self.text_area = Some(Rect::from_points(origin, self.clamp(point))),
            Drag::Pan { last } => {
                self.auto_fit = false;
                self.view.scroll.0 -= view_point.0 - last.0;
                self.view.scroll.1 -= view_point.1 - last.1;
                self.view.clamp(self.doc.canvas());
                self.drag = Some(Drag::Pan { last: view_point });
            }
        }
        changes.redraw = true;
        changes
    }

    pub fn release(&mut self, point: Point) -> Changes {
        let mut changes = Changes { redraw: true, ..Default::default() };
        let drag = self.drag.take();
        let before = self.before.take();
        let changed = std::mem::replace(&mut self.changed, false);
        match drag {
            Some(Drag::Create { id, .. }) => {
                let Some(shape) = self.shape(id) else { return changes };
                if is_degenerate(shape) {
                    self.remove(id);
                    self.select(None, &mut changes);
                } else if let Some(before) = before {
                    let name = Tool::for_kind(shape.kind).map_or("Forma", Tool::title);
                    self.commit(before, name, &mut changes);
                }
            }
            Some(Drag::Move { .. }) => {
                if let (true, Some(before)) = (changed, before) {
                    self.commit(before, "Mover", &mut changes);
                }
            }
            Some(Drag::Handle { .. }) => {
                if let (true, Some(before)) = (changed, before) {
                    self.commit(before, "Redimensionar", &mut changes);
                }
            }
            Some(Drag::Crop { .. }) | Some(Drag::CropHandle(_)) => {
                if self.pending_crop.is_some_and(|crop| crop.width < 3.0 || crop.height < 3.0) {
                    self.pending_crop = None;
                }
            }
            Some(Drag::Cut { start }) => {
                self.cut_preview = None;
                let end = self.clamp(point);
                self.apply_cut(start, end, &mut changes);
            }
            Some(Drag::TextArea { origin }) => {
                let area = Rect::from_points(origin, self.clamp(point));
                self.selecting_text = false;
                self.text_area = None;
                if area.width >= 3.0 && area.height >= 3.0 {
                    changes.recognize = Some(area);
                }
            }
            Some(Drag::Pan { .. }) | None => {}
        }
        changes
    }

    pub fn select_text_area(&mut self) -> Changes {
        let mut changes = Changes { redraw: true, ..Default::default() };
        self.cancel_crop(&mut changes);
        self.selecting_text = true;
        changes
    }

    fn cancel_text_area(&mut self, changes: &mut Changes) {
        if std::mem::take(&mut self.selecting_text) || self.text_area.take().is_some() {
            changes.redraw = true;
        }
    }

    pub fn double_click(&mut self, point: Point, font: &Font) -> Changes {
        let mut changes = Changes::default();
        if self.tool == Tool::Crop && self.pending_crop.is_some_and(|crop| crop.contains(point)) {
            self.apply_crop(&mut changes);
            return changes;
        }
        self.end_editing(true, font, &mut changes);
        if let Some(id) = self.top_shape(point).filter(|shape| shape.kind.is_text()).map(|shape| shape.id) {
            self.select(Some(id), &mut changes);
            let before = self.doc.state.clone();
            self.begin_editing(id, before, &mut changes);
        }
        changes
    }

    pub fn escape(&mut self) -> Changes {
        let mut changes = Changes::default();
        if self.selecting_text {
            self.drag = None;
            self.cancel_text_area(&mut changes);
        } else if self.pending_crop.is_some() {
            self.cancel_crop(&mut changes);
        } else {
            self.select(None, &mut changes);
        }
        changes
    }

    pub fn enter(&mut self) -> Changes {
        let mut changes = Changes::default();
        if self.pending_crop.is_some() {
            self.apply_crop(&mut changes);
        } else if let Some(id) = self.selected_shape().filter(|shape| shape.kind.is_text()).map(|shape| shape.id) {
            let before = self.doc.state.clone();
            self.begin_editing(id, before, &mut changes);
        }
        changes
    }

    pub fn delete_selection(&mut self) -> Changes {
        let mut changes = Changes::default();
        let Some(id) = self.selected else { return changes };
        let before = self.doc.state.clone();
        self.remove(id);
        self.select(None, &mut changes);
        self.commit(before, "Eliminar", &mut changes);
        changes
    }

    pub fn duplicate_selection(&mut self) -> Changes {
        let mut changes = Changes::default();
        let Some(copy) = self.selected_shape().map(|shape| shape.duplicated(12.0)) else { return changes };
        let before = self.doc.state.clone();
        let id = copy.id;
        self.doc.state.shapes.push(copy);
        self.select(Some(id), &mut changes);
        self.commit(before, "Duplicar", &mut changes);
        changes
    }

    pub fn reorder(&mut self, front: bool) -> Changes {
        let mut changes = Changes::default();
        let Some(index) = self.selected.and_then(|id| self.doc.state.shapes.iter().position(|shape| shape.id == id)) else { return changes };
        let before = self.doc.state.clone();
        let shape = self.doc.state.shapes.remove(index);
        if front {
            self.doc.state.shapes.push(shape);
        } else {
            self.doc.state.shapes.insert(0, shape);
        }
        self.commit(before, "Ordenar", &mut changes);
        changes
    }

    pub fn nudge(&mut self, dx: f32, dy: f32) -> Changes {
        let mut changes = Changes::default();
        let Some(id) = self.selected else { return changes };
        let before = self.doc.state.clone();
        self.update(id, |shape| shape.map(|p| Point::new(p.x + dx, p.y + dy)));
        self.commit(before, "Mover", &mut changes);
        changes
    }

    pub fn apply_crop(&mut self, changes: &mut Changes) {
        let Some(crop) = self.pending_crop else { return };
        let scale = self.doc.scale;
        let pixels = Rect { x: crop.x * scale, y: crop.y * scale, width: crop.width * scale, height: crop.height * scale }.integral();
        let Some(image) = self.doc.state.base.crop(pixels.x as i64, pixels.y as i64, pixels.width as i64, pixels.height as i64) else { return };
        let before = self.doc.state.clone();
        let (dx, dy) = (pixels.x / scale, pixels.y / scale);
        let mut shapes = self.doc.state.shapes.clone();
        for shape in &mut shapes {
            shape.map(|p| Point::new(p.x - dx, p.y - dy));
        }
        self.doc.state = EditorState::new(image, shapes);
        self.pending_crop = None;
        self.commit(before, Tool::Crop.title(), changes);
        self.fitted = false;
    }

    fn cancel_crop(&mut self, changes: &mut Changes) {
        if self.pending_crop.take().is_some() {
            changes.redraw = true;
        }
    }

    pub fn cancel_crop_now(&mut self) -> Changes {
        let mut changes = Changes::default();
        self.cancel_crop(&mut changes);
        changes
    }

    pub fn apply_crop_now(&mut self) -> Changes {
        let mut changes = Changes::default();
        self.apply_crop(&mut changes);
        changes
    }

    fn apply_cut(&mut self, start: Point, end: Point, changes: &mut Changes) {
        let scale = self.doc.scale;
        let columns = (end.x - start.x).abs() >= (end.y - start.y).abs();
        let (low, high) = if columns { (start.x.min(end.x), start.x.max(end.x)) } else { (start.y.min(end.y), start.y.max(end.y)) };
        let lower_pixel = (low * scale).round() as i64;
        let upper_pixel = (high * scale).round() as i64;
        if upper_pixel - lower_pixel < 2 {
            return;
        }
        let Some(image) = self.doc.state.base.cut_out(lower_pixel, upper_pixel, columns) else { return };
        let before = self.doc.state.clone();
        let (lower, upper) = (lower_pixel as f32 / scale, upper_pixel as f32 / scale);
        let removed = upper - lower;
        let collapse = |value: f32| if value >= upper { value - removed } else if value > lower { lower } else { value };
        let mut shapes = self.doc.state.shapes.clone();
        for shape in &mut shapes {
            shape.map(|p| if columns { Point::new(collapse(p.x), p.y) } else { Point::new(p.x, collapse(p.y)) });
        }
        self.doc.state = EditorState::new(image, shapes);
        self.commit(before, Tool::CutOut.title(), changes);
    }

    pub fn rotate(&mut self, clockwise: bool, font: &Font) -> Changes {
        let (width, height) = self.doc.canvas();
        let base = self.doc.state.base.rotated(clockwise);
        let map = move |p: Point| if clockwise { Point::new(height - p.y, p.x) } else { Point::new(p.y, width - p.x) };
        let pixels = move |w: usize, h: usize, x: usize, y: usize| if clockwise { (h - 1 - y, x) } else { (y, w - 1 - x) };
        self.transform(base, map, pixels, true, "Girar", font)
    }

    pub fn flip(&mut self, horizontal: bool, font: &Font) -> Changes {
        let (width, height) = self.doc.canvas();
        let base = self.doc.state.base.flipped(horizontal);
        let map = move |p: Point| if horizontal { Point::new(width - p.x, p.y) } else { Point::new(p.x, height - p.y) };
        let pixels = move |w: usize, h: usize, x: usize, y: usize| if horizontal { (w - 1 - x, y) } else { (x, h - 1 - y) };
        self.transform(base, map, pixels, false, "Voltear", font)
    }

    fn transform(
        &mut self,
        base: sharek_common::image::RgbImage,
        map: impl Fn(Point) -> Point,
        pixels: impl Fn(usize, usize, usize, usize) -> (usize, usize),
        swap: bool,
        name: &str,
        font: &Font,
    ) -> Changes {
        let mut changes = Changes::default();
        self.end_editing(true, font, &mut changes);
        self.cancel_crop(&mut changes);
        let before = self.doc.state.clone();
        let mut shapes = before.shapes.clone();
        for shape in &mut shapes {
            transform_shape(shape, &map, &pixels, swap);
        }
        self.doc.state = EditorState::new(base, shapes);
        self.commit(before, name, &mut changes);
        self.fitted = false;
        changes
    }

    pub fn apply_frame(&mut self, frame: &Frame, font: &Font) -> Changes {
        let mut changes = Changes::default();
        self.end_editing(true, font, &mut changes);
        self.cancel_crop(&mut changes);
        let Some((image, offset)) = crate::render::framed(&self.doc.state.base, self.doc.scale, frame) else { return changes };
        let before = self.doc.state.clone();
        let mut shapes = before.shapes.clone();
        for shape in &mut shapes {
            shape.map(|p| Point::new(p.x + offset, p.y + offset));
        }
        self.doc.state = EditorState::new(image, shapes);
        self.commit(before, "Marco", &mut changes);
        self.fitted = false;
        changes
    }

    fn begin_editing(&mut self, id: u64, before: EditorState, changes: &mut Changes) {
        if !self.shape(id).is_some_and(|shape| shape.kind.is_text()) {
            return;
        }
        self.editing = Some(id);
        self.editing_before = Some(before);
        self.caret = None;
        self.typed = false;
        changes.begin_edit = true;
        changes.redraw = true;
    }

    pub fn text_changed(&mut self, text: &str, font: &Font) {
        if let Some(id) = self.editing {
            self.typed = true;
            self.update(id, |shape| {
                shape.text = text.to_string();
                geometry::fit(font, shape);
            });
        }
    }

    pub fn text_history_belongs_to_document(&self, redo: bool) -> bool {
        let Some(id) = self.editing else { return true };
        if redo {
            return !self.typed;
        }
        let original = self.editing_before.as_ref().and_then(|before| before.shapes.iter().find(|shape| shape.id == id)).map_or("", |shape| shape.text.as_str());
        self.shape(id).is_none_or(|shape| shape.text == original)
    }

    pub fn live_from(&self) -> usize {
        let dragged = match self.drag {
            Some(Drag::Create { id, .. } | Drag::Move { id, .. } | Drag::Handle { id, .. }) => Some(id),
            _ => None,
        };
        [dragged, self.editing, self.selected]
            .into_iter()
            .flatten()
            .filter_map(|id| self.doc.state.shapes.iter().position(|shape| shape.id == id))
            .min()
            .unwrap_or(self.doc.state.shapes.len())
    }

    pub fn end_editing(&mut self, commit: bool, _font: &Font, changes: &mut Changes) {
        let Some(id) = self.editing.take() else { return };
        let before = self.editing_before.take();
        self.caret = None;
        changes.end_edit = true;
        changes.redraw = true;
        if self.shape(id).is_some_and(|shape| shape.text.trim().is_empty()) {
            self.remove(id);
            self.select(None, changes);
        }
        if let (true, Some(before)) = (commit, before) {
            let previous = before.shapes.iter().find(|shape| shape.id == id).map(|shape| shape.text.clone());
            let current = self.shape(id).map(|shape| shape.text.clone());
            if previous != current {
                self.commit(before, "Texto", changes);
            }
        }
    }

    pub fn finish_editing(&mut self, font: &Font) -> Changes {
        let mut changes = Changes::default();
        self.end_editing(true, font, &mut changes);
        changes
    }

    pub fn restore_finished(&mut self) {
        self.editing = None;
        self.editing_before = None;
        self.pending_crop = None;
        self.drag = None;
        if self.selected.is_some_and(|id| self.shape(id).is_none()) {
            self.selected = None;
        }
    }

    pub fn insert_image(&mut self, image: RgbaImage, image_scale: f32, styles: &StyleStore) -> Changes {
        let mut changes = Changes::default();
        let before = self.doc.state.clone();
        let canvas = self.doc.canvas();
        let top_left = self.view.to_canvas(canvas, 0.0, 0.0);
        let bottom_right = self.view.to_canvas(canvas, self.view.size.0, self.view.size.1);
        let visible = Rect::from_points(self.clamp(top_left), self.clamp(bottom_right));
        let (mut width, mut height) = (image.width as f32 / image_scale, image.height as f32 / image_scale);
        let fit = (visible.width * 0.6 / width).min(visible.height * 0.6 / height).min(1.0);
        width *= fit;
        height *= fit;
        let origin = Point::new(visible.mid().x - width / 2.0, visible.mid().y - height / 2.0);
        let mut shape = Shape::new(ShapeKind::Image, styles.style(ShapeKind::Image), origin);
        shape.end = Point::new(origin.x + width, origin.y + height);
        shape.image = Some(Arc::new(image));
        let id = shape.id;
        self.doc.state.shapes.push(shape);
        self.select(Some(id), &mut changes);
        self.commit(before, "Pegar imagen", &mut changes);
        changes
    }

    fn draw_text_cursor(&self, target: &mut PixmapMut, transform: Transform, font: &Font) {
        let Some(shape) = self.editing.and_then(|id| self.shape(id)) else { return };
        let scale = self.view.device;
        let bounds = geometry::bounds(shape);
        let corner = |x: f32, y: f32| {
            let mut mapped = [tiny_skia::Point::from_xy(x, y)];
            transform.map_points(&mut mapped);
            Point::new(mapped[0].x, mapped[0].y)
        };
        let margin = 3.0 * scale;
        let frame = Rect::from_points(corner(bounds.x, bounds.y), corner(bounds.max_x(), bounds.max_y())).inset(-margin, -margin);
        selection_outline(target, frame, scale);
        let Some((cursor, anchor)) = self.caret else { return };
        let text = shape.text.as_str();
        let size = shape.style.font_size;
        let line_height = font.line_height(size);
        let origin = geometry::text_rect(font, shape);
        let locate = |offset: usize| {
            let offset = text.floor_char_boundary(offset.min(text.len()));
            let line_start = text[..offset].rfind('\n').map_or(0, |at| at + 1);
            (text[..offset].matches('\n').count(), font.measure(&text[line_start..offset], size))
        };
        let line_width = |index: usize| text.split('\n').nth(index).map_or(0.0, |line| font.measure(line, size));
        if cursor != anchor {
            let (first, last) = (locate(cursor.min(anchor)), locate(cursor.max(anchor)));
            for line in first.0..=last.0 {
                let from = if line == first.0 { first.1 } else { 0.0 };
                let to = if line == last.0 { last.1 } else { line_width(line).max(from + size / 4.0) };
                let band = Rect { x: origin.x + 1.0 + from, y: origin.y + line as f32 * line_height, width: to - from, height: line_height };
                if let Some(band) = band.skia() {
                    target.fill_rect(band, &solid(TEXT_SELECTION[0], TEXT_SELECTION[1], TEXT_SELECTION[2], TEXT_SELECTION[3]), transform, None);
                }
            }
            return;
        }
        let (line, x) = locate(cursor);
        let top = corner(origin.x + 1.0 + x, origin.y + line as f32 * line_height);
        let bottom = corner(origin.x + 1.0 + x, origin.y + (line + 1) as f32 * line_height);
        let width = (1.5 * scale).round().max(1.0);
        let halo = scale.round().max(1.0);
        let (left, height) = ((top.x - width / 2.0).round(), (bottom.y - top.y).round().max(1.0));
        if let Some(outer) = tiny_skia::Rect::from_xywh(left - halo, top.y.round() - halo, width + 2.0 * halo, height + 2.0 * halo) {
            target.fill_rect(outer, &solid(255, 255, 255, 255), Transform::identity(), None);
        }
        if let Some(bar) = tiny_skia::Rect::from_xywh(left, top.y.round(), width, height) {
            target.fill_rect(bar, &solid(0, 0, 0, 255), Transform::identity(), None);
        }
    }

    pub fn draw_overlay(&self, target: &mut PixmapMut, transform: Transform, font: &Font) {
        self.draw_text_cursor(target, transform, font);
        let canvas = self.doc.canvas();
        let canvas_rect = Rect { x: 0.0, y: 0.0, width: canvas.0, height: canvas.1 };
        if let Some(crop) = self.pending_crop {
            let mut builder = PathBuilder::new();
            if let (Some(outer), Some(inner)) = (canvas_rect.skia(), crop.skia()) {
                builder.push_rect(outer);
                builder.push_rect(inner);
            }
            if let Some(path) = builder.finish() {
                target.fill_path(&path, &solid(0, 0, 0, 128), FillRule::EvenOdd, transform, None);
            }
        }
        if let Some(area) = self.text_area {
            let mut builder = PathBuilder::new();
            if let (Some(outer), Some(inner)) = (canvas_rect.skia(), area.skia()) {
                builder.push_rect(outer);
                builder.push_rect(inner);
            }
            if let Some(path) = builder.finish() {
                target.fill_path(&path, &solid(0, 0, 0, 110), FillRule::EvenOdd, transform, None);
            }
        }
        if let Some((start, end)) = self.cut_preview {
            let columns = (end.x - start.x).abs() >= (end.y - start.y).abs();
            let band = if columns {
                Rect { x: start.x.min(end.x), y: 0.0, width: (end.x - start.x).abs(), height: canvas.1 }
            } else {
                Rect { x: 0.0, y: start.y.min(end.y), width: canvas.0, height: (end.y - start.y).abs() }
            };
            if let Some(rect) = band.skia() {
                target.fill_rect(rect, &solid(255, 59, 48, 89), transform, None);
            }
        }
        let device = |point: Point| {
            let mut mapped = [tiny_skia::Point::from_xy(point.x, point.y)];
            transform.map_points(&mut mapped);
            Point::new(mapped[0].x, mapped[0].y)
        };
        let scale = self.view.device;
        if let Some(area) = self.text_area {
            let a = device(Point::new(area.x, area.y));
            let b = device(Point::new(area.max_x(), area.max_y()));
            selection_outline(target, Rect::from_points(a, b), scale);
        }
        if let Some(crop) = self.pending_crop {
            let a = device(Point::new(crop.x, crop.y));
            let b = device(Point::new(crop.max_x(), crop.max_y()));
            selection_outline(target, Rect::from_points(a, b), scale);
            draw_handles(target, geometry::rect_handles(&crop).iter().map(|(_, p)| device(*p)), scale);
        }
        let Some(shape) = self.selected_shape().filter(|_| self.editing.is_none()) else { return };
        let handles = geometry::handles(shape);
        if !matches!(shape.kind, ShapeKind::Line | ShapeKind::Arrow) {
            let cornered = handles.iter().any(|(handle, _)| matches!(handle, Handle::Corner(_)));
            let bounds = if cornered { shape.rect() } else { geometry::bounds(shape) };
            let a = device(Point::new(bounds.x, bounds.y));
            let b = device(Point::new(bounds.max_x(), bounds.max_y()));
            let margin = if cornered { 0.0 } else { (shape.style.line_width / 2.0 * self.view.zoom + 3.0) * scale };
            selection_outline(target, Rect::from_points(a, b).inset(-margin, -margin), scale);
        }
        draw_handles(target, handles.iter().map(|(_, p)| device(*p)), scale);
    }

    pub fn style_changed(&mut self, font: &Font, change: impl FnOnce(&mut crate::model::ShapeStyle)) -> Option<EditorState> {
        let id = self.selected?;
        let before = self.doc.state.clone();
        self.update(id, |shape| {
            change(&mut shape.style);
            if shape.kind.is_text() {
                geometry::fit(font, shape);
            }
        });
        Some(before)
    }
}

fn transform_shape(shape: &mut Shape, map: &impl Fn(Point) -> Point, pixels: &impl Fn(usize, usize, usize, usize) -> (usize, usize), swap: bool) {
    if shape.kind.is_text() {
        let rect = shape.rect();
        let center = map(rect.mid());
        shape.start = Point::new(center.x - rect.width / 2.0, center.y - rect.height / 2.0);
        shape.end = Point::new(center.x + rect.width / 2.0, center.y + rect.height / 2.0);
        shape.tail = map(shape.tail);
        return;
    }
    shape.map(map);
    if !matches!(shape.kind, ShapeKind::Line | ShapeKind::Arrow | ShapeKind::Freehand | ShapeKind::Step) {
        let rect = shape.rect();
        shape.start = Point::new(rect.x, rect.y);
        shape.end = Point::new(rect.max_x(), rect.max_y());
    }
    if let Some(image) = &shape.image {
        let (width, height) = (image.width as usize, image.height as usize);
        if let Some(turned) = image.transformed(|x, y| pixels(width, height, x, y), swap) {
            shape.image = Some(Arc::new(turned));
        }
    }
}

fn is_degenerate(shape: &Shape) -> bool {
    match shape.kind {
        ShapeKind::Freehand => shape.points.len() < 2,
        ShapeKind::Line | ShapeKind::Arrow => shape.start.distance(shape.end) < 3.0,
        ShapeKind::Step | ShapeKind::TextOutline | ShapeKind::TextBackground | ShapeKind::SpeechBalloon => false,
        _ => shape.rect().width < 3.0 || shape.rect().height < 3.0,
    }
}

fn solid(r: u8, g: u8, b: u8, a: u8) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color(Color::from_rgba8(r, g, b, a));
    paint.anti_alias = true;
    paint
}

fn selection_outline(target: &mut PixmapMut, rect: Rect, scale: f32) {
    let rect = rect.integral().inset(0.5, 0.5);
    let Some(rect) = rect.skia() else { return };
    let path = PathBuilder::from_rect(rect);
    let width = scale.max(1.0);
    let mut dark = solid(0, 0, 0, 255);
    dark.anti_alias = false;
    target.stroke_path(&path, &dark, &Stroke { width, ..Default::default() }, Transform::identity(), None);
    let mut light = solid(255, 255, 255, 255);
    light.anti_alias = false;
    let dashed = Stroke { width, dash: StrokeDash::new(vec![4.0 * scale, 4.0 * scale], 0.0), ..Default::default() };
    target.stroke_path(&path, &light, &dashed, Transform::identity(), None);
}

fn draw_handles(target: &mut PixmapMut, points: impl Iterator<Item = Point>, scale: f32) {
    let half = 4.5 * scale;
    for point in points {
        let Some(rect) = tiny_skia::Rect::from_xywh(point.x.round() - half, point.y.round() - half, half * 2.0, half * 2.0) else { continue };
        target.fill_rect(rect, &solid(255, 255, 255, 255), Transform::identity(), None);
        let path = PathBuilder::from_rect(rect);
        target.stroke_path(&path, &solid(0, 0, 0, 255), &Stroke { width: scale.max(1.0), ..Default::default() }, Transform::identity(), None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sharek_common::image::RgbImage;

    fn tab(width: u32, height: u32) -> CaptureTab {
        let base = RgbImage { width, height, data: vec![128; (width * height * 3) as usize].into() };
        let mut tab = CaptureTab::new(Document::new(1, base, 1.0, 0, None), Tool::Rectangle);
        tab.view = View { zoom: 1.0, scroll: (0.0, 0.0), size: (width as f32, height as f32), device: 1.0 };
        tab
    }

    fn rectangle(tab: &mut CaptureTab, start: Point, end: Point) -> u64 {
        let mut shape = Shape::new(ShapeKind::Rectangle, ShapeKind::Rectangle.default_style(), start);
        shape.end = end;
        let id = shape.id;
        tab.doc.state.shapes.push(shape);
        id
    }

    #[test]
    fn undo_while_typing_belongs_to_the_text_until_it_is_back_to_where_it_started() {
        let mut tab = tab(200, 100);
        let font = Font::bold().expect("system font");
        tab.tool = Tool::TextOutline;
        let styles = StyleStore::empty();
        tab.press(Point::new(20.0, 20.0), (20.0, 20.0), "left", false, &styles, &font);
        assert!(tab.editing.is_some());
        assert!(tab.text_history_belongs_to_document(false));
        assert!(tab.text_history_belongs_to_document(true));
        tab.text_changed("hola", &font);
        assert!(!tab.text_history_belongs_to_document(false));
        assert!(!tab.text_history_belongs_to_document(true));
        tab.text_changed("", &font);
        assert!(tab.text_history_belongs_to_document(false));
        assert!(!tab.text_history_belongs_to_document(true));
    }

    #[test]
    fn the_shape_being_dragged_and_everything_above_it_are_drawn_live() {
        let mut tab = tab(200, 100);
        let font = Font::bold().expect("system font");
        let styles = StyleStore::empty();
        let below = rectangle(&mut tab, Point::new(10.0, 10.0), Point::new(40.0, 40.0));
        rectangle(&mut tab, Point::new(100.0, 10.0), Point::new(140.0, 40.0));
        assert_eq!(tab.live_from(), 2);
        tab.tool = Tool::Select;
        tab.press(Point::new(10.0, 25.0), (10.0, 25.0), "left", false, &styles, &font);
        assert_eq!(tab.selected, Some(below));
        assert_eq!(tab.live_from(), 0);
        tab.release(Point::new(10.0, 25.0));
        tab.escape();
        assert_eq!(tab.live_from(), 2);
    }

    #[test]
    fn a_pasted_image_larger_than_the_view_keeps_its_proportions() {
        let mut tab = tab(200, 100);
        let image = RgbaImage { width: 400, height: 200, pixmap: tiny_skia::Pixmap::new(400, 200).unwrap() };
        tab.insert_image(image, 1.0, &StyleStore::empty());
        let rect = tab.selected_shape().unwrap().rect();
        assert_eq!((rect.width, rect.height), (120.0, 60.0));
    }

    #[test]
    fn the_selection_frame_of_a_resizable_shape_runs_through_its_handles() {
        let mut tab = tab(100, 100);
        let font = Font::bold().expect("system font");
        let id = rectangle(&mut tab, Point::new(20.0, 20.0), Point::new(80.0, 70.0));
        tab.selected = Some(id);
        let mut pixmap = tiny_skia::Pixmap::new(100, 100).unwrap();
        tab.draw_overlay(&mut pixmap.as_mut(), Transform::identity(), &font);
        let drawn = |x: u32, y: u32| pixmap.pixel(x, y).is_some_and(|pixel| pixel.alpha() > 0);
        assert!((26..36).all(|y| drawn(20, y)));
        assert!((26..36).all(|y| !drawn(15, y) && !drawn(16, y)));
    }

    #[test]
    fn cutting_a_vertical_band_collapses_shapes_onto_the_seam() {
        let mut tab = tab(100, 50);
        let inside = rectangle(&mut tab, Point::new(30.0, 5.0), Point::new(90.0, 20.0));
        let mut changes = Changes::default();
        tab.apply_cut(Point::new(40.0, 10.0), Point::new(60.0, 12.0), &mut changes);
        assert!(changes.content);
        assert_eq!(tab.doc.state.base.width, 80);
        let shape = tab.shape(inside).unwrap();
        assert_eq!((shape.start.x, shape.end.x), (30.0, 70.0));
    }

    #[test]
    fn cropping_shifts_shapes_into_the_new_origin() {
        let mut tab = tab(100, 100);
        let id = rectangle(&mut tab, Point::new(40.0, 40.0), Point::new(60.0, 60.0));
        tab.pending_crop = Some(Rect { x: 30.0, y: 20.0, width: 50.0, height: 50.0 });
        let changes = tab.apply_crop_now();
        assert!(changes.content);
        assert_eq!((tab.doc.state.base.width, tab.doc.state.base.height), (50, 50));
        assert_eq!(tab.shape(id).unwrap().start, Point::new(10.0, 20.0));
        assert!(tab.doc.undo());
        assert_eq!(tab.doc.state.base.width, 100);
    }

    #[test]
    fn rotating_clockwise_turns_the_image_and_carries_shapes_along() {
        let mut tab = tab(100, 50);
        let font = Font::bold().expect("system font");
        let id = rectangle(&mut tab, Point::new(10.0, 5.0), Point::new(30.0, 15.0));
        let changes = tab.rotate(true, &font);
        assert!(changes.content);
        assert_eq!((tab.doc.state.base.width, tab.doc.state.base.height), (50, 100));
        let shape = tab.shape(id).unwrap();
        assert_eq!((shape.start, shape.end), (Point::new(35.0, 10.0), Point::new(45.0, 30.0)));
        assert!(tab.doc.undo());
        assert_eq!(tab.doc.state.base.width, 100);
    }

    #[test]
    fn rotating_keeps_text_upright_around_its_new_center() {
        let mut tab = tab(100, 50);
        let font = Font::bold().expect("system font");
        let mut text = Shape::new(ShapeKind::TextBackground, ShapeKind::TextBackground.default_style(), Point::new(10.0, 10.0));
        text.end = Point::new(50.0, 20.0);
        let id = text.id;
        tab.doc.state.shapes.push(text);
        tab.rotate(true, &font);
        let rect = tab.shape(id).unwrap().rect();
        assert_eq!((rect.width, rect.height), (40.0, 10.0));
        assert_eq!(rect.mid(), Point::new(35.0, 30.0));
    }

    #[test]
    fn flipping_mirrors_an_arrow_without_swapping_its_ends() {
        let mut tab = tab(100, 50);
        let font = Font::bold().expect("system font");
        let mut arrow = Shape::new(ShapeKind::Arrow, ShapeKind::Arrow.default_style(), Point::new(10.0, 10.0));
        arrow.end = Point::new(40.0, 30.0);
        let id = arrow.id;
        tab.doc.state.shapes.push(arrow);
        tab.flip(true, &font);
        let shape = tab.shape(id).unwrap();
        assert_eq!((shape.start, shape.end), (Point::new(90.0, 10.0), Point::new(60.0, 30.0)));
    }

    #[test]
    fn a_frame_grows_the_canvas_and_shifts_shapes_by_the_margin() {
        let mut tab = tab(100, 50);
        let font = Font::bold().expect("system font");
        let id = rectangle(&mut tab, Point::new(10.0, 10.0), Point::new(20.0, 20.0));
        let frame = Frame { margin: 16.0, shadow: false, ..Frame::default() };
        assert!(tab.apply_frame(&frame, &font).content);
        assert_eq!((tab.doc.state.base.width, tab.doc.state.base.height), (132, 82));
        assert_eq!(tab.shape(id).unwrap().start, Point::new(26.0, 26.0));
    }

    #[test]
    fn shapes_on_a_large_capture_keep_the_chosen_size() {
        let mut tab = tab(2400, 1200);
        let styles = StyleStore::empty();
        let font = Font::bold().expect("system font");
        tab.tool = Tool::Arrow;
        tab.press(Point::new(10.0, 10.0), (10.0, 10.0), "left", false, &styles, &font);
        tab.motion(Point::new(200.0, 200.0), (200.0, 200.0), false);
        tab.release(Point::new(200.0, 200.0));
        assert_eq!(tab.doc.state.shapes.last().unwrap().style.line_width, ShapeKind::Arrow.default_style().line_width);
    }

    #[test]
    fn dragging_over_text_returns_the_area_to_recognize_inside_the_image() {
        let mut tab = tab(100, 80);
        let styles = StyleStore::empty();
        let font = Font::bold().expect("system font");
        tab.select_text_area();
        tab.press(Point::new(20.0, 10.0), (20.0, 10.0), "left", false, &styles, &font);
        tab.motion(Point::new(150.0, 40.0), (150.0, 40.0), false);
        let changes = tab.release(Point::new(150.0, 40.0));
        assert_eq!(changes.recognize, Some(Rect { x: 20.0, y: 10.0, width: 80.0, height: 30.0 }));
        assert!(!tab.selecting_text && tab.doc.state.shapes.is_empty());
    }

    #[test]
    fn a_click_while_selecting_text_recognizes_nothing() {
        let mut tab = tab(100, 80);
        let styles = StyleStore::empty();
        let font = Font::bold().expect("system font");
        tab.select_text_area();
        tab.press(Point::new(20.0, 10.0), (20.0, 10.0), "left", false, &styles, &font);
        assert_eq!(tab.release(Point::new(21.0, 11.0)).recognize, None);
    }

    #[test]
    fn a_click_without_drag_leaves_no_rectangle_behind() {
        let mut tab = tab(100, 100);
        let styles = StyleStore::empty();
        let font = Font::bold().expect("system font");
        tab.press(Point::new(10.0, 10.0), (10.0, 10.0), "left", false, &styles, &font);
        let changes = tab.release(Point::new(11.0, 11.0));
        assert!(!changes.content);
        assert!(tab.doc.state.shapes.is_empty());
    }
}
