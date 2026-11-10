use crate::app::App;
use crate::draw::{self, bgra};
use crate::windows::Rect;
use crate::wl::{Globals, PixelBuffer, WmBase};
use sharek_common::image::RgbImage;
use sharek_common::text::{Align, Font};
use smithay_client_toolkit::dispatch2::Dispatch2;
use smithay_client_toolkit::seat::pointer::{PointerEvent, PointerEventKind, BTN_LEFT, BTN_RIGHT};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerSurface};
use smithay_client_toolkit::shell::xdg::popup::Popup;
use smithay_client_toolkit::shell::xdg::XdgPositioner;
use smithay_client_toolkit::shm::raw::RawPool;
use smithay_client_toolkit::shm::slot::{Buffer, SlotPool};
use tiny_skia::{FillRule, PixmapMut, Transform};
use wayland_client::protocol::wl_callback::{self, WlCallback};
use wayland_client::protocol::{wl_buffer::WlBuffer, wl_output::WlOutput, wl_seat::WlSeat, wl_shm, wl_subsurface::WlSubsurface, wl_surface::WlSurface};
use wayland_client::{Connection, QueueHandle};
use wayland_protocols::wp::alpha_modifier::v1::client::wp_alpha_modifier_surface_v1::WpAlphaModifierSurfaceV1;
use wayland_protocols::wp::fractional_scale::v1::client::wp_fractional_scale_v1::WpFractionalScaleV1;
use wayland_protocols::wp::viewporter::client::wp_viewport::WpViewport;
use wayland_protocols::xdg::shell::client::xdg_positioner;

const MIN_WIDTH: f64 = 24.0;
const MAX_ZOOM: f64 = 8.0;
const MIN_OPACITY: f64 = 0.15;
const DOUBLE_CLICK_MS: u32 = 400;
const CASCADE: f64 = 24.0;
const KEY_ESCAPE: u32 = 0xff1b;
const KEY_BACKSPACE: u32 = 0xff08;
const KEY_DELETE: u32 = 0xffff;

#[derive(Clone)]
pub struct Placement {
    pub output: WlOutput,
    pub name: String,
    pub rect: Rect,
}

pub enum PinRequest {
    Copy(RgbImage, f64),
    Edit(RgbImage, f64),
    SaveAs(RgbImage, f64),
    Recognize(RgbImage, f64),
    Changed,
}

#[derive(Clone, Copy, PartialEq, Debug)]
struct Geometry {
    position: (f64, f64),
    size: (f64, f64),
}

impl Geometry {
    fn rounded(self) -> Geometry {
        Geometry {
            position: (self.position.0.round(), self.position.1.round()),
            size: (self.size.0.round().max(1.0), self.size.1.round().max(1.0)),
        }
    }
}

struct Border {
    surface: WlSurface,
    subsurface: WlSubsurface,
    viewport: WpViewport,
}

struct Surface {
    layer: LayerSurface,
    viewport: WpViewport,
    alpha: Option<WpAlphaModifierSurfaceV1>,
    borders: Vec<Border>,
    configured: bool,
}

impl Surface {
    fn new(placement: &Placement, geometry: Geometry, globals: &Globals, qh: &QueueHandle<App>) -> Surface {
        let surface = globals.compositor.create_surface(qh);
        let viewport = globals.viewport(&surface, qh);
        let alpha = globals.alpha_surface(&surface, qh);
        let borders = (0..4)
            .map(|_| {
                let (subsurface, child) = globals.subcompositor.create_subsurface(surface.clone(), qh);
                globals.no_input(&child);
                let viewport = globals.viewport(&child, qh);
                Border { surface: child, subsurface, viewport }
            })
            .collect();
        let layer = globals.layer_shell.create_layer_surface(qh, surface, Layer::Top, Some("sharek-pin"), Some(&placement.output));
        layer.set_anchor(Anchor::TOP | Anchor::LEFT);
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::OnDemand);
        layer.set_margin(geometry.position.1 as i32, 0, 0, geometry.position.0 as i32);
        layer.set_size(geometry.size.0 as u32, geometry.size.1 as u32);
        layer.commit();
        Surface { layer, viewport, alpha, borders, configured: false }
    }

    fn destroy(self) {
        for border in self.borders {
            border.viewport.destroy();
            border.subsurface.destroy();
            border.surface.destroy();
        }
        if let Some(alpha) = self.alpha {
            alpha.destroy();
        }
        self.viewport.destroy();
    }
}

