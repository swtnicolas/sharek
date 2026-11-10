use crate::app::App;
use crate::draw::{self, bgra};
use crate::windows::Rect;
use crate::wl::{Globals, PixelBuffer};
use sharek_common::image::RgbImage;
use sharek_common::region_label;
use sharek_common::text::Font;
use smithay_client_toolkit::compositor::FrameCallbackData;
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerSurface};
use smithay_client_toolkit::shm::raw::RawPool;
use smithay_client_toolkit::shm::slot::{Buffer, SlotPool};
use tiny_skia::{FillRule, Paint, PathBuilder, PixmapMut, Stroke, Transform};
use wayland_client::protocol::{wl_buffer::WlBuffer, wl_output::WlOutput, wl_shm, wl_subsurface::WlSubsurface, wl_surface::WlSurface};
use wayland_client::QueueHandle;
use wayland_protocols::wp::fractional_scale::v1::client::wp_fractional_scale_v1::WpFractionalScaleV1;
use wayland_protocols::wp::viewporter::client::wp_viewport::WpViewport;

const MAGNIFIER_SIZE: f64 = 132.0;
const MAGNIFIER_PIXELS: i64 = 21;
const HINT: &str = " Arrastra para medir · Ctrl+C copia la medida · Esc cierra ";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Select,
    Measure,
    Pick,
}

pub struct Selection {
    pub image: Option<RgbImage>,
    pub global: Rect,
    pub output: String,
    pub ratio: f64,
}

pub enum Request {
    Finish(Option<Selection>),
    CopyText { text: String, title: String, detail: String },
}

pub struct FrozenScreen {
    pub output: WlOutput,
    pub name: String,
    pub origin: (f64, f64),
    pub size: (f64, f64),
    pub pool: RawPool,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
}

struct Part {
    surface: WlSurface,
    subsurface: WlSubsurface,
    viewport: WpViewport,
    mapped: bool,
    buffer: Option<Buffer>,
}

impl Part {
    fn new(globals: &Globals, parent: &WlSurface, qh: &QueueHandle<App>) -> Part {
        let (subsurface, surface) = globals.subcompositor.create_subsurface(parent.clone(), qh);
        globals.no_input(&surface);
        let viewport = globals.viewport(&surface, qh);
        Part { surface, subsurface, viewport, mapped: false, buffer: None }
    }

    fn solid(&mut self, buffer: &WlBuffer, rect: Rect) {
        let rect = rect.integral();
        if rect.width < 1.0 || rect.height < 1.0 {
            self.hide();
            return;
        }
        self.subsurface.set_position(rect.x as i32, rect.y as i32);
        self.viewport.set_destination(rect.width as i32, rect.height as i32);
        self.surface.attach(Some(buffer), 0, 0);
        self.surface.damage_buffer(0, 0, 1, 1);
        self.surface.commit();
        self.mapped = true;
    }

    fn strip(&mut self, buffer: &Buffer, rect: Rect, horizontal: bool) {
        let rect = rect.integral();
        let length = if horizontal { rect.width } else { rect.height };
        if length < 1.0 {
            self.hide();
            return;
        }
        self.subsurface.set_position(rect.x as i32, rect.y as i32);
        if horizontal {
            self.viewport.set_source(0.0, 0.0, length, 1.0);
            self.viewport.set_destination(length as i32, 1);
        } else {
            self.viewport.set_source(0.0, 0.0, 1.0, length);
            self.viewport.set_destination(1, length as i32);
        }
        self.surface.attach(Some(buffer.wl_buffer()), 0, 0);
        self.surface.damage_buffer(0, 0, i32::MAX, i32::MAX);
        self.surface.commit();
        self.mapped = true;
    }

    fn image(&mut self, pool: &mut SlotPool, x: f64, y: f64, size: (u32, u32), scale: f64, paint: impl FnOnce(&mut PixmapMut)) {
        let (width, height) = size;
        let Ok((buffer, canvas)) = pool.create_buffer(width as i32, height as i32, width as i32 * 4, wl_shm::Format::Argb8888) else { return };
        canvas.fill(0);
        if let Some(mut pixmap) = PixmapMut::from_bytes(canvas, width, height) {
            paint(&mut pixmap);
        }
        self.subsurface.set_position(x.round() as i32, y.round() as i32);
        self.viewport.set_source(-1.0, -1.0, -1.0, -1.0);
        self.viewport.set_destination((f64::from(width) / scale).round().max(1.0) as i32, (f64::from(height) / scale).round().max(1.0) as i32);
        let _ = buffer.attach_to(&self.surface);
        self.surface.damage_buffer(0, 0, width as i32, height as i32);
        self.surface.commit();
        self.buffer = Some(buffer);
        self.mapped = true;
    }

