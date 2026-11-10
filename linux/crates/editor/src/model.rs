use serde::{Deserialize, Serialize};
use sharek_common::image::RgbImage;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Rgba {
    pub const CLEAR: Rgba = Rgba { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };
    pub const WHITE: Rgba = Rgba { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
    pub const BLACK: Rgba = Rgba { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };
    pub const RED: Rgba = Rgba { r: 242.0 / 255.0, g: 60.0 / 255.0, b: 60.0 / 255.0, a: 1.0 };
    pub const YELLOW: Rgba = Rgba { r: 1.0, g: 1.0, b: 0.0, a: 1.0 };
    pub const GRAY: Rgba = Rgba { r: 142.0 / 255.0, g: 142.0 / 255.0, b: 147.0 / 255.0, a: 1.0 };

    pub fn from_rgb8([r, g, b]: [u8; 3]) -> Rgba {
        Rgba { r: f32::from(r) / 255.0, g: f32::from(g) / 255.0, b: f32::from(b) / 255.0, a: 1.0 }
    }

    pub fn is_visible(&self) -> bool {
        self.a > 0.001
    }

    pub fn color(&self) -> tiny_skia::Color {
        tiny_skia::Color::from_rgba(self.r.clamp(0.0, 1.0), self.g.clamp(0.0, 1.0), self.b.clamp(0.0, 1.0), self.a.clamp(0.0, 1.0))
            .unwrap_or(tiny_skia::Color::BLACK)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShapeStyle {
    pub stroke: Rgba,
    pub fill: Rgba,
    pub text: Rgba,
    pub line_width: f32,
    pub corner_radius: f32,
    pub font_size: f32,
    pub strength: f32,
    pub shadow: bool,
}

pub mod option {
    pub const STROKE: u8 = 1 << 0;
    pub const FILL: u8 = 1 << 1;
    pub const TEXT: u8 = 1 << 2;
    pub const LINE_WIDTH: u8 = 1 << 3;
    pub const CORNER_RADIUS: u8 = 1 << 4;
    pub const FONT_SIZE: u8 = 1 << 5;
    pub const STRENGTH: u8 = 1 << 6;
    pub const SHADOW: u8 = 1 << 7;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ShapeKind {
    Rectangle,
    Ellipse,
    Freehand,
    Line,
    Arrow,
    TextOutline,
    TextBackground,
    SpeechBalloon,
    Step,
    Magnify,
    SmartEraser,
    Blur,
    Pixelate,
    Highlight,
    Spotlight,
    Image,
}

impl ShapeKind {
    pub fn is_text(self) -> bool {
        matches!(self, ShapeKind::TextOutline | ShapeKind::TextBackground | ShapeKind::SpeechBalloon)
    }

    pub fn is_effect(self) -> bool {
        matches!(self, ShapeKind::Blur | ShapeKind::Pixelate | ShapeKind::Highlight)
    }

    pub fn options(self) -> u8 {
        use option::*;
        match self {
            ShapeKind::Rectangle => STROKE | FILL | LINE_WIDTH | CORNER_RADIUS | SHADOW,
            ShapeKind::Ellipse => STROKE | FILL | LINE_WIDTH | SHADOW,
            ShapeKind::Freehand | ShapeKind::Line | ShapeKind::Arrow => STROKE | LINE_WIDTH | SHADOW,
            ShapeKind::TextOutline => TEXT | STROKE | LINE_WIDTH | FONT_SIZE | SHADOW,
            ShapeKind::TextBackground | ShapeKind::SpeechBalloon => TEXT | FILL | STROKE | LINE_WIDTH | CORNER_RADIUS | FONT_SIZE | SHADOW,
            ShapeKind::Step => TEXT | FILL | STROKE | LINE_WIDTH | FONT_SIZE | SHADOW,
            ShapeKind::Magnify => STROKE | LINE_WIDTH | STRENGTH | SHADOW,
            ShapeKind::SmartEraser => 0,
            ShapeKind::Blur | ShapeKind::Pixelate | ShapeKind::Spotlight => STRENGTH,
            ShapeKind::Highlight => FILL,
            ShapeKind::Image => STROKE | LINE_WIDTH | SHADOW,
        }
    }

    pub fn stroke_label(self) -> &'static str {
        match self {
            ShapeKind::Freehand | ShapeKind::Line | ShapeKind::Arrow => "Color",
            ShapeKind::TextOutline => "Contorno",
            _ => "Borde",
        }
    }

    pub fn fill_label(self) -> &'static str {
        if self == ShapeKind::Highlight { "Color" } else { "Relleno" }
    }

    pub fn strength_label(self) -> &'static str {
        match self {
            ShapeKind::Blur => "Radio",
            ShapeKind::Pixelate => "Píxel",
            ShapeKind::Magnify => "Zoom",
            ShapeKind::Spotlight => "Oscurecer",
            _ => "",
        }
    }

    pub fn strength_choices(self) -> &'static [f32] {
        match self {
            ShapeKind::Blur => &[3.0, 5.0, 8.0, 10.0, 15.0, 20.0, 30.0, 40.0],
            ShapeKind::Pixelate => &[3.0, 5.0, 7.0, 10.0, 14.0, 20.0, 30.0],
            ShapeKind::Magnify => &[1.5, 2.0, 2.5, 3.0, 4.0, 5.0],
            ShapeKind::Spotlight => &[0.3, 0.45, 0.6, 0.75, 0.9],
            _ => &[],
        }
    }

    pub fn strength_title(self, value: f32) -> String {
        match self {
            ShapeKind::Magnify => format!("{}×", number(value)),
            ShapeKind::Spotlight => format!("{} %", (value * 100.0).round() as i32),
            _ => format!("{} px", number(value)),
        }
    }

    pub fn default_style(self) -> ShapeStyle {
        let mut style = ShapeStyle {
            stroke: Rgba::RED,
            fill: Rgba::CLEAR,
            text: Rgba::WHITE,
            line_width: 2.0,
            corner_radius: 0.0,
            font_size: 14.0,
            strength: 0.0,
            shadow: true,
        };
        match self {
            ShapeKind::Arrow => style.line_width = 3.0,
            ShapeKind::TextOutline => {
                style.text = Rgba::BLACK;
                style.stroke = Rgba::WHITE;
                style.font_size = 16.0;
                style.shadow = false;
            }
            ShapeKind::TextBackground => {
                style.text = Rgba::BLACK;
                style.fill = Rgba::WHITE;
                style.stroke = Rgba::GRAY;
                style.line_width = 0.0;
                style.corner_radius = 4.0;
            }
            ShapeKind::SpeechBalloon => {
                style.text = Rgba::BLACK;
                style.fill = Rgba::WHITE;
                style.stroke = Rgba::GRAY;
                style.corner_radius = 6.0;
            }
            ShapeKind::Step => {
                style.fill = Rgba::RED;
                style.stroke = Rgba::WHITE;
                style.font_size = 12.0;
            }
            ShapeKind::Magnify => style.strength = 2.0,
            ShapeKind::SmartEraser => style.shadow = false,
            ShapeKind::Blur => {
                style.strength = 15.0;
                style.shadow = false;
            }
            ShapeKind::Pixelate => {
                style.strength = 10.0;
                style.shadow = false;
            }
            ShapeKind::Highlight => {
                style.fill = Rgba::YELLOW;
                style.shadow = false;
            }
            ShapeKind::Spotlight => {
                style.strength = 0.6;
                style.shadow = false;
            }
            ShapeKind::Image => style.line_width = 0.0,
            _ => {}
        }
        style
    }
}