pub struct Pin {
    id: u64,
    pool: RawPool,
    stride: usize,
    width: u32,
    height: u32,
    scale: f64,
    buffer: WlBuffer,
    surface: Option<Surface>,
    placement: Placement,
    target: Geometry,
    basis: Geometry,
    sent: Geometry,
    in_flight: bool,
    queued: bool,
    opacity: f64,
    grab: Option<(f64, f64)>,
    pointer: (f64, f64),
    last_click: Option<(u32, (f64, f64))>,
    enter_serial: u32,
}

impl Pin {
    fn base_size(&self) -> (f64, f64) {
        (f64::from(self.width) / self.scale, f64::from(self.height) / self.scale)
    }

    fn image(&mut self) -> RgbImage {
        let row = self.width as usize * 3;
        let mut data = Vec::with_capacity(row * self.height as usize);
        let mapped = self.pool.mmap();
        for y in 0..self.height as usize {
            data.extend_from_slice(&mapped[y * self.stride..y * self.stride + row]);
        }
        RgbImage { width: self.width, height: self.height, data: data.into() }
    }

    fn surface(&self) -> Option<&WlSurface> {
        self.surface.as_ref().map(|surface| surface.layer.wl_surface())
    }

    fn request(&mut self, geometry: Geometry, connection: &Connection, qh: &QueueHandle<App>) {
        self.target = geometry;
        if self.in_flight || !self.surface.as_ref().is_some_and(|surface| surface.configured) {
            self.queued = true;
            return;
        }
        self.commit(connection, qh);
    }

    fn commit(&mut self, connection: &Connection, qh: &QueueHandle<App>) {
        let Some(surface) = self.surface.as_ref() else { return };
        let geometry = self.target.rounded();
        let (x, y) = (geometry.position.0 as i32, geometry.position.1 as i32);
        let (width, height) = (geometry.size.0 as i32, geometry.size.1 as i32);
        surface.layer.set_margin(y, 0, 0, x);
        surface.layer.set_size(width as u32, height as u32);
        surface.viewport.set_destination(width, height);
        let edges = [(0, 0, width, 1), (0, height - 1, width, 1), (0, 0, 1, height), (width - 1, 0, 1, height)];
        for (border, (bx, by, bw, bh)) in surface.borders.iter().zip(edges) {
            border.subsurface.set_position(bx, by);
            border.viewport.set_destination(bw.max(1), bh.max(1));
            border.surface.commit();
        }
        let wl_surface = surface.layer.wl_surface();
        wl_surface.attach(Some(&self.buffer), 0, 0);
        wl_surface.damage_buffer(0, 0, self.width as i32, self.height as i32);
        wl_surface.commit();
        self.sent = geometry;
        self.in_flight = true;
        self.queued = false;
        connection.display().sync(qh, Moved(self.id));
    }

    fn settled(&mut self, connection: &Connection, qh: &QueueHandle<App>) {
        self.in_flight = false;
        self.basis = self.sent;
        if self.queued {
            self.commit(connection, qh);
        }
    }

    fn set_opacity(&mut self, opacity: f64) {
        self.opacity = opacity.clamp(MIN_OPACITY, 1.0);
        let Some(surface) = self.surface.as_ref() else { return };
        let multiplier = (self.opacity * f64::from(u32::MAX)) as u32;
        if let Some(alpha) = surface.alpha.as_ref() {
            alpha.set_multiplier(multiplier);
        }
        surface.layer.wl_surface().commit();
    }

    fn zoom(&mut self, factor: f64, anchor: (f64, f64), connection: &Connection, qh: &QueueHandle<App>) {
        let current = self.target;
        let base = self.base_size();
        let width = (current.size.0 * factor).clamp(MIN_WIDTH, base.0 * MAX_ZOOM);
        let height = width * base.1 / base.0;
        let absolute = (self.basis.position.0 + anchor.0, self.basis.position.1 + anchor.1);
        let relative = ((absolute.0 - current.position.0) / current.size.0, (absolute.1 - current.position.1) / current.size.1);
        let position = (absolute.0 - width * relative.0, absolute.1 - height * relative.1);
        self.request(Geometry { position, size: (width, height) }, connection, qh);
    }

    fn reset_size(&mut self, connection: &Connection, qh: &QueueHandle<App>) {
        let geometry = Geometry { position: self.target.position, size: self.base_size() };
        self.request(geometry, connection, qh);
    }

