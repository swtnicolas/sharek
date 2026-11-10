use crate::paths;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub copy_on_capture: bool,
    pub auto_save: bool,
    pub open_editor: bool,
    pub shows_cursor: bool,
    pub records_audio: bool,
    pub login_item_configured: bool,
    pub save_directory: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            copy_on_capture: true,
            auto_save: false,
            open_editor: true,
            shows_cursor: false,
            records_audio: false,
            login_item_configured: false,
            save_directory: None,
        }
    }
}

impl Settings {
    fn file() -> PathBuf {
        paths::config_dir().join("settings.json")
    }

    pub fn load() -> Settings {
        std::fs::read(Self::file())
            .ok()
            .and_then(|data| serde_json::from_slice(&data).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let file = Self::file();
        if let Some(parent) = file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(data) = serde_json::to_vec_pretty(self) {
            let _ = crate::write_atomically(&file, &data);
        }
    }

    pub fn save_directory(&self) -> PathBuf {
        self.save_directory.clone().unwrap_or_else(|| paths::pictures_dir().join("ShareK"))
    }
}