pub fn number(value: f32) -> String {
    if value.fract() == 0.0 { format!("{value:.0}") } else { format!("{value}") }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub fn new(x: f32, y: f32) -> Point {
        Point { x, y }
    }

    pub fn distance(self, other: Point) -> f32 {
        (self.x - other.x).hypot(self.y - other.y)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn from_points(a: Point, b: Point) -> Rect {
        Rect { x: a.x.min(b.x), y: a.y.min(b.y), width: (b.x - a.x).abs(), height: (b.y - a.y).abs() }
    }

    pub fn max_x(&self) -> f32 {
        self.x + self.width
    }

    pub fn max_y(&self) -> f32 {
        self.y + self.height
    }

    pub fn mid(&self) -> Point {
        Point::new(self.x + self.width / 2.0, self.y + self.height / 2.0)
    }

    pub fn contains(&self, point: Point) -> bool {
        point.x >= self.x && point.y >= self.y && point.x <= self.max_x() && point.y <= self.max_y()
    }

    pub fn inset(&self, dx: f32, dy: f32) -> Rect {
        Rect { x: self.x + dx, y: self.y + dy, width: self.width - 2.0 * dx, height: self.height - 2.0 * dy }
    }

    pub fn union(&self, other: &Rect) -> Rect {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        Rect { x, y, width: self.max_x().max(other.max_x()) - x, height: self.max_y().max(other.max_y()) - y }
    }

    pub fn union_point(&self, point: Point) -> Rect {
        self.union(&Rect { x: point.x, y: point.y, width: 0.0, height: 0.0 })
    }

    pub fn integral(&self) -> Rect {
        let (x, y) = (self.x.floor(), self.y.floor());
        Rect { x, y, width: self.max_x().ceil() - x, height: self.max_y().ceil() - y }
    }

    pub fn intersection(&self, other: &Rect) -> Option<Rect> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self.max_x().min(other.max_x());
        let bottom = self.max_y().min(other.max_y());
        (right > x && bottom > y).then_some(Rect { x, y, width: right - x, height: bottom - y })
    }

    pub fn skia(&self) -> Option<tiny_skia::Rect> {
        tiny_skia::Rect::from_xywh(self.x, self.y, self.width.max(0.0), self.height.max(0.0))
    }
}

