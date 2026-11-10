use std::env;
use std::path::PathBuf;

pub const APP_ID: &str = "local.sharek";

pub fn home() -> PathBuf {
    env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

fn xdg(variable: &str, fallback: &str) -> PathBuf {
    env::var_os(variable)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home().join(fallback))
}

pub fn config_dir() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config").join("sharek")
}

pub fn data_dir() -> PathBuf {
    xdg("XDG_DATA_HOME", ".local/share").join("sharek")
}

pub fn cache_dir() -> PathBuf {
    xdg("XDG_CACHE_HOME", ".cache").join("sharek")
}

pub fn recordings_dir() -> PathBuf {
    cache_dir().join("recordings")
}

pub fn autostart_file() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config").join("autostart").join(format!("{APP_ID}.desktop"))
}

pub fn pictures_dir() -> PathBuf {
    let dirs = xdg("XDG_CONFIG_HOME", ".config").join("user-dirs.dirs");
    std::fs::read_to_string(dirs)
        .ok()
        .and_then(|text| {
            text.lines().find_map(|line| {
                let value = line.trim().strip_prefix("XDG_PICTURES_DIR=")?.trim_matches('"');
                Some(match value.strip_prefix("$HOME") {
                    Some(rest) => home().join(rest.trim_start_matches('/')),
                    None => PathBuf::from(value),
                })
            })
        })
        .unwrap_or_else(|| home().join("Pictures"))
}