    fn destroy(self) {
        if let Some(surface) = self.surface {
            surface.destroy();
        }
        self.buffer.destroy();
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Command {
    Copy,
    Edit,
    SaveAs,
    Recognize,
    OriginalSize,
    Close,
    CloseAll,
}

enum Entry {
    Item(&'static str, Command),
    Opacity,
    Separator,
}

const ENTRIES: [Entry; 10] = [
    Entry::Item("Copiar", Command::Copy),
    Entry::Item("Abrir en el editor", Command::Edit),
    Entry::Item("Guardar como…", Command::SaveAs),
    Entry::Item("Extraer texto (OCR)", Command::Recognize),
    Entry::Separator,
    Entry::Opacity,
    Entry::Item("Tamaño original", Command::OriginalSize),
    Entry::Separator,
    Entry::Item("Cerrar", Command::Close),
    Entry::Item("Cerrar todas", Command::CloseAll),
];
const OPACITY_LEVELS: [u32; 4] = [100, 75, 50, 25];
const MENU_WIDTH: f64 = 280.0;
const ITEM_HEIGHT: f64 = 28.0;
const SEPARATOR_HEIGHT: f64 = 9.0;
const MENU_PADDING: f64 = 5.0;
const SEGMENT_WIDTH: f64 = 44.0;
const SEGMENT_GAP: f64 = 3.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Hit {
    Item(usize),
    Opacity(usize),
}

fn menu_height() -> f64 {
    2.0 * MENU_PADDING + ENTRIES.iter().map(|entry| if matches!(entry, Entry::Separator) { SEPARATOR_HEIGHT } else { ITEM_HEIGHT }).sum::<f64>()
}

fn segment_x(index: usize) -> f64 {
    let count = OPACITY_LEVELS.len() as f64;
    MENU_WIDTH - MENU_PADDING - 6.0 - count * SEGMENT_WIDTH - (count - 1.0) * SEGMENT_GAP + index as f64 * (SEGMENT_WIDTH + SEGMENT_GAP)
}

fn hit(x: f64, y: f64) -> Option<Hit> {
    if x < MENU_PADDING || x > MENU_WIDTH - MENU_PADDING {
        return None;
    }
    let mut top = MENU_PADDING;
    for (index, entry) in ENTRIES.iter().enumerate() {
        let height = if matches!(entry, Entry::Separator) { SEPARATOR_HEIGHT } else { ITEM_HEIGHT };
        if y >= top && y < top + height {
            return match entry {
                Entry::Item(..) => Some(Hit::Item(index)),
                Entry::Opacity => (0..OPACITY_LEVELS.len()).find(|&level| x >= segment_x(level) && x < segment_x(level) + SEGMENT_WIDTH).map(Hit::Opacity),
                Entry::Separator => None,
            };
        }
        top += height;
    }
    None
}

struct Menu {
    pin: u64,
    popup: Popup,
    fractional: Option<WpFractionalScaleV1>,
    viewport: WpViewport,
    pool: Option<SlotPool>,
    buffer: Option<Buffer>,
    scale: f64,
    hover: Option<Hit>,
    opacity: f64,
    font: Option<Font>,
    configured: bool,
}

impl Menu {
    fn render(&mut self) {
        if !self.configured {
            return;
        }
        let Some(pool) = self.pool.as_mut() else { return };
        let scale = self.scale.max(1.0);
        let (width, height) = ((MENU_WIDTH * scale).ceil() as u32, (menu_height() * scale).ceil() as u32);
        let Ok((buffer, canvas)) = pool.create_buffer(width as i32, height as i32, width as i32 * 4, wl_shm::Format::Argb8888) else { return };
        canvas.fill(0);
        if let Some(mut pixmap) = PixmapMut::from_bytes(canvas, width, height) {
            paint_menu(&mut pixmap, scale as f32, self.hover, self.opacity, self.font.as_ref());
        }
        self.viewport.set_destination(MENU_WIDTH as i32, menu_height().round() as i32);
        let surface = self.popup.wl_surface();
        let _ = buffer.attach_to(surface);
        surface.damage_buffer(0, 0, width as i32, height as i32);
        surface.commit();
        self.buffer = Some(buffer);
    }

    fn destroy(self) {
        if let Some(fractional) = self.fractional {
            fractional.destroy();
        }
        self.viewport.destroy();
    }
}

fn paint_menu(pixmap: &mut PixmapMut, scale: f32, hover: Option<Hit>, opacity: f64, font: Option<&Font>) {
    let (width, height) = (pixmap.width() as f32, pixmap.height() as f32);
    if let Some(background) = draw::rounded_rect(0.5 * scale, 0.5 * scale, width - scale, height - scale, 8.0 * scale) {
        pixmap.fill_path(&background, &draw::paint(bgra(41, 44, 48, 252)), FillRule::Winding, Transform::identity(), None);
        let stroke = tiny_skia::Stroke { width: scale, ..Default::default() };
        pixmap.stroke_path(&background, &draw::paint(bgra(63, 68, 75, 255)), &stroke, Transform::identity(), None);
    }
    let text = |pixmap: &mut PixmapMut, label: &str, x: f32, y: f32, size: f32, color: tiny_skia::Color, align: Align| {
        let Some(font) = font else { return };
        let layout = font.layout(label, size * scale, align);
        let left = match align {
            Align::Center => x - layout.width / 2.0,
            _ => x,
        };
        if let Some(path) = &layout.path {
            pixmap.fill_path(path, &draw::paint(color), FillRule::Winding, Transform::from_translate(left, y - layout.height / 2.0), None);
        }
    };
    let selected_level = OPACITY_LEVELS.iter().position(|&level| (opacity * 100.0).round() as u32 == level);
    let mut top = MENU_PADDING as f32;
    for (index, entry) in ENTRIES.iter().enumerate() {
        match entry {
            Entry::Separator => {
                let y = (top + SEPARATOR_HEIGHT as f32 / 2.0) * scale;
                pixmap.fill_rect(
                    tiny_skia::Rect::from_xywh(12.0 * scale, y.floor(), width - 24.0 * scale, scale.max(1.0)).unwrap_or(tiny_skia::Rect::from_xywh(0.0, 0.0, 1.0, 1.0).unwrap()),
                    &draw::paint(bgra(63, 68, 75, 255)),
                    Transform::identity(),
                    None,
                );
                top += SEPARATOR_HEIGHT as f32;
            }
            Entry::Item(label, _) => {
                if hover == Some(Hit::Item(index))
                    && let Some(highlight) = draw::rounded_rect(MENU_PADDING as f32 * scale, top * scale, width - 2.0 * MENU_PADDING as f32 * scale, ITEM_HEIGHT as f32 * scale, 5.0 * scale) {
                        pixmap.fill_path(&highlight, &draw::paint(bgra(61, 174, 233, 80)), FillRule::Winding, Transform::identity(), None);
                    }
                text(pixmap, label, 14.0 * scale, (top + ITEM_HEIGHT as f32 / 2.0) * scale, 13.0, bgra(239, 240, 241, 255), Align::Left);
                top += ITEM_HEIGHT as f32;
            }
            Entry::Opacity => {
                let middle = (top + ITEM_HEIGHT as f32 / 2.0) * scale;
                text(pixmap, "Opacidad", 14.0 * scale, middle, 13.0, bgra(239, 240, 241, 255), Align::Left);
                for (level, value) in OPACITY_LEVELS.iter().enumerate() {
                    let x = segment_x(level) as f32 * scale;
                    let y = (top + 4.0) * scale;
                    let (w, h) = (SEGMENT_WIDTH as f32 * scale, (ITEM_HEIGHT as f32 - 8.0) * scale);
                    let selected = selected_level == Some(level);
                    let hovered = hover == Some(Hit::Opacity(level));
                    let fill = if selected {
                        bgra(61, 174, 233, 255)
                    } else if hovered {
                        bgra(61, 174, 233, 80)
                    } else {
                        bgra(255, 255, 255, 18)
                    };
                    if let Some(segment) = draw::rounded_rect(x, y, w, h, 4.0 * scale) {
                        pixmap.fill_path(&segment, &draw::paint(fill), FillRule::Winding, Transform::identity(), None);
                    }
                    let color = if selected { bgra(255, 255, 255, 255) } else { bgra(195, 200, 206, 255) };
                    text(pixmap, &format!("{value} %"), x + w / 2.0, middle, 11.5, color, Align::Center);
                }
                top += ITEM_HEIGHT as f32;
            }
        }
    }
}

pub struct Pins {
    pins: Vec<Pin>,
    menu: Option<Menu>,
    border: Option<WlBuffer>,
    next_id: u64,
    pinching: Option<(u64, f64)>,
}

pub struct Moved(u64);

fn double_click_interval() -> u32 {
    let kdeglobals = sharek_common::paths::config_dir().parent().map(|config| config.join("kdeglobals"));
    kdeglobals.and_then(|path| std::fs::read_to_string(path).ok()).as_deref().and_then(configured_interval).unwrap_or(DOUBLE_CLICK_MS)
}

fn configured_interval(kdeglobals: &str) -> Option<u32> {
    let mut in_kde = false;
    for line in kdeglobals.lines().map(str::trim) {
        if line.starts_with('[') {
            in_kde = line == "[KDE]";
        } else if in_kde && let Some(value) = line.strip_prefix("DoubleClickInterval=") {
            return value.trim().parse().ok();
        }
    }
    None
}

impl Pins {
    pub fn new() -> Pins {
        Pins { pins: Vec::new(), menu: None, border: None, next_id: 0, pinching: None }
    }

    pub fn is_empty(&self) -> bool {
        self.pins.is_empty()
    }

    pub fn pinch_begin(&mut self, surface: &WlSurface) {
        self.pinching = self.pins.iter().find(|pin| pin.surface() == Some(surface)).map(|pin| (pin.id, 1.0));
    }

    pub fn pinch_update(&mut self, scale: f64, connection: &Connection, qh: &QueueHandle<App>) {
        let Some((id, last)) = self.pinching else { return };
        if scale <= 0.0 || last <= 0.0 {
            return;
        }
        if let Some(pin) = self.pins.iter_mut().find(|pin| pin.id == id) {
            let anchor = pin.pointer;
            pin.zoom(scale / last, anchor, connection, qh);
        }
        self.pinching = Some((id, scale));
    }

    pub fn pinch_end(&mut self) {
        self.pinching = None;
    }

    pub fn add(&mut self, image: RgbImage, scale: f64, placement: Placement, at: Option<Rect>, globals: &Globals, qh: &QueueHandle<App>) -> Result<(), String> {
        let (width, height) = (image.width, image.height);
        if width == 0 || height == 0 {
            return Err("La imagen está vacía".into());
        }
        let stride = (width as usize * 3 + 3) & !3;
        let mut pool = RawPool::new(stride * height as usize, &globals.shm).map_err(|error| error.to_string())?;
        {
            let mapped = pool.mmap();
            let row = width as usize * 3;
            for y in 0..height as usize {
                mapped[y * stride..y * stride + row].copy_from_slice(&image.data[y * row..(y + 1) * row]);
            }
        }
        drop(image);
        let buffer = pool.create_buffer(0, width as i32, height as i32, stride as i32, wl_shm::Format::Bgr888, PixelBuffer, qh);
        let scale = scale.max(0.1);
        let base = (f64::from(width) / scale, f64::from(height) / scale);
        let area = placement.rect;
        let geometry = match at {
            Some(rect) => Geometry { position: (rect.x - area.x, rect.y - area.y), size: (rect.width, rect.height) },
            None => {
                let fit = 1f64.min(area.width * 0.7 / base.0).min(area.height * 0.7 / base.1);
                let size = (base.0 * fit, base.1 * fit);
                let offset = (self.pins.len() % 8) as f64 * CASCADE;
                Geometry { position: ((area.width - size.0) / 2.0 + offset, (area.height - size.1) / 2.0 + offset), size }
            }
        }
        .rounded();
        if self.border.is_none() {
            self.border = Some(globals.solid([0.5, 0.5, 0.5, 0.45], qh));
        }
        self.next_id += 1;
        let surface = Surface::new(&placement, geometry, globals, qh);
        self.pins.push(Pin {
            id: self.next_id,
            pool,
            stride,
            width,
            height,
            scale,
            buffer,
            surface: Some(surface),
            placement,
            target: geometry,
            basis: geometry,
            sent: geometry,
            in_flight: false,
            queued: false,
            opacity: 1.0,
            grab: None,
            pointer: (0.0, 0.0),
            last_click: None,
            enter_serial: 0,
        });
        Ok(())
    }

    pub fn owns_layer(&self, layer: &LayerSurface) -> bool {
        self.pins.iter().any(|pin| pin.surface.as_ref().is_some_and(|surface| &surface.layer == layer))
    }

    pub fn owns(&self, surface: &WlSurface) -> bool {
        self.pins.iter().any(|pin| pin.surface() == Some(surface)) || self.menu.as_ref().is_some_and(|menu| menu.popup.wl_surface() == surface)
    }

    pub fn configure(&mut self, layer: &LayerSurface, connection: &Connection, qh: &QueueHandle<App>) {
        let Some(pin) = self.pins.iter_mut().find(|pin| pin.surface.as_ref().is_some_and(|surface| &surface.layer == layer)) else { return };
        let Some(surface) = pin.surface.as_mut() else { return };
        if surface.configured {
            surface.layer.wl_surface().commit();
            return;
        }
        surface.configured = true;
        if let Some(border) = self.border.as_ref() {
            for edge in &surface.borders {
                edge.surface.attach(Some(border), 0, 0);
                edge.surface.damage_buffer(0, 0, 1, 1);
            }
        }
        if let Some(alpha) = surface.alpha.as_ref() {
            alpha.set_multiplier((pin.opacity * f64::from(u32::MAX)) as u32);
        }
        pin.in_flight = false;
        pin.commit(connection, qh);
    }

    pub fn closed(&mut self, layer: &LayerSurface) -> bool {
        let Some(index) = self.pins.iter().position(|pin| pin.surface.as_ref().is_some_and(|surface| &surface.layer == layer)) else { return false };
        self.close(index);
        true
    }

    pub fn settled(&mut self, id: u64, connection: &Connection, qh: &QueueHandle<App>) {
        if let Some(pin) = self.pins.iter_mut().find(|pin| pin.id == id) {
            pin.settled(connection, qh);
        }
    }

    pub fn set_scale(&mut self, surface: &WlSurface, scale: f64) {
        if let Some(menu) = self.menu.as_mut().filter(|menu| menu.popup.wl_surface() == surface) {
            menu.scale = scale;
            menu.render();
        }
    }

    fn close(&mut self, index: usize) {
        let pin = self.pins.remove(index);
        if self.menu.as_ref().is_some_and(|menu| menu.pin == pin.id)
            && let Some(menu) = self.menu.take() {
                menu.destroy();
            }
        pin.destroy();
    }

    pub fn close_all(&mut self) {
        if let Some(menu) = self.menu.take() {
            menu.destroy();
        }
        while !self.pins.is_empty() {
            self.close(self.pins.len() - 1);
        }
    }

    pub fn popup_configure(&mut self, popup: &Popup) {
        if let Some(menu) = self.menu.as_mut().filter(|menu| &menu.popup == popup) {
            menu.configured = true;
            menu.render();
        }
    }

    pub fn popup_done(&mut self, popup: &Popup) {
        if self.menu.as_ref().is_some_and(|menu| &menu.popup == popup)
            && let Some(menu) = self.menu.take() {
                menu.destroy();
            }
    }

    fn open_menu(&mut self, index: usize, seat: Option<&WlSeat>, serial: u32, xdg: &WmBase, globals: &Globals, qh: &QueueHandle<App>) {
        if let Some(menu) = self.menu.take() {
            menu.destroy();
        }
        let pin = &self.pins[index];
        let Some(surface) = pin.surface.as_ref() else { return };
        let Ok(positioner) = XdgPositioner::new(xdg) else { return };
        positioner.set_size(MENU_WIDTH as i32, menu_height().round() as i32);
        positioner.set_anchor_rect(pin.pointer.0.round() as i32, pin.pointer.1.round() as i32, 1, 1);
        positioner.set_anchor(xdg_positioner::Anchor::BottomRight);
        positioner.set_gravity(xdg_positioner::Gravity::BottomRight);
        positioner.set_constraint_adjustment(
            xdg_positioner::ConstraintAdjustment::SlideX
                | xdg_positioner::ConstraintAdjustment::SlideY
                | xdg_positioner::ConstraintAdjustment::FlipX
                | xdg_positioner::ConstraintAdjustment::FlipY,
        );
        let wl_surface = globals.compositor.create_surface(qh);
        let viewport = globals.viewport(&wl_surface, qh);
        let fractional = globals.fractional_scale(&wl_surface, qh);
        let Ok(popup) = Popup::from_surface(None, &positioner, qh, wl_surface, xdg) else { return };
        surface.layer.get_popup(popup.xdg_popup());
        if let Some(seat) = seat {
            popup.xdg_popup().grab(seat, serial);
        }
        popup.wl_surface().commit();
        self.menu = Some(Menu {
            pin: pin.id,
            popup,
            fractional,
            viewport,
            pool: SlotPool::new(256 << 10, &globals.shm).ok(),
            buffer: None,
            scale: pin.scale,
            hover: None,
            opacity: pin.opacity,
            font: Font::ui(),
            configured: false,
        });
    }

    fn command(&mut self, id: u64, command: Command, connection: &Connection, qh: &QueueHandle<App>) -> Option<PinRequest> {
        let index = self.pins.iter().position(|pin| pin.id == id)?;
        let pin = &mut self.pins[index];
        match command {
            Command::Copy => Some(PinRequest::Copy(pin.image(), pin.scale)),
            Command::Edit => Some(PinRequest::Edit(pin.image(), pin.scale)),
            Command::SaveAs => Some(PinRequest::SaveAs(pin.image(), pin.scale)),
            Command::Recognize => Some(PinRequest::Recognize(pin.image(), pin.scale)),
            Command::OriginalSize => {
                pin.reset_size(connection, qh);
                None
            }
            Command::Close => {
                self.close(index);
                Some(PinRequest::Changed)
            }
            Command::CloseAll => {
                self.close_all();
                Some(PinRequest::Changed)
            }
        }
    }

    fn menu_pointer(&mut self, event: &PointerEvent, connection: &Connection, qh: &QueueHandle<App>) -> Option<PinRequest> {
        let menu = self.menu.as_mut()?;
        let (x, y) = event.position;
        match event.kind {
            PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                let hover = hit(x, y);
                if hover != menu.hover {
                    menu.hover = hover;
                    menu.render();
                }
                None
            }
            PointerEventKind::Leave { .. } => {
                if menu.hover.take().is_some() {
                    menu.render();
                }
                None
            }
            PointerEventKind::Release { button: BTN_LEFT | BTN_RIGHT, .. } => {
                let pin = menu.pin;
                match hit(x, y)? {
                    Hit::Opacity(level) => {
                        let opacity = f64::from(OPACITY_LEVELS[level]) / 100.0;
                        menu.opacity = opacity;
                        menu.render();
                        if let Some(target) = self.pins.iter_mut().find(|candidate| candidate.id == pin) {
                            target.set_opacity(opacity);
                        }
                        None
                    }
                    Hit::Item(index) => {
                        let Entry::Item(_, command) = ENTRIES[index] else { return None };
                        if let Some(menu) = self.menu.take() {
                            menu.destroy();
                        }
                        self.command(pin, command, connection, qh)
                    }
                }
            }
            _ => None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn pointer(
        &mut self,
        event: &PointerEvent,
        alt: bool,
        seat: Option<&WlSeat>,
        xdg: Option<&WmBase>,
        placements: impl FnOnce() -> Vec<Placement>,
        connection: &Connection,
        globals: &Globals,
        qh: &QueueHandle<App>,
    ) -> (Option<PinRequest>, Option<(u32, bool)>) {
        if self.menu.as_ref().is_some_and(|menu| menu.popup.wl_surface() == &event.surface) {
            return (self.menu_pointer(event, connection, qh), None);
        }
        let Some(index) = self.pins.iter().position(|pin| pin.surface() == Some(&event.surface)) else { return (None, None) };
        let pin = &mut self.pins[index];
        pin.pointer = event.position;
        match event.kind {
            PointerEventKind::Enter { serial } => {
                pin.enter_serial = serial;
                (None, Some((serial, false)))
            }
            PointerEventKind::Leave { .. } => (None, None),
            PointerEventKind::Motion { .. } => {
                if let Some(offset) = pin.grab {
                    let absolute = (pin.basis.position.0 + event.position.0, pin.basis.position.1 + event.position.1);
                    let area = pin.placement.rect;
                    let size = pin.target.size;
                    let position = (
                        (absolute.0 - offset.0).clamp(MIN_WIDTH - size.0, area.width - MIN_WIDTH),
                        (absolute.1 - offset.1).clamp(MIN_WIDTH - size.1, area.height - MIN_WIDTH),
                    );
                    pin.request(Geometry { position, size }, connection, qh);
                }
                (None, None)
            }
            PointerEventKind::Press { button: BTN_LEFT, time, .. } => {
                let interval = double_click_interval();
                let double = pin.last_click.is_some_and(|(last, at)| time.wrapping_sub(last) < interval && (at.0 - event.position.0).abs() < 4.0 && (at.1 - event.position.1).abs() < 4.0);
                if double {
                    self.close(index);
                    return (Some(PinRequest::Changed), None);
                }
                pin.last_click = Some((time, event.position));
                pin.grab = Some(event.position);
                (None, Some((pin.enter_serial, true)))
            }
            PointerEventKind::Release { button: BTN_LEFT, .. } => {
                let Some(offset) = pin.grab.take() else { return (None, None) };
                let serial = pin.enter_serial;
                let area = pin.placement.rect;
                let global = (area.x + pin.basis.position.0 + event.position.0, area.y + pin.basis.position.1 + event.position.1);
                if !area.contains(global.0, global.1)
                    && let Some(destination) = placements().into_iter().find(|placement| placement.rect.contains(global.0, global.1)) {
                        let geometry = Geometry {
                            position: (global.0 - destination.rect.x - offset.0, global.1 - destination.rect.y - offset.1),
                            size: pin.target.size,
                        }
                        .rounded();
                        if let Some(surface) = pin.surface.take() {
                            surface.destroy();
                        }
                        pin.surface = Some(Surface::new(&destination, geometry, globals, qh));
                        pin.placement = destination;
                        pin.target = geometry;
                        pin.basis = geometry;
                        pin.sent = geometry;
                        pin.in_flight = false;
                        pin.queued = false;
                    }
                (None, Some((serial, false)))
            }
            PointerEventKind::Press { button: BTN_RIGHT, serial, .. } => {
                if let Some(xdg) = xdg {
                    self.open_menu(index, seat, serial, xdg, globals, qh);
                }
                (None, None)
            }
            PointerEventKind::Axis { vertical, .. } => {
                let delta = if vertical.value120 != 0 {
                    -f64::from(vertical.value120) / 120.0 * 0.05
                } else if vertical.discrete != 0 {
                    -f64::from(vertical.discrete) * 0.05
                } else {
                    -vertical.absolute * 0.005
                };
                if delta != 0.0 {
                    if alt {
                        let opacity = pin.opacity + delta;
                        pin.set_opacity(opacity);
                    } else {
                        pin.zoom(1.0 + delta, event.position, connection, qh);
                    }
                }
                (None, None)
            }
            _ => (None, None),
        }
    }

    pub fn key(&mut self, surface: &WlSurface, keysym: u32) -> Option<PinRequest> {
        if self.menu.as_ref().is_some_and(|menu| menu.popup.wl_surface() == surface) {
            if keysym == KEY_ESCAPE
                && let Some(menu) = self.menu.take() {
                    menu.destroy();
                }
            return None;
        }
        let index = self.pins.iter().position(|pin| pin.surface() == Some(surface))?;
        if matches!(keysym, KEY_ESCAPE | KEY_BACKSPACE | KEY_DELETE) {
            self.close(index);
            return Some(PinRequest::Changed);
        }
        None
    }
}

impl Dispatch2<WlCallback, App> for Moved {
    fn event(&self, app: &mut App, _: &WlCallback, event: wl_callback::Event, _: &Connection, qh: &QueueHandle<App>) {
        if let wl_callback::Event::Done { .. } = event {
            app.pins.settled(self.0, &app.connection, qh);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_double_click_interval_comes_from_the_kde_section() {
        assert_eq!(configured_interval("[General]\nDoubleClickInterval=900\n[KDE]\nDoubleClickInterval=250\n"), Some(250));
        assert_eq!(configured_interval("[KDE]\nSingleClick=false\n"), None);
    }

    #[test]
    fn menu_clicks_map_to_entries_and_opacity_segments() {
        let opacity_middle = MENU_PADDING + 4.0 * ITEM_HEIGHT + SEPARATOR_HEIGHT + ITEM_HEIGHT / 2.0;
        assert_eq!(hit(20.0, MENU_PADDING + 1.0), Some(Hit::Item(0)));
        assert_eq!(hit(20.0, MENU_PADDING + 4.0 * ITEM_HEIGHT + 2.0), None);
        assert_eq!(hit(segment_x(2) + 5.0, opacity_middle), Some(Hit::Opacity(2)));
        assert_eq!(hit(20.0, opacity_middle), None);
        assert_eq!(hit(20.0, menu_height() - MENU_PADDING - 1.0), Some(Hit::Item(ENTRIES.len() - 1)));
        assert_eq!(hit(MENU_WIDTH - 1.0, MENU_PADDING + 1.0), None);
    }
}