    fn hide(&mut self) {
        if self.mapped {
            self.surface.attach(None, 0, 0);
            self.surface.commit();
            self.mapped = false;
        }
    }

    fn destroy(self) {
        self.viewport.destroy();
        self.subsurface.destroy();
        self.surface.destroy();
    }
}

struct Parts {
    dim: [Part; 4],
    outline: [Part; 4],
    cross: [Part; 2],
    line: Part,
    label: Part,
    hint: Part,
    magnifier: Part,
}

struct Screen {
    frozen: FrozenScreen,
    buffer: WlBuffer,
    layer: LayerSurface,
    viewport: WpViewport,
    fractional: Option<WpFractionalScaleV1>,
    parts: Option<Parts>,
    windows: Vec<Rect>,
    cursor: Option<(f64, f64)>,
    drag_origin: Option<(f64, f64)>,
    selection: Option<Rect>,
    hovered: Option<Rect>,
    measure_start: Option<(f64, f64)>,
    measure_end: Option<(f64, f64)>,
    dirty: bool,
    frame_pending: bool,
}

impl Screen {
    fn ratio(&self) -> f64 {
        f64::from(self.frozen.width) / self.frozen.size.0
    }

    fn bounds(&self) -> Rect {
        Rect { x: 0.0, y: 0.0, width: self.frozen.size.0, height: self.frozen.size.1 }
    }

    fn clamp(&self, (x, y): (f64, f64)) -> (f64, f64) {
        (x.clamp(0.0, self.frozen.size.0), y.clamp(0.0, self.frozen.size.1))
    }

    fn snapped(&self, (x, y): (f64, f64)) -> (f64, f64) {
        let ratio = self.ratio();
        ((x * ratio).round() / ratio, (y * ratio).round() / ratio)
    }

    fn pixel(&mut self, x: i64, y: i64) -> Option<[u8; 3]> {
        if x < 0 || y < 0 || x >= i64::from(self.frozen.width) || y >= i64::from(self.frozen.height) {
            return None;
        }
        let offset = (y as usize * self.frozen.width as usize + x as usize) * 3;
        let data = &self.frozen.pool.mmap()[offset..offset + 3];
        Some([data[0], data[1], data[2]])
    }

    fn crop(&mut self, rect: Rect) -> Option<RgbImage> {
        let ratio = self.ratio();
        let (width, height) = (self.frozen.width, self.frozen.height);
        let x = (rect.x * ratio).round() as i64;
        let y = (rect.y * ratio).round() as i64;
        let right = ((rect.x + rect.width) * ratio).round() as i64;
        let bottom = ((rect.y + rect.height) * ratio).round() as i64;
        RgbImage::crop_from(&self.frozen.pool.mmap()[..width as usize * height as usize * 3], width, height, x, y, right - x, bottom - y)
    }
}

struct Measurement {
    width: i64,
    height: i64,
    diagonal: f64,
    angle: f64,
}

pub struct Overlay {
    mode: Mode,
    screens: Vec<Screen>,
    pool: SlotPool,
    dim: WlBuffer,
    cross: WlBuffer,
    strip_horizontal: Option<Buffer>,
    strip_vertical: Option<Buffer>,
    font: Option<Font>,
    active: usize,
    separator: char,
    finished: bool,
}

fn format_number(value: f64, separator: char) -> String {
    let rounded = (value * 10.0).round() / 10.0;
    let text = if rounded.fract() == 0.0 { format!("{rounded:.0}") } else { format!("{rounded:.1}") };
    text.replace('.', &separator.to_string())
}

