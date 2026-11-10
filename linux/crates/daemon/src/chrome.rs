use crate::app::App;
use crate::draw::{self, bgra};
use crate::windows::Rect;
use crate::wl::Globals;
use sharek_common::text::{Align, Font, Layout};
use smithay_client_toolkit::dispatch2::Dispatch2;
use smithay_client_toolkit::seat::pointer::{PointerEvent, PointerEventKind, BTN_LEFT};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerSurface};
use smithay_client_toolkit::shm::slot::{Buffer, SlotPool};
use std::time::Instant;
use tiny_skia::{Color, FillRule, PathBuilder, PixmapMut, Stroke, StrokeDash, Transform};
use wayland_client::protocol::wl_callback::{self, WlCallback};
use wayland_client::protocol::{wl_buffer::WlBuffer, wl_output::WlOutput, wl_shm, wl_surface::WlSurface};
use wayland_client::{Connection, QueueHandle};
use wayland_protocols::wp::fractional_scale::v1::client::wp_fractional_scale_v1::WpFractionalScaleV1;
use wayland_protocols::wp::viewporter::client::wp_viewport::WpViewport;

pub const OUTSET: f64 = 3.0;
const PANEL_HEIGHT: f64 = 40.0;
const PANEL_PADDING: f64 = 12.0;
const PANEL_GAP: f64 = 12.0;
const SCREEN_MARGIN: f64 = 8.0;
const SPACING: f64 = 8.0;
const DOT: f64 = 10.0;
const BUTTON_HEIGHT: f64 = 22.0;
const BUTTON_PADDING: f64 = 10.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChromeAction {
    Manual,
    Stop,
    Cancel,
}

struct Piece {
    layer: LayerSurface,
    viewport: WpViewport,
    fractional: Option<WpFractionalScaleV1>,
    size: (u32, u32),
    scale: f64,
    configured: bool,
    buffer: Option<Buffer>,
}

impl Piece {
    fn new(output: &WlOutput, origin: (f64, f64), position: (f64, f64), size: (u32, u32), input: bool, globals: &Globals, qh: &QueueHandle<App>) -> Piece {
        let surface = globals.compositor.create_surface(qh);
        if !input {
            globals.no_input(&surface);
        }
        let viewport = globals.viewport(&surface, qh);
        let fractional = globals.fractional_scale(&surface, qh);
        let layer = globals.layer_shell.create_layer_surface(qh, surface, Layer::Overlay, Some("sharek-recording"), Some(output));
        layer.set_anchor(Anchor::TOP | Anchor::LEFT);
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_margin((position.1 - origin.1).round() as i32, 0, 0, (position.0 - origin.0).round() as i32);
        layer.set_size(size.0, size.1);
        layer.commit();
        Piece { layer, viewport, fractional, size, scale: 1.0, configured: false, buffer: None }
    }

    fn surface(&self) -> &WlSurface {
        self.layer.wl_surface()
    }