pub struct RgbaImage {
    pub width: u32,
    pub height: u32,
    pub pixmap: tiny_skia::Pixmap,
}

impl RgbaImage {
    pub fn transformed(&self, map: impl Fn(usize, usize) -> (usize, usize), swap: bool) -> Option<RgbaImage> {
        let (width, height) = if swap { (self.height, self.width) } else { (self.width, self.height) };
        let mut pixmap = tiny_skia::Pixmap::new(width, height)?;
        let source = self.pixmap.data();
        let target = pixmap.data_mut();
        for y in 0..self.height as usize {
            for x in 0..self.width as usize {
                let (nx, ny) = map(x, y);
                let from = (y * self.width as usize + x) * 4;
                let to = (ny * width as usize + nx) * 4;
                target[to..to + 4].copy_from_slice(&source[from..from + 4]);
            }
        }
        Some(RgbaImage { width, height, pixmap })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub margin: f32,
    pub background: Rgba,
    pub radius: f32,
    pub shadow: bool,
    pub border: f32,
    pub border_color: Rgba,
}

impl Default for Frame {
    fn default() -> Frame {
        Frame { margin: 32.0, background: Rgba::WHITE, radius: 12.0, shadow: true, border: 0.0, border_color: Rgba::BLACK }
    }
}

impl Frame {
    pub const MARGINS: [f32; 5] = [0.0, 16.0, 32.0, 48.0, 64.0];
    pub const RADII: [f32; 5] = [0.0, 6.0, 12.0, 18.0, 24.0];
    pub const BORDERS: [f32; 5] = [0.0, 1.0, 2.0, 4.0, 8.0];
    const SHADOW_ROOM: f32 = 16.0;

    pub fn effective_margin(&self) -> f32 {
        if self.shadow { self.margin.max(Self::SHADOW_ROOM) } else { self.margin }
    }

    fn file() -> std::path::PathBuf {
        sharek_common::paths::config_dir().join("frame.json")
    }

    pub fn load() -> Frame {
        std::fs::read(Self::file()).ok().and_then(|data| serde_json::from_slice(&data).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        let file = Self::file();
        if let Some(parent) = file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(data) = serde_json::to_vec_pretty(self) {
            let _ = sharek_common::write_atomically(&file, &data);
        }
    }
}

static IDS: AtomicU64 = AtomicU64::new(1);

pub fn next_id() -> u64 {
    IDS.fetch_add(1, Ordering::Relaxed)
}

#[derive(Clone)]
pub struct Shape {
    pub id: u64,
    pub kind: ShapeKind,
    pub style: ShapeStyle,
    pub start: Point,
    pub end: Point,
    pub points: Vec<Point>,
    pub text: String,
    pub tail: Point,
    pub image: Option<Arc<RgbaImage>>,
}

impl Shape {
    pub fn new(kind: ShapeKind, style: ShapeStyle, at: Point) -> Shape {
        Shape {
            id: next_id(),
            kind,
            style,
            start: at,
            end: at,
            points: if kind == ShapeKind::Freehand { vec![at] } else { Vec::new() },
            text: String::new(),
            tail: Point::default(),
            image: None,
        }
    }

    pub fn rect(&self) -> Rect {
        Rect::from_points(self.start, self.end)
    }

    pub fn duplicated(&self, offset: f32) -> Shape {
        let mut copy = self.clone();
        copy.id = next_id();
        copy.map(|p| Point::new(p.x + offset, p.y + offset));
        copy
    }