impl Overlay {
    pub fn new(mode: Mode, frozen: Vec<FrozenScreen>, windows: Vec<Rect>, globals: &Globals, qh: &QueueHandle<App>) -> Option<Overlay> {
        let mut pool = SlotPool::new(256 << 10, &globals.shm).ok()?;
        let dim = globals.solid([0.0, 0.0, 0.0, 0.4], qh);
        let cross = globals.solid([1.0, 1.0, 1.0, 0.35], qh);
        let longest = frozen.iter().map(|screen| screen.size.0.max(screen.size.1)).fold(1.0, f64::max).ceil() as i32;
        let strip_horizontal = dashed_strip(&mut pool, longest, true);
        let strip_vertical = dashed_strip(&mut pool, longest, false);
        let mut screens = Vec::new();
        for frozen in frozen {
            let mut frozen = frozen;
            let buffer = frozen.pool.create_buffer(0, frozen.width as i32, frozen.height as i32, frozen.width as i32 * 3, wl_shm::Format::Bgr888, PixelBuffer, qh);
            let surface = globals.compositor.create_surface(qh);
            let viewport = globals.viewport(&surface, qh);
            let fractional = globals.fractional_scale(&surface, qh);
            let layer = globals.layer_shell.create_layer_surface(qh, surface, Layer::Overlay, Some("sharek-overlay"), Some(&frozen.output));
            layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
            layer.set_exclusive_zone(-1);
            layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
            layer.commit();
            let screen_rect = Rect { x: frozen.origin.0, y: frozen.origin.1, width: frozen.size.0, height: frozen.size.1 };
            let local = if mode == Mode::Select {
                windows
                    .iter()
                    .filter_map(|window| window.intersection(&screen_rect))
                    .map(|rect| Rect { x: rect.x - screen_rect.x, y: rect.y - screen_rect.y, ..rect })
                    .collect()
            } else {
                Vec::new()
            };
            screens.push(Screen {
                frozen,
                buffer,
                layer,
                viewport,
                fractional,
                parts: None,
                windows: local,
                cursor: None,
                drag_origin: None,
                selection: None,
                hovered: None,
                measure_start: None,
                measure_end: None,
                dirty: true,
                frame_pending: false,
            });
        }
        if screens.is_empty() {
            return None;
        }
        Some(Overlay {
            mode,
            screens,
            pool,
            dim,
            cross,
            strip_horizontal,
            strip_vertical,
            font: Font::ui(),
            active: 0,
            separator: region_label::decimal_separator(),
            finished: false,
        })
    }

    pub fn owns(&self, surface: &WlSurface) -> Option<usize> {
        self.screens.iter().position(|screen| screen.layer.wl_surface() == surface)
    }

    pub fn owns_layer(&self, layer: &LayerSurface) -> Option<usize> {
        self.screens.iter().position(|screen| &screen.layer == layer)
    }

    pub fn configure(&mut self, index: usize, size: (u32, u32), globals: &Globals, qh: &QueueHandle<App>) {
        let screen = &mut self.screens[index];
        if size.0 > 0 && size.1 > 0 {
            screen.frozen.size = (f64::from(size.0), f64::from(size.1));
        }
        let surface = screen.layer.wl_surface().clone();
        screen.viewport.set_destination(screen.frozen.size.0 as i32, screen.frozen.size.1 as i32);
        surface.attach(Some(&screen.buffer), 0, 0);
        surface.damage_buffer(0, 0, screen.frozen.width as i32, screen.frozen.height as i32);
        if screen.parts.is_none() {
            screen.parts = Some(Parts {
                dim: std::array::from_fn(|_| Part::new(globals, &surface, qh)),
                outline: std::array::from_fn(|_| Part::new(globals, &surface, qh)),
                cross: std::array::from_fn(|_| Part::new(globals, &surface, qh)),
                line: Part::new(globals, &surface, qh),
                label: Part::new(globals, &surface, qh),
                hint: Part::new(globals, &surface, qh),
                magnifier: Part::new(globals, &surface, qh),
            });
        }
        screen.dirty = true;
        self.render(index, qh);
    }

    pub fn set_scale(&mut self, surface: &WlSurface, scale: f64, qh: &QueueHandle<App>) {
        if let Some(index) = self.owns(surface) {
            self.screens[index].frozen.scale = scale;
            self.screens[index].dirty = true;
            self.render(index, qh);
        }
    }

    pub fn frame(&mut self, surface: &WlSurface, qh: &QueueHandle<App>) {
        if let Some(index) = self.owns(surface) {
            self.screens[index].frame_pending = false;
            if self.screens[index].dirty {
                self.render(index, qh);
            }
        }
    }

