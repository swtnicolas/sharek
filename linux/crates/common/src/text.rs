use std::path::PathBuf;
use tiny_skia::{Path, PathBuilder, Transform};

pub struct Font {
    data: Vec<u8>,
}

struct Outline<'a> {
    builder: &'a mut PathBuilder,
    x: f32,
    y: f32,
    scale: f32,
}

impl ttf_parser::OutlineBuilder for Outline<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.builder.move_to(self.x + x * self.scale, self.y - y * self.scale);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.builder.line_to(self.x + x * self.scale, self.y - y * self.scale);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.builder.quad_to(self.x + x1 * self.scale, self.y - y1 * self.scale, self.x + x * self.scale, self.y - y * self.scale);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.builder.cubic_to(
            self.x + x1 * self.scale,
            self.y - y1 * self.scale,
            self.x + x2 * self.scale,
            self.y - y2 * self.scale,
            self.x + x * self.scale,
            self.y - y * self.scale,
        );
    }
    fn close(&mut self) {
        self.builder.close();
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
}

pub struct Layout {
    pub path: Option<Path>,
    pub width: f32,
    pub height: f32,
    pub line_height: f32,
}

fn locate(pattern: &str, fallbacks: &[&str]) -> Option<PathBuf> {
    let found = std::process::Command::new("fc-match")
        .args(["-f", "%{file}", pattern])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| PathBuf::from(String::from_utf8_lossy(&output.stdout).into_owned()))
        .filter(|path| path.is_file());
    found.or_else(|| fallbacks.iter().map(PathBuf::from).find(|path| path.is_file()))
}

impl Font {
    pub fn ui() -> Option<Font> {
        let path = locate("Noto Sans:weight=medium", &["/usr/share/fonts/noto/NotoSans-Medium.ttf", "/usr/share/fonts/TTF/DejaVuSans.ttf"])?;
        Font::open(path)
    }

    pub fn bold() -> Option<Font> {
        let path = locate("Noto Sans:weight=semibold", &["/usr/share/fonts/noto/NotoSans-Bold.ttf", "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf"])?;
        Font::open(path)
    }

    pub fn open(path: PathBuf) -> Option<Font> {
        let data = std::fs::read(path).ok()?;
        ttf_parser::Face::parse(&data, 0).ok()?;
        Some(Font { data })
    }

    fn face(&self) -> ttf_parser::Face<'_> {
        ttf_parser::Face::parse(&self.data, 0).expect("font parsed at load")
    }

    pub fn line_height(&self, size: f32) -> f32 {
        let face = self.face();
        let scale = size / f32::from(face.units_per_em());
        f32::from(face.ascender() - face.descender() + face.line_gap()) * scale
    }

    pub fn measure(&self, line: &str, size: f32) -> f32 {
        let face = self.face();
        let scale = size / f32::from(face.units_per_em());
        line.chars().map(|c| face.glyph_index(c).and_then(|g| face.glyph_hor_advance(g)).unwrap_or(0) as f32 * scale).sum()
    }

    pub fn layout(&self, text: &str, size: f32, align: Align) -> Layout {
        let face = self.face();
        let scale = size / f32::from(face.units_per_em());
        let ascender = f32::from(face.ascender()) * scale;
        let line_height = self.line_height(size);
        let lines: Vec<&str> = text.split('\n').collect();
        let widths: Vec<f32> = lines.iter().map(|line| self.measure(line, size)).collect();
        let width = widths.iter().copied().fold(0.0, f32::max);
        let mut builder = PathBuilder::new();
        for (index, line) in lines.iter().enumerate() {
            let mut x = if align == Align::Center { (width - widths[index]) / 2.0 } else { 0.0 };
            let y = ascender + index as f32 * line_height;
            for character in line.chars() {
                let Some(glyph) = face.glyph_index(character) else { continue };
                face.outline_glyph(glyph, &mut Outline { builder: &mut builder, x, y, scale });
                x += face.glyph_hor_advance(glyph).unwrap_or(0) as f32 * scale;
            }
        }
        Layout { path: builder.finish(), width, height: line_height * lines.len() as f32, line_height }
    }
}

pub fn translated(path: &Path, x: f32, y: f32) -> Option<Path> {
    path.clone().transform(Transform::from_translate(x, y))
}