    pub fn map(&mut self, transform: impl Fn(Point) -> Point) {
        self.start = transform(self.start);
        self.end = transform(self.end);
        self.tail = transform(self.tail);
        for point in &mut self.points {
            *point = transform(*point);
        }
    }
}

#[derive(Clone)]
pub struct EditorState {
    pub base: Arc<RgbImage>,
    pub shapes: Vec<Shape>,
}

impl EditorState {
    pub fn new(base: RgbImage, shapes: Vec<Shape>) -> EditorState {
        EditorState { base: Arc::new(base.on_disk()), shapes }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tool {
    Select,
    Rectangle,
    Ellipse,
    Freehand,
    Line,
    Arrow,
    TextOutline,
    TextBackground,
    SpeechBalloon,
    Step,
    Magnify,
    SmartEraser,
    Blur,
    Pixelate,
    Highlight,
    Spotlight,
    Crop,
    CutOut,
}

pub const SECTIONS: [(&str, &[Tool]); 4] = [
    ("Editar", &[Tool::Select, Tool::Crop, Tool::CutOut, Tool::SmartEraser]),
    ("Dibujar", &[Tool::Rectangle, Tool::Ellipse, Tool::Line, Tool::Arrow, Tool::Freehand, Tool::Highlight]),
    ("Texto", &[Tool::TextOutline, Tool::TextBackground, Tool::SpeechBalloon, Tool::Step]),
    ("Efectos", &[Tool::Blur, Tool::Pixelate, Tool::Spotlight, Tool::Magnify]),
];

pub const ALL_TOOLS: [Tool; 18] = [
    Tool::Select,
    Tool::Rectangle,
    Tool::Ellipse,
    Tool::Freehand,
    Tool::Line,
    Tool::Arrow,
    Tool::TextOutline,
    Tool::TextBackground,
    Tool::SpeechBalloon,
    Tool::Step,
    Tool::Magnify,
    Tool::SmartEraser,
    Tool::Blur,
    Tool::Pixelate,
    Tool::Highlight,
    Tool::Spotlight,
    Tool::Crop,
    Tool::CutOut,
];

impl Tool {
    pub fn kind(self) -> Option<ShapeKind> {
        Some(match self {
            Tool::Rectangle => ShapeKind::Rectangle,
            Tool::Ellipse => ShapeKind::Ellipse,
            Tool::Freehand => ShapeKind::Freehand,
            Tool::Line => ShapeKind::Line,
            Tool::Arrow => ShapeKind::Arrow,
            Tool::TextOutline => ShapeKind::TextOutline,
            Tool::TextBackground => ShapeKind::TextBackground,
            Tool::SpeechBalloon => ShapeKind::SpeechBalloon,
            Tool::Step => ShapeKind::Step,
            Tool::Magnify => ShapeKind::Magnify,
            Tool::SmartEraser => ShapeKind::SmartEraser,
            Tool::Blur => ShapeKind::Blur,
            Tool::Pixelate => ShapeKind::Pixelate,
            Tool::Highlight => ShapeKind::Highlight,
            Tool::Spotlight => ShapeKind::Spotlight,
            Tool::Select | Tool::Crop | Tool::CutOut => return None,
        })
    }

    pub fn for_kind(kind: ShapeKind) -> Option<Tool> {
        ALL_TOOLS.into_iter().find(|tool| tool.kind() == Some(kind))
    }

    pub fn name(self) -> &'static str {
        match self {
            Tool::Select => "select",
            Tool::Rectangle => "rectangle",
            Tool::Ellipse => "ellipse",
            Tool::Freehand => "freehand",
            Tool::Line => "line",
            Tool::Arrow => "arrow",
            Tool::TextOutline => "textOutline",
            Tool::TextBackground => "textBackground",
            Tool::SpeechBalloon => "speechBalloon",
            Tool::Step => "step",
            Tool::Magnify => "magnify",
            Tool::SmartEraser => "smartEraser",
            Tool::Blur => "blur",
            Tool::Pixelate => "pixelate",
            Tool::Highlight => "highlight",
            Tool::Spotlight => "spotlight",
            Tool::Crop => "crop",
            Tool::CutOut => "cutOut",
        }
    }

    pub fn named(name: &str) -> Option<Tool> {
        ALL_TOOLS.into_iter().find(|tool| tool.name() == name)
    }

    pub fn title(self) -> &'static str {
        match self {
            Tool::Select => "Seleccionar y mover",
            Tool::Rectangle => "Rectángulo",
            Tool::Ellipse => "Elipse",
            Tool::Freehand => "Mano alzada",
            Tool::Line => "Línea",
            Tool::Arrow => "Flecha",
            Tool::TextOutline => "Texto con contorno",
            Tool::TextBackground => "Texto con fondo",
            Tool::SpeechBalloon => "Globo de diálogo",
            Tool::Step => "Numeración por pasos",
            Tool::Magnify => "Lupa",
            Tool::SmartEraser => "Borrador inteligente",
            Tool::Blur => "Desenfoque",
            Tool::Pixelate => "Pixelado",
            Tool::Highlight => "Resaltador",
            Tool::Spotlight => "Foco",
            Tool::Crop => "Recortar",
            Tool::CutOut => "Quitar franja",
        }
    }