    fn invalidate(&mut self, index: usize, qh: &QueueHandle<App>) {
        self.screens[index].dirty = true;
        if !self.screens[index].frame_pending {
            self.render(index, qh);
        }
    }

    pub fn pointer_enter(&mut self, index: usize, position: (f64, f64), qh: &QueueHandle<App>) {
        self.active = index;
        self.pointer_motion(index, position, qh);
    }

    pub fn pointer_leave(&mut self, index: usize, qh: &QueueHandle<App>) {
        let screen = &mut self.screens[index];
        if screen.drag_origin.is_some() {
            return;
        }
        screen.hovered = None;
        screen.cursor = None;
        self.invalidate(index, qh);
    }

    pub fn pointer_motion(&mut self, index: usize, position: (f64, f64), qh: &QueueHandle<App>) {
        let mode = self.mode;
        let screen = &mut self.screens[index];
        let point = screen.clamp(position);
        screen.cursor = Some(point);
        if let Some(origin) = screen.drag_origin {
            let point = if mode == Mode::Measure { screen.snapped(point) } else { point };
            if mode == Mode::Measure {
                screen.measure_end = Some(point);
            }
            screen.selection = Some(Rect {
                x: origin.0.min(point.0),
                y: origin.1.min(point.1),
                width: (point.0 - origin.0).abs(),
                height: (point.1 - origin.1).abs(),
            });
        } else {
            screen.hovered = screen.windows.iter().find(|rect| rect.contains(point.0, point.1)).copied();
        }
        self.invalidate(index, qh);
    }

    pub fn press(&mut self, index: usize, button: u32, qh: &QueueHandle<App>) -> Option<Request> {
        self.active = index;
        let mode = self.mode;
        if button == 0x111 {
            let screen = &mut self.screens[index];
            if screen.selection.is_some() {
                screen.selection = None;
                screen.measure_start = None;
                screen.measure_end = None;
                screen.drag_origin = None;
                self.invalidate(index, qh);
                return None;
            }
            return self.cancel();
        }
        if button != 0x110 {
            return None;
        }
        if mode == Mode::Pick {
            return self.pick(index);
        }
        let screen = &mut self.screens[index];
        let point = screen.cursor.unwrap_or((0.0, 0.0));
        let origin = if mode == Mode::Measure { screen.snapped(point) } else { point };
        screen.drag_origin = Some(origin);
        screen.selection = None;
        screen.measure_start = (mode == Mode::Measure).then_some(origin);
        screen.measure_end = None;
        self.invalidate(index, qh);
        None
    }

    pub fn release(&mut self, index: usize, button: u32, qh: &QueueHandle<App>) -> Option<Request> {
        if button != 0x110 || self.mode == Mode::Pick {
            return None;
        }
        let screen = &mut self.screens[index];
        screen.drag_origin.take()?;
        if self.mode == Mode::Measure {
            if screen.selection.is_none() {
                screen.measure_start = None;
            }
            self.invalidate(index, qh);
            return None;
        }
        let rect = match screen.selection {
            Some(selection) if selection.width >= 3.0 && selection.height >= 3.0 => selection,
            _ => screen.hovered.unwrap_or(screen.bounds()),
        };
        self.complete(index, rect)
    }

    pub fn key(&mut self, keysym: u32, ctrl: bool) -> Option<Request> {
        const ESCAPE: u32 = 0xff1b;
        const RETURN: u32 = 0xff0d;
        const KP_ENTER: u32 = 0xff8d;
        match keysym {
            ESCAPE => self.cancel(),
            RETURN | KP_ENTER => {
                if self.mode != Mode::Select {
                    return self.cancel();
                }
                let screen = &self.screens[self.active];
                let rect = screen.selection.or(screen.hovered).unwrap_or(screen.bounds());
                self.complete(self.active, rect)
            }
            0x63 | 0x43 if ctrl && self.mode == Mode::Measure => {
                let text = self.measurement(self.active).map(|m| self.describe(&m, " · "))?;
                Some(Request::CopyText { title: "Medida copiada".into(), detail: text.clone(), text })
            }
            _ => None,
        }
    }