    fn present(&mut self, pool: &mut SlotPool, paint: impl FnOnce(&mut PixmapMut, f32)) {
        if !self.configured {
            return;
        }
        let scale = self.scale.max(1.0);
        let (width, height) = ((f64::from(self.size.0) * scale).ceil() as u32, (f64::from(self.size.1) * scale).ceil() as u32);
        let Ok((buffer, canvas)) = pool.create_buffer(width as i32, height as i32, width as i32 * 4, wl_shm::Format::Argb8888) else { return };
        canvas.fill(0);
        if let Some(mut pixmap) = PixmapMut::from_bytes(canvas, width, height) {
            paint(&mut pixmap, scale as f32);
        }
        self.viewport.set_destination(self.size.0 as i32, self.size.1 as i32);
        let surface = self.layer.wl_surface();
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

struct Guard {
    layer: LayerSurface,
    viewport: WpViewport,
    pixel: WlBuffer,
    size: (u32, u32),
}

impl Guard {
    fn new(output: &WlOutput, screen: Rect, region: Rect, globals: &Globals, qh: &QueueHandle<App>) -> Guard {
        let surface = globals.compositor.create_surface(qh);
        let size = (screen.width.round().max(1.0) as u32, screen.height.round().max(1.0) as u32);
        let hole = screen.intersection(&region).map(|hole| {
            ((hole.x - screen.x).round() as i32, (hole.y - screen.y).round() as i32, hole.width.round() as i32, hole.height.round() as i32)
        });
        globals.input_outside(&surface, size, hole);
        let viewport = globals.viewport(&surface, qh);
        let layer = globals.layer_shell.create_layer_surface(qh, surface, Layer::Top, Some("sharek-scroll-guard"), Some(output));
        layer.set_anchor(Anchor::TOP | Anchor::LEFT);
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_size(size.0, size.1);
        layer.commit();
        Guard { layer, viewport, pixel: globals.solid([0.0; 4], qh), size }
    }

    fn configure(&self) {
        self.viewport.set_destination(self.size.0 as i32, self.size.1 as i32);
        let surface = self.layer.wl_surface();
        surface.attach(Some(&self.pixel), 0, 0);
        surface.damage_buffer(0, 0, 1, 1);
        surface.commit();
    }

    fn destroy(self) {
        self.viewport.destroy();
        self.pixel.destroy();
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Button {
    Manual,
    Stop,
    Cancel,
}

impl Button {
    fn label(self) -> &'static str {
        match self {
            Button::Manual => "Manual",
            Button::Stop => "Detener",
            Button::Cancel => "Cancelar",
        }
    }
}

pub struct Chrome {
    strips: Vec<(Piece, bool)>,
    panel: Piece,
    guards: Vec<Guard>,
    pool: Option<SlotPool>,
    ui: Option<Font>,
    bold: Option<Font>,
    title: &'static str,
    offers_manual: bool,
    width: f64,
    started: Instant,
    processing: bool,
    hover: Option<Button>,
    pressed: Option<Button>,
    buttons: Vec<(Button, f64, f64)>,
    screen: Rect,
    basis: (f64, f64),
    sent: (f64, f64),
    target: (f64, f64),
    in_flight: bool,
    queued: bool,
    grab: Option<(f64, f64)>,
}

pub struct PanelMoved;

impl Dispatch2<WlCallback, App> for PanelMoved {
    fn event(&self, app: &mut App, _: &WlCallback, event: wl_callback::Event, connection: &Connection, qh: &QueueHandle<App>) {
        if let wl_callback::Event::Done { .. } = event
            && let Some(chrome) = app.chrome() {
                chrome.settled(connection, qh);
            }
    }
}

pub fn panel_position(region: Rect, screen: Rect, width: f64) -> (f64, f64) {
    let height = PANEL_HEIGHT;
    let center = region.x + region.width / 2.0 - width / 2.0;
    let x = center.min(screen.x + screen.width - width - SCREEN_MARGIN).max(screen.x + SCREEN_MARGIN);
    let below = region.y + region.height + PANEL_GAP;
    if below + height <= screen.y + screen.height {
        return (x, below);
    }
    let above = region.y - PANEL_GAP - height;
    if above >= screen.y {
        return (x, above);
    }
    (x, (region.y + region.height).min(screen.y + screen.height) - PANEL_GAP - height)
}

pub fn strips(region: Rect, screen: Rect) -> Vec<(Rect, bool)> {
    let (screen_right, screen_bottom) = (screen.x + screen.width, screen.y + screen.height);
    let (region_right, region_bottom) = (region.x + region.width, region.y + region.height);
    let left = (region.x - OUTSET).max(screen.x);
    let width = (region_right + OUTSET).min(screen_right) - left;
    let mut placed = Vec::new();
    if region.y - OUTSET >= screen.y {
        placed.push((Rect { x: left, y: region.y - OUTSET, width, height: OUTSET }, true));
    }
    if region_bottom + OUTSET <= screen_bottom {
        placed.push((Rect { x: left, y: region_bottom, width, height: OUTSET }, true));
    }
    if region.x - OUTSET >= screen.x {
        placed.push((Rect { x: region.x - OUTSET, y: region.y, width: OUTSET, height: region.height }, false));
    }
    if region_right + OUTSET <= screen_right {
        placed.push((Rect { x: region_right, y: region.y, width: OUTSET, height: region.height }, false));
    }
    placed
}

fn clock(seconds: u64) -> String {
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

struct PanelLayout {
    label: Option<Layout>,
    time: Option<Layout>,
    buttons: Vec<(Button, Option<Layout>)>,
    widths: Vec<f64>,
}

impl PanelLayout {
    fn total(&self) -> f64 {
        self.widths.iter().sum::<f64>() + SPACING * (self.widths.len() - 1) as f64
    }
}

impl Chrome {
    pub fn new(output: &WlOutput, screen: Rect, region: Rect, title: &'static str, offers_manual: bool, globals: &Globals, qh: &QueueHandle<App>) -> Chrome {
        let origin = (screen.x, screen.y);
        let strips = strips(region, screen)
            .into_iter()
            .map(|(rect, horizontal)| (Piece::new(output, origin, (rect.x, rect.y), (rect.width.round().max(1.0) as u32, rect.height.round().max(1.0) as u32), false, globals, qh), horizontal))
            .collect();
        let mut chrome = Chrome {
            strips,
            panel: Piece::new(output, origin, (screen.x, screen.y), (1, 1), true, globals, qh),
            guards: Vec::new(),
            pool: SlotPool::new(64 << 10, &globals.shm).ok(),
            ui: Font::ui(),
            bold: Font::bold(),
            title,
            offers_manual,
            width: 0.0,
            started: Instant::now(),
            processing: false,
            hover: None,
            pressed: None,
            buttons: Vec::new(),
            screen,
            basis: (0.0, 0.0),
            sent: (0.0, 0.0),
            target: (0.0, 0.0),
            in_flight: false,
            queued: false,
            grab: None,
        };
        chrome.width = (chrome.layout(1.0).total() + 2.0 * PANEL_PADDING).ceil();
        let (x, y) = panel_position(region, screen, chrome.width);
        let position = ((x - origin.0).round(), (y - origin.1).round());
        chrome.basis = position;
        chrome.sent = position;
        chrome.target = position;
        chrome.panel.size = (chrome.width as u32, PANEL_HEIGHT as u32);
        chrome.panel.layer.set_size(chrome.panel.size.0, chrome.panel.size.1);
        chrome.panel.layer.set_margin(position.1 as i32, 0, 0, position.0 as i32);
        chrome.panel.layer.commit();
        chrome
    }

    pub fn guard<'a>(&mut self, screens: impl Iterator<Item = (&'a WlOutput, Rect)>, region: Rect, globals: &Globals, qh: &QueueHandle<App>) {
        self.guards = screens.map(|(output, screen)| Guard::new(output, screen, region, globals, qh)).collect();
    }

    pub fn release_guards(&mut self) {
        for guard in self.guards.drain(..) {
            guard.destroy();
        }
    }

    pub fn set_title(&mut self, title: &'static str, offers_manual: bool) {
        if self.offers_manual != offers_manual {
            self.offers_manual = offers_manual;
            self.hover = None;
            self.pressed = None;
        } else if self.title == title {
            return;
        }
        self.title = title;
        self.render_panel();
    }

    pub fn owns_layer(&self, layer: &LayerSurface) -> bool {
        &self.panel.layer == layer || self.strips.iter().any(|(piece, _)| &piece.layer == layer) || self.guards.iter().any(|guard| &guard.layer == layer)
    }

    pub fn owns(&self, surface: &WlSurface) -> bool {
        self.panel.surface() == surface || self.strips.iter().any(|(piece, _)| piece.surface() == surface) || self.guards.iter().any(|guard| guard.layer.wl_surface() == surface)
    }

    pub fn configure(&mut self, layer: &LayerSurface) {
        if let Some(guard) = self.guards.iter().find(|guard| &guard.layer == layer) {
            guard.configure();
            return;
        }
        if &self.panel.layer == layer {
            self.panel.configured = true;
            self.render_panel();
            return;
        }
        if let Some(index) = self.strips.iter().position(|(piece, _)| &piece.layer == layer) {
            self.strips[index].0.configured = true;
            self.render_strip(index);
        }
    }

    pub fn set_scale(&mut self, surface: &WlSurface, scale: f64) {
        if self.panel.surface() == surface {
            self.panel.scale = scale;
            self.render_panel();
            return;
        }
        if let Some(index) = self.strips.iter().position(|(piece, _)| piece.surface() == surface) {
            self.strips[index].0.scale = scale;
            self.render_strip(index);
        }
    }

    pub fn tick(&mut self) {
        if !self.processing {
            self.render_panel();
        }
    }

    pub fn show_processing(&mut self) {
        self.processing = true;
        self.hover = None;
        self.pressed = None;
        self.render_panel();
    }

    fn move_to(&mut self, position: (f64, f64), connection: &Connection, qh: &QueueHandle<App>) {
        self.target = position;
        if self.in_flight {
            self.queued = true;
            return;
        }
        self.panel.layer.set_margin(position.1 as i32, 0, 0, position.0 as i32);
        self.panel.layer.wl_surface().commit();
        self.sent = position;
        self.in_flight = true;
        self.queued = false;
        connection.display().sync(qh, PanelMoved);
    }

    pub fn settled(&mut self, connection: &Connection, qh: &QueueHandle<App>) {
        self.in_flight = false;
        self.basis = self.sent;
        if self.queued {
            self.move_to(self.target, connection, qh);
        }
    }

    pub fn pointer(&mut self, event: &PointerEvent, connection: &Connection, qh: &QueueHandle<App>) -> Option<ChromeAction> {
        if self.guards.iter().any(|guard| guard.layer.wl_surface() == &event.surface) {
            return Some(ChromeAction::Manual);
        }
        if self.panel.surface() != &event.surface {
            return None;
        }
        let (x, y) = event.position;
        if let (PointerEventKind::Motion { .. }, Some(grab)) = (&event.kind, self.grab) {
            let limit = |value: f64, size: f64, room: f64| value.clamp(0.0, (room - size).max(0.0)).round();
            let position = (limit(self.basis.0 + x - grab.0, self.width, self.screen.width), limit(self.basis.1 + y - grab.1, PANEL_HEIGHT, self.screen.height));
            self.move_to(position, connection, qh);
            return None;
        }
        if matches!(event.kind, PointerEventKind::Release { button: BTN_LEFT, .. }) && self.grab.take().is_some() {
            return None;
        }
        if self.processing {
            return None;
        }
        let over = self.button_at(x, y);
        match event.kind {
            PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => self.set_hover(over),
            PointerEventKind::Leave { .. } => {
                self.pressed = None;
                self.set_hover(None);
            }
            PointerEventKind::Press { button: BTN_LEFT, .. } => {
                if over.is_none() {
                    self.grab = Some((x, y));
                    return None;
                }
                self.pressed = over;
                self.render_panel();
            }
            PointerEventKind::Release { button: BTN_LEFT, .. } => {
                let pressed = self.pressed.take();
                self.render_panel();
                if pressed.is_some() && pressed == over {
                    return pressed.map(|button| match button {
                        Button::Manual => ChromeAction::Manual,
                        Button::Stop => ChromeAction::Stop,
                        Button::Cancel => ChromeAction::Cancel,
                    });
                }
            }
            _ => {}
        }
        None
    }

    fn set_hover(&mut self, hover: Option<Button>) {
        if self.hover != hover {
            self.hover = hover;
            self.render_panel();
        }
    }

    fn button_at(&self, x: f64, y: f64) -> Option<Button> {
        let top = (PANEL_HEIGHT - BUTTON_HEIGHT) / 2.0;
        if y < top || y > top + BUTTON_HEIGHT {
            return None;
        }
        self.buttons.iter().find(|(_, left, width)| x >= *left && x <= left + width).map(|(button, _, _)| *button)
    }

    fn layout(&self, scale: f32) -> PanelLayout {
        let text = |font: Option<&Font>, value: &str, size: f32| font.map(|font| font.layout(value, size * scale, Align::Left));
        let time_text = if self.processing { "Procesando…".to_string() } else { clock(self.started.elapsed().as_secs()) };
        let logical = |layout: &Option<Layout>| layout.as_ref().map_or(0.0, |layout| f64::from(layout.width) / f64::from(scale));
        let shown: &[Button] = if self.offers_manual { &[Button::Manual, Button::Stop, Button::Cancel] } else { &[Button::Stop, Button::Cancel] };
        let label = text(self.bold.as_ref(), self.title, 11.0);
        let time = text(self.ui.as_ref(), &time_text, 13.0);
        let buttons: Vec<(Button, Option<Layout>)> = shown.iter().map(|button| (*button, text(self.ui.as_ref(), button.label(), 11.0))).collect();
        let mut widths = vec![DOT, logical(&label), logical(&time)];
        widths.extend(buttons.iter().map(|(_, layout)| logical(layout) + 2.0 * BUTTON_PADDING));
        PanelLayout { label, time, buttons, widths }
    }

    fn render_panel(&mut self) {
        if !self.panel.configured {
            return;
        }
        let scale = self.panel.scale.max(1.0) as f32;
        let layout = self.layout(scale);
        let mut lefts = Vec::with_capacity(layout.widths.len());
        let mut x = (self.width - layout.total()) / 2.0;
        for width in &layout.widths {
            lefts.push(x);
            x += width + SPACING;
        }
        self.buttons = layout.buttons.iter().enumerate().map(|(index, (button, _))| (*button, lefts[index + 3], layout.widths[index + 3])).collect();
        let (hover, pressed, processing, panel_width) = (self.hover, self.pressed, self.processing, self.width);
        let Some(pool) = self.pool.as_mut() else { return };
        self.panel.present(pool, |pixmap, s| {
            let px = |value: f64| value as f32 * s;
            let (width, height) = (px(panel_width), px(PANEL_HEIGHT));
            if let Some(background) = draw::rounded_rect(0.0, 0.0, width, height, px(10.0)) {
                pixmap.fill_path(&background, &draw::paint(bgra(32, 32, 36, 235)), FillRule::Winding, Transform::identity(), None);
            }
            let middle = height / 2.0;
            if let Some(dot) = PathBuilder::from_circle(px(lefts[0] + DOT / 2.0), middle, px(DOT / 2.0)) {
                pixmap.fill_path(&dot, &draw::paint(bgra(255, 59, 48, 255)), FillRule::Winding, Transform::identity(), None);
            }
            let text = |pixmap: &mut PixmapMut, layout: &Option<Layout>, left: f64, color: Color| {
                let Some(layout) = layout else { return };
                let Some(path) = &layout.path else { return };
                let top = middle - layout.height / 2.0;
                pixmap.fill_path(path, &draw::paint(color), FillRule::Winding, Transform::from_translate(px(left), top), None);
            };
            text(pixmap, &layout.label, lefts[1], bgra(170, 170, 178, 255));
            text(pixmap, &layout.time, lefts[2], bgra(255, 255, 255, 255));
            let alpha = if processing { 0.4 } else { 1.0 };
            for (index, (button, label)) in layout.buttons.iter().enumerate() {
                let button = *button;
                let left = lefts[index + 3];
                let active = !processing && (hover == Some(button) || pressed == Some(button));
                let primary = button == Button::Stop;
                let fill = match (primary, pressed == Some(button) && !processing, active) {
                    (true, true, _) => bgra(0, 100, 210, 255),
                    (true, false, true) => bgra(40, 150, 255, 255),
                    (true, false, false) => bgra(10, 132, 255, (255.0 * alpha) as u8),
                    (false, true, _) => bgra(255, 255, 255, 70),
                    (false, false, true) => bgra(255, 255, 255, 50),
                    (false, false, false) => bgra(255, 255, 255, (30.0 * alpha) as u8),
                };
                let top = (PANEL_HEIGHT - BUTTON_HEIGHT) / 2.0;
                if let Some(shape) = draw::rounded_rect(px(left), px(top), px(layout.widths[index + 3]), px(BUTTON_HEIGHT), px(6.0)) {
                    pixmap.fill_path(&shape, &draw::paint(fill), FillRule::Winding, Transform::identity(), None);
                }
                text(pixmap, label, left + BUTTON_PADDING, bgra(255, 255, 255, (255.0 * alpha) as u8));
            }
        });
    }

    fn render_strip(&mut self, index: usize) {
        let Some(pool) = self.pool.as_mut() else { return };
        let (piece, horizontal) = &mut self.strips[index];
        let horizontal = *horizontal;
        let length = f64::from(if horizontal { piece.size.0 } else { piece.size.1 });
        piece.present(pool, |pixmap, s| {
            let center = (OUTSET / 2.0) as f32 * s;
            let end = length as f32 * s;
            let mut builder = PathBuilder::new();
            if horizontal {
                builder.move_to(0.0, center);
                builder.line_to(end, center);
            } else {
                builder.move_to(center, 0.0);
                builder.line_to(center, end);
            }
            let Some(path) = builder.finish() else { return };
            let stroke = Stroke { width: 2.0 * s, dash: StrokeDash::new(vec![6.0 * s, 4.0 * s], 0.0), ..Default::default() };
            pixmap.stroke_path(&path, &draw::paint(bgra(255, 59, 48, 255)), &stroke, Transform::identity(), None);
        });
    }

    pub fn destroy(mut self) {
        self.release_guards();
        for (piece, _) in self.strips {
            piece.destroy();
        }
        self.panel.destroy();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect { x: 100.0, y: 0.0, width: 1920.0, height: 1080.0 };

    #[test]
    fn the_panel_goes_below_the_region_when_it_fits() {
        let region = Rect { x: 600.0, y: 200.0, width: 400.0, height: 300.0 };
        assert_eq!(panel_position(region, SCREEN, 250.0), (675.0, 512.0));
    }

    #[test]
    fn the_panel_goes_above_a_region_that_reaches_the_bottom() {
        let region = Rect { x: 600.0, y: 600.0, width: 400.0, height: 470.0 };
        assert_eq!(panel_position(region, SCREEN, 250.0), (675.0, 548.0));
    }

    #[test]
    fn the_panel_goes_inside_a_region_that_fills_the_screen() {
        assert_eq!(panel_position(SCREEN, SCREEN, 250.0), (935.0, 1028.0));
    }

    #[test]
    fn the_panel_stays_on_screen_next_to_an_edge() {
        let region = Rect { x: 100.0, y: 200.0, width: 50.0, height: 50.0 };
        assert_eq!(panel_position(region, SCREEN, 250.0).0, 108.0);
    }

    fn placed(region: Rect) -> Vec<(f64, f64, f64, f64)> {
        strips(region, SCREEN).into_iter().map(|(rect, _)| (rect.x, rect.y, rect.width, rect.height)).collect()
    }

    #[test]
    fn border_strips_surround_the_region_from_outside() {
        let region = Rect { x: 600.0, y: 200.0, width: 400.0, height: 300.0 };
        assert_eq!(placed(region), vec![(597.0, 197.0, 406.0, 3.0), (597.0, 500.0, 406.0, 3.0), (597.0, 200.0, 3.0, 300.0), (1000.0, 200.0, 3.0, 300.0)]);
    }

    #[test]
    fn a_border_strip_that_would_fall_inside_the_region_at_a_screen_edge_is_left_out() {
        let region = Rect { x: 100.0, y: 200.0, width: 400.0, height: 300.0 };
        assert_eq!(placed(region), vec![(100.0, 197.0, 403.0, 3.0), (100.0, 500.0, 403.0, 3.0), (500.0, 200.0, 3.0, 300.0)]);
    }
}
