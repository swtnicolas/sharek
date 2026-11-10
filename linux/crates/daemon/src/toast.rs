use crate::app::App;
use crate::draw::{self, bgra};
use crate::wl::Globals;
use sharek_common::text::{Align, Font};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerSurface};
use smithay_client_toolkit::shm::slot::{Buffer, SlotPool};
use tiny_skia::{FillRule, PixmapMut, Transform};
use wayland_client::protocol::wl_shm;
use wayland_client::QueueHandle;
use wayland_protocols::wp::fractional_scale::v1::client::wp_fractional_scale_v1::WpFractionalScaleV1;
use wayland_protocols::wp::viewporter::client::wp_viewport::WpViewport;

pub const VISIBLE_MS: u64 = 2200;
const WIDTH_LIMIT: f32 = 320.0;

pub struct Toast {
    pub generation: u64,
    title: String,
    detail: Option<String>,
    layer: LayerSurface,
    viewport: WpViewport,
    fractional: Option<WpFractionalScaleV1>,
    pool: Option<SlotPool>,
    buffer: Option<Buffer>,
    scale: f64,
    logical: (u32, u32),
    configured: bool,
}

struct Layouts {
    title: sharek_common::text::Layout,
    detail: Option<sharek_common::text::Layout>,
    size: (u32, u32),
    padding: (f32, f32, f32),
}

fn layouts(font: &Font, title: &str, detail: Option<&str>, scale: f32) -> Layouts {
    let title = font.layout(title, 13.0 * scale, Align::Left);
    let detail = detail.map(|text| font.layout(&wrap(font, text, 11.0 * scale, WIDTH_LIMIT * scale), 11.0 * scale, Align::Left));
    let padding = (16.0 * scale, 12.0 * scale, 4.0 * scale);
    let content_width = detail.as_ref().map_or(title.width, |detail| title.width.max(detail.width));
    let content_height = title.height + detail.as_ref().map_or(0.0, |detail| padding.2 + detail.height);
    let size = ((content_width + 2.0 * padding.0).ceil() as u32, (content_height + 2.0 * padding.1).ceil() as u32);
    Layouts { title, detail, size, padding }
}

fn wrap(font: &Font, text: &str, size: f32, width: f32) -> String {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        for word in paragraph.split(' ') {
            let candidate = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if !line.is_empty() && font.measure(&candidate, size) > width {
                lines.push(std::mem::replace(&mut line, word.to_string()));
            } else {
                line = candidate;
            }
        }
        lines.push(line);
    }
    lines.join("\n")
}

fn preview(detail: &str) -> String {
    let lines: Vec<&str> = detail.split('\n').take(3).collect();
    let joined = lines.join("\n");
    if joined.chars().count() > 180 { format!("{}…", joined.chars().take(180).collect::<String>()) } else { joined }
}

impl Toast {
    pub fn new(generation: u64, title: &str, detail: Option<&str>, globals: &Globals, qh: &QueueHandle<App>) -> Toast {
        let surface = globals.compositor.create_surface(qh);
        globals.no_input(&surface);
        let viewport = globals.viewport(&surface, qh);
        let fractional = globals.fractional_scale(&surface, qh);
        let layer = globals.layer_shell.create_layer_surface(qh, surface, Layer::Overlay, Some("sharek-toast"), None);
        layer.set_anchor(Anchor::BOTTOM);
        layer.set_margin(0, 0, 80, 0);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        let detail = detail.map(preview).filter(|text| !text.is_empty());
        let logical = Font::ui().map_or((320, 48), |font| layouts(&font, title, detail.as_deref(), 1.0).size);
        layer.set_size(logical.0, logical.1);
        layer.commit();
        Toast {
            generation,
            title: title.to_string(),
            detail,
            layer,
            viewport,
            fractional,
            pool: SlotPool::new(64 << 10, &globals.shm).ok(),
            buffer: None,
            scale: 1.0,
            logical,
            configured: false,
        }
    }

    pub fn owns(&self, layer: &LayerSurface) -> bool {
        &self.layer == layer
    }

    pub fn owns_surface(&self, surface: &wayland_client::protocol::wl_surface::WlSurface) -> bool {
        self.layer.wl_surface() == surface
    }

    pub fn set_scale(&mut self, scale: f64) {
        self.scale = scale;
        if self.configured {
            self.render();
        }
    }

    pub fn configure(&mut self) {
        self.configured = true;
        self.render();
    }

    fn render(&mut self) {
        let Some(font) = Font::ui() else { return };
        let Some(pool) = self.pool.as_mut() else { return };
        let scale = self.scale.max(1.0) as f32;
        let Layouts { title, detail, size: (width, height), padding: (padding_x, padding_y, spacing) } =
            layouts(&font, &self.title, self.detail.as_deref(), scale);
        let Ok((buffer, canvas)) = pool.create_buffer(width as i32, height as i32, width as i32 * 4, wl_shm::Format::Argb8888) else { return };
        canvas.fill(0);
        if let Some(mut pixmap) = PixmapMut::from_bytes(canvas, width, height) {
            if let Some(background) = draw::rounded_rect(0.0, 0.0, width as f32, height as f32, 12.0 * scale) {
                pixmap.fill_path(&background, &draw::paint(bgra(32, 32, 36, 235)), FillRule::Winding, Transform::identity(), None);
            }
            if let Some(path) = &title.path {
                pixmap.fill_path(path, &draw::paint(bgra(255, 255, 255, 255)), FillRule::Winding, Transform::from_translate(padding_x, padding_y), None);
            }
            if let Some(path) = detail.as_ref().and_then(|detail| detail.path.as_ref()) {
                let y = padding_y + title.height + spacing;
                pixmap.fill_path(path, &draw::paint(bgra(200, 200, 205, 255)), FillRule::Winding, Transform::from_translate(padding_x, y), None);
            }
        }
        self.viewport.set_destination(self.logical.0 as i32, self.logical.1 as i32);
        let surface = self.layer.wl_surface();
        let _ = buffer.attach_to(surface);
        surface.damage_buffer(0, 0, width as i32, height as i32);
        surface.commit();
        self.buffer = Some(buffer);
    }

    pub fn destroy(self) {
        if let Some(fractional) = self.fractional {
            fractional.destroy();
        }
        self.viewport.destroy();
    }
}