    fn pick(&mut self, index: usize) -> Option<Request> {
        let screen = &mut self.screens[index];
        let (x, y) = screen.cursor?;
        let ratio = screen.ratio();
        let [r, g, b] = screen.pixel((x * ratio).floor() as i64, (y * ratio).floor() as i64)?;
        let hex = format!("#{r:02X}{g:02X}{b:02X}");
        let detail = color_detail(r, g, b);
        self.finished = true;
        Some(Request::CopyText { title: format!("Color copiado: {hex}"), text: hex, detail })
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    fn complete(&mut self, index: usize, rect: Rect) -> Option<Request> {
        if self.finished {
            return None;
        }
        self.finished = true;
        let screen = &mut self.screens[index];
        let clipped = rect.integral().intersection(&screen.bounds())?;
        let image = screen.crop(clipped);
        let origin = screen.frozen.origin;
        Some(Request::Finish(Some(Selection {
            image,
            global: Rect { x: clipped.x + origin.0, y: clipped.y + origin.1, ..clipped },
            output: screen.frozen.name.clone(),
            ratio: screen.ratio(),
        })))
    }

    fn cancel(&mut self) -> Option<Request> {
        if self.finished {
            return None;
        }
        self.finished = true;
        Some(Request::Finish(None))
    }

    fn measurement(&self, index: usize) -> Option<Measurement> {
        let screen = &self.screens[index];
        let (start, end, selection) = (screen.measure_start?, screen.measure_end?, screen.selection?);
        let ratio = screen.ratio();
        let width = (selection.width * ratio).round() as i64;
        let height = (selection.height * ratio).round() as i64;
        Some(Measurement {
            width,
            height,
            diagonal: (width as f64).hypot(height as f64),
            angle: (-(end.1 - start.1)).atan2(end.0 - start.0).to_degrees(),
        })
    }

    fn describe(&self, measurement: &Measurement, separator: &str) -> String {
        let ratio = self.screens[self.active].ratio();
        let mut size = format!("{} × {} px", measurement.width, measurement.height);
        if (ratio - 1.0).abs() > 0.01 {
            size += &format!(
                " ({} × {} px lógicos)",
                format_number(measurement.width as f64 / ratio, self.separator),
                format_number(measurement.height as f64 / ratio, self.separator)
            );
        }
        let second = format!("Diagonal {} px · {}°", format_number(measurement.diagonal, self.separator), format_number(measurement.angle, self.separator));
        [size, second].join(separator)
    }

    fn render(&mut self, index: usize, qh: &QueueHandle<App>) {
        let measurement = self.measurement(index).map(|m| self.describe(&m, "\n"));
        let mode = self.mode;
        let Overlay { screens, pool, dim, cross, strip_horizontal, strip_vertical, font, .. } = self;
        let screen = &mut screens[index];
        let Some(parts) = screen.parts.as_mut() else { return };
        screen.dirty = false;
        let bounds = Rect { x: 0.0, y: 0.0, width: screen.frozen.size.0, height: screen.frozen.size.1 };
        let scale = screen.frozen.scale.max(1.0);
        let ratio = f64::from(screen.frozen.width) / screen.frozen.size.0;

        let active = match mode {
            Mode::Measure => screen.selection,
            Mode::Select => screen.selection.or(if screen.drag_origin.is_none() { screen.hovered } else { None }),
            Mode::Pick => None,
        }
        .map(|rect| rect.integral());

        if mode == Mode::Pick {
            for part in &mut parts.dim {
                part.hide();
            }
        } else {
            let hole = active.unwrap_or(Rect { x: 0.0, y: bounds.height, width: 0.0, height: 0.0 });
            let regions = [
                Rect { x: 0.0, y: 0.0, width: bounds.width, height: hole.y },
                Rect { x: 0.0, y: hole.y + hole.height, width: bounds.width, height: bounds.height - hole.y - hole.height },
                Rect { x: 0.0, y: hole.y, width: hole.x, height: hole.height },
                Rect { x: hole.x + hole.width, y: hole.y, width: bounds.width - hole.x - hole.width, height: hole.height },
            ];
            for (part, region) in parts.dim.iter_mut().zip(regions) {
                part.solid(dim, region);
            }
        }

        match (active, strip_horizontal.as_ref(), strip_vertical.as_ref()) {
            (Some(rect), Some(horizontal), Some(vertical)) if rect.width >= 1.0 && rect.height >= 1.0 => {
                let edges = [
                    (Rect { x: rect.x, y: rect.y, width: rect.width, height: 1.0 }, true),
                    (Rect { x: rect.x, y: rect.y + rect.height - 1.0, width: rect.width, height: 1.0 }, true),
                    (Rect { x: rect.x, y: rect.y, width: 1.0, height: rect.height }, false),
                    (Rect { x: rect.x + rect.width - 1.0, y: rect.y, width: 1.0, height: rect.height }, false),
                ];
                for (part, (edge, is_horizontal)) in parts.outline.iter_mut().zip(edges) {
                    part.strip(if is_horizontal { horizontal } else { vertical }, edge, is_horizontal);
                }
            }
            _ => parts.outline.iter_mut().for_each(Part::hide),
        }

        let label_text = match mode {
            Mode::Measure => measurement.clone(),
            Mode::Select => active.map(|rect| {
                region_label::text(
                    (rect.width * ratio).round() as u32,
                    (rect.height * ratio).round() as u32,
                    screen.frozen.width,
                    screen.frozen.height,
                    region_label::decimal_separator(),
                )
            }),
            Mode::Pick => None,
        };

        if let (Some(start), Some(end), Some(_)) = (screen.measure_start, screen.measure_end, measurement.as_ref()) {
            let left = start.0.min(end.0) - 4.0;
            let top = start.1.min(end.1) - 4.0;
            let width = (start.0 - end.0).abs() + 8.0;
            let height = (start.1 - end.1).abs() + 8.0;
            let line_scale = scale.min((4_000_000.0 / (width * height)).sqrt()).max(1.0);
            let size = ((width * line_scale).ceil() as u32, (height * line_scale).ceil() as u32);
            parts.line.image(pool, left, top, size, line_scale, |pixmap| {
                let transform = Transform::from_scale(line_scale as f32, line_scale as f32).pre_translate(-left as f32, -top as f32);
                let mut paint = Paint::default();
                paint.set_color(bgra(255, 214, 10, 255));
                paint.anti_alias = true;
                let mut builder = PathBuilder::new();
                builder.move_to(start.0 as f32, start.1 as f32);
                builder.line_to(end.0 as f32, end.1 as f32);
                if let Some(path) = builder.finish() {
                    pixmap.stroke_path(&path, &paint, &Stroke { width: 1.5, ..Default::default() }, transform, None);
                }
                for (x, y) in [start, end] {
                    if let Some(dot) = PathBuilder::from_circle(x as f32, y as f32, 2.5) {
                        pixmap.fill_path(&dot, &paint, FillRule::Winding, transform, None);
                    }
                }
            });
        } else {
            parts.line.hide();
        }

        match (label_text, font.as_ref()) {
            (Some(text), Some(font)) => {
                let label = draw::label(font, &text, 11.0, 4.0, scale);
                let logical = (f64::from(label.size.0) / scale, f64::from(label.size.1) / scale);
                let (x, y) = if mode == Mode::Measure {
                    let end = screen.measure_end.unwrap_or((0.0, 0.0));
                    let pointer = screen.cursor.unwrap_or(end);
                    let magnifier_right = pointer.0 + 24.0 + MAGNIFIER_SIZE <= bounds.width;
                    let magnifier_below = pointer.1 + 24.0 + MAGNIFIER_SIZE <= bounds.height;
                    let x = if magnifier_right { end.0 - logical.0 - 14.0 } else { end.0 + 14.0 };
                    let y = if magnifier_below { end.1 - logical.1 - 14.0 } else { end.1 + 14.0 };
                    (x.clamp(0.0, (bounds.width - logical.0).max(0.0)), y.clamp(0.0, (bounds.height - logical.1).max(0.0)))
                } else {
                    let rect = active.unwrap_or(bounds);
                    let mut y = rect.y - logical.1 - 4.0;
                    if y < 0.0 {
                        y = rect.y + 4.0;
                    }
                    (rect.x.min(bounds.width - logical.0), y)
                };
                parts.label.image(pool, x, y, label.size, scale, |pixmap| label.paint(pixmap));
            }
            _ => parts.label.hide(),
        }

        if mode == Mode::Measure && !parts.hint.mapped
            && let Some(font) = font.as_ref() {
                let label = draw::label(font, HINT, 11.0, 6.0, scale);
                let x = (bounds.width - f64::from(label.size.0) / scale) / 2.0;
                parts.hint.image(pool, x, 48.0, label.size, scale, |pixmap| label.paint(pixmap));
            }

        match screen.cursor {
            Some((x, y)) => {
                let (cx, cy) = (x.floor(), y.floor());
                parts.cross[0].solid(cross, Rect { x: 0.0, y: cy, width: bounds.width, height: 1.0 });
                parts.cross[1].solid(cross, Rect { x: cx, y: 0.0, width: 1.0, height: bounds.height });
                let side = (MAGNIFIER_SIZE * scale).round() as u32;
                let mut left = x + 24.0;
                let mut top = y + 24.0;
                if left + MAGNIFIER_SIZE > bounds.width {
                    left = x - 24.0 - MAGNIFIER_SIZE;
                }
                if top + MAGNIFIER_SIZE > bounds.height {
                    top = y - 24.0 - MAGNIFIER_SIZE;
                }
                let center = ((x * ratio).floor() as i64, (y * ratio).floor() as i64);
                let (width, height) = (screen.frozen.width as i64, screen.frozen.height as i64);
                let data = screen.frozen.pool.mmap();
                parts.magnifier.image(pool, left, top, (side, side), scale, |pixmap| {
                    draw::magnifier(pixmap, side, scale, |sx, sy| {
                        let px = center.0 - MAGNIFIER_PIXELS / 2 + sx;
                        let py = center.1 - MAGNIFIER_PIXELS / 2 + sy;
                        if px < 0 || py < 0 || px >= width || py >= height {
                            return [0, 0, 0];
                        }
                        let offset = (py as usize * width as usize + px as usize) * 3;
                        [data[offset], data[offset + 1], data[offset + 2]]
                    }, MAGNIFIER_PIXELS);
                });
            }
            None => {
                parts.cross.iter_mut().for_each(Part::hide);
                parts.magnifier.hide();
            }
        }

        let surface = screen.layer.wl_surface();
        surface.frame(qh, FrameCallbackData(surface.clone()));
        screen.frame_pending = true;
        surface.commit();
    }

    pub fn destroy(self) {
        for screen in self.screens {
            if let Some(parts) = screen.parts {
                let Parts { dim, outline, cross, line, label, hint, magnifier } = parts;
                for part in dim.into_iter().chain(outline).chain(cross).chain([line, label, hint, magnifier]) {
                    part.destroy();
                }
            }
            if let Some(fractional) = screen.fractional {
                fractional.destroy();
            }
            screen.viewport.destroy();
            screen.buffer.destroy();
            drop(screen.layer);
        }
        self.dim.destroy();
        self.cross.destroy();
    }
}

fn dashed_strip(pool: &mut SlotPool, length: i32, horizontal: bool) -> Option<Buffer> {
    let (width, height) = if horizontal { (length, 1) } else { (1, length) };
    let (buffer, canvas) = pool.create_buffer(width, height, width * 4, wl_shm::Format::Argb8888).ok()?;
    for (index, pixel) in canvas.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let color = if index % 9 < 5 { [255, 255, 255, 255] } else { [0, 0, 0, 204] };
        pixel.copy_from_slice(&color);
    }
    Some(buffer)
}

pub fn color_detail(r: u8, g: u8, b: u8) -> String {
    let (rf, gf, bf) = (f64::from(r) / 255.0, f64::from(g) / 255.0, f64::from(b) / 255.0);
    let max = rf.max(gf).max(bf);
    let min = rf.min(gf).min(bf);
    let delta = max - min;
    let hue = if delta == 0.0 {
        0.0
    } else if max == rf {
        60.0 * (((gf - bf) / delta).rem_euclid(6.0))
    } else if max == gf {
        60.0 * ((bf - rf) / delta + 2.0)
    } else {
        60.0 * ((rf - gf) / delta + 4.0)
    };
    let saturation = if max == 0.0 { 0.0 } else { delta / max };
    format!(
        "RGB {r}, {g}, {b} · HSB {}°, {} %, {} %",
        hue.round() as i64 % 360,
        (saturation * 100.0).round() as i64,
        (max * 100.0).round() as i64
    )
}
