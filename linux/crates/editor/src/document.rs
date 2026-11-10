use crate::model::EditorState;
use crate::render::Renderer;
use sharek_common::image::RgbImage;
use sharek_common::time;
use std::path::PathBuf;
use std::sync::Arc;

const UNDO_LEVELS: usize = 100;

pub struct Document {
    pub id: u64,
    pub scale: f32,
    pub created: i64,
    pub title: String,
    pub state: EditorState,
    pub version: u64,
    saved_version: u64,
    pub saved: Option<PathBuf>,
    pub auto_saved: bool,
    undo: Vec<(EditorState, String)>,
    redo: Vec<(EditorState, String)>,
    export: Option<(u64, Arc<RgbImage>)>,
}

impl Document {
    pub fn new(id: u64, base: RgbImage, scale: f32, created: i64, saved: Option<PathBuf>) -> Document {
        let title = format!("{} · {}×{}", time::clock(created), base.width, base.height);
        let auto_saved = saved.is_some();
        Document {
            id,
            scale,
            created,
            title,
            state: EditorState::new(base, Vec::new()),
            version: 0,
            saved_version: 0,
            saved,
            auto_saved,
            undo: Vec::new(),
            redo: Vec::new(),
            export: None,
        }
    }

    pub fn canvas(&self) -> (f32, f32) {
        (self.state.base.width as f32 / self.scale, self.state.base.height as f32 / self.scale)
    }

    pub fn has_unsaved_changes(&self) -> bool {
        self.version != self.saved_version
    }

    pub fn register(&mut self, before: EditorState, name: &str) {
        self.undo.push((before, name.to_string()));
        if self.undo.len() > UNDO_LEVELS {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.version += 1;
    }

    pub fn touch(&mut self) {
        self.version += 1;
    }

    pub fn undo(&mut self) -> bool {
        let Some((state, name)) = self.undo.pop() else { return false };
        let current = std::mem::replace(&mut self.state, state);
        self.redo.push((current, name));
        self.version += 1;
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some((state, name)) = self.redo.pop() else { return false };
        let current = std::mem::replace(&mut self.state, state);
        self.undo.push((current, name));
        self.version += 1;
        true
    }

    pub fn rendered(&mut self, renderer: &mut Renderer) -> Arc<RgbImage> {
        if let Some((version, image)) = &self.export
            && *version == self.version {
                return image.clone();
            }
        let image = if self.state.shapes.is_empty() { self.state.base.clone() } else { Arc::new(renderer.export(&self.state, self.scale).on_disk()) };
        self.export = Some((self.version, image.clone()));
        image
    }

    pub fn mark_saved(&mut self, path: PathBuf, automatically: bool) {
        self.saved = Some(path);
        self.saved_version = self.version;
        self.auto_saved = automatically;
    }

    pub fn subtitle(&self) -> String {
        let Some(path) = &self.saved else { return "Sin guardar".into() };
        let prefix = if self.auto_saved { "Guardado automático" } else { "Guardado" };
        let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        format!("{prefix}: {name}{}", if self.has_unsaved_changes() { " · con cambios" } else { "" })
    }
}