    pub fn help(self) -> &'static str {
        match self {
            Tool::Select => "Haz clic en una forma para seleccionarla y arrástrala para moverla.",
            Tool::Rectangle => "Arrastra para dibujar un rectángulo. Con Mayús, un cuadrado.",
            Tool::Ellipse => "Arrastra para dibujar una elipse. Con Mayús, un círculo.",
            Tool::Freehand => "Dibuja un trazo libre siguiendo el puntero.",
            Tool::Line => "Arrastra para trazar una línea. Con Mayús, en ángulos fijos.",
            Tool::Arrow => "Arrastra para señalar algo con una flecha. Con Mayús, en ángulos fijos.",
            Tool::TextOutline => "Texto con contorno, legible sobre cualquier fondo.",
            Tool::TextBackground => "Texto sobre un recuadro de color.",
            Tool::SpeechBalloon => "Texto dentro de un globo de diálogo.",
            Tool::Step => "Círculos numerados 1, 2, 3… para marcar pasos.",
            Tool::Magnify => "Muestra ampliada la zona que encierras.",
            Tool::SmartEraser => "Tapa un área con el color del punto donde empiezas a arrastrar.",
            Tool::Blur => "Desenfoca un área para ocultar información.",
            Tool::Pixelate => "Pixela un área para ocultar información.",
            Tool::Highlight => "Resalta un área como un marcador fluorescente.",
            Tool::Spotlight => "Oscurece todo menos el óvalo que dibujas.",
            Tool::Crop => "Recorta la imagen al área que elijas.",
            Tool::CutOut => "Quita una franja horizontal o vertical y junta los bordes.",
        }
    }

    pub fn shortcut(self) -> char {
        match self {
            Tool::Select => 'm',
            Tool::Rectangle => 'r',
            Tool::Ellipse => 'e',
            Tool::Freehand => 'f',
            Tool::Line => 'l',
            Tool::Arrow => 'a',
            Tool::TextOutline => 'o',
            Tool::TextBackground => 't',
            Tool::SpeechBalloon => 's',
            Tool::Step => 'i',
            Tool::Magnify => 'g',
            Tool::SmartEraser => 'y',
            Tool::Blur => 'b',
            Tool::Pixelate => 'p',
            Tool::Highlight => 'h',
            Tool::Spotlight => 'k',
            Tool::Crop => 'c',
            Tool::CutOut => 'x',
        }
    }

    pub fn matching(character: char) -> Option<Tool> {
        ALL_TOOLS.into_iter().find(|tool| tool.shortcut() == character)
    }
}

pub struct StyleStore {
    styles: HashMap<ShapeKind, ShapeStyle>,
}

impl StyleStore {
    fn file() -> std::path::PathBuf {
        sharek_common::paths::config_dir().join("styles.json")
    }

    pub fn load() -> StyleStore {
        let styles = std::fs::read(Self::file()).ok().and_then(|data| serde_json::from_slice(&data).ok()).unwrap_or_default();
        StyleStore { styles }
    }

    #[cfg(test)]
    pub fn empty() -> StyleStore {
        StyleStore { styles: HashMap::new() }
    }

    pub fn style(&self, kind: ShapeKind) -> ShapeStyle {
        self.styles.get(&kind).copied().unwrap_or_else(|| kind.default_style())
    }

    pub fn save(&mut self, kind: ShapeKind, style: ShapeStyle) {
        self.styles.insert(kind, style);
        let file = Self::file();
        if let Some(parent) = file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(data) = serde_json::to_vec_pretty(&self.styles) {
            let _ = sharek_common::write_atomically(&file, &data);
        }
    }
}
