use crate::app::with;
use crate::EditorWindow;
use sharek_common::image;
use sharek_common::ipc::ToDaemon;
use sharek_common::portal;
use sharek_common::settings::Settings;
use sharek_common::{history, time};
use slint::{Image, Rgba8Pixel, SharedPixelBuffer, Weak};
use std::fs::File;
use std::path::PathBuf;
use std::time::Duration;

pub struct MediaTab {
    pub id: u64,
    pub path: PathBuf,
    pub gif: bool,
    pub size: (f64, f64),
    pub created: i64,
    pub saved_automatically: bool,
    pub is_saved: bool,
    pub title: String,
    timer: slint::Timer,
}

pub struct ActionResult {
    pub message: Option<ToDaemon>,
    pub close: bool,
}

struct Player {
    decoder: gif::Decoder<File>,
    canvas: Vec<u8>,
    width: usize,
    height: usize,
}

impl Player {
    fn open(path: &PathBuf) -> Option<Player> {
        let mut options = gif::DecodeOptions::new();
        options.set_color_output(gif::ColorOutput::RGBA);
        let decoder = options.read_info(File::open(path).ok()?).ok()?;
        let (width, height) = (decoder.width() as usize, decoder.height() as usize);
        Some(Player { decoder, canvas: vec![0; width * height * 4], width, height })
    }

    fn next(&mut self) -> Option<(SharedPixelBuffer<Rgba8Pixel>, u64)> {
        let frame = self.decoder.read_next_frame().ok()??;
        let (left, top, frame_width) = (frame.left as usize, frame.top as usize, frame.width as usize);
        let previous = (frame.dispose == gif::DisposalMethod::Previous).then(|| self.canvas.clone());
        for (row, line) in frame.buffer.chunks_exact(frame_width * 4).enumerate() {
            let y = top + row;
            if y >= self.height {
                break;
            }
            for (column, pixel) in line.as_chunks::<4>().0.iter().enumerate() {
                let x = left + column;
                if x >= self.width || pixel[3] == 0 {
                    continue;
                }
                let offset = (y * self.width + x) * 4;
                self.canvas[offset..offset + 4].copy_from_slice(pixel);
            }
        }
        let buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&self.canvas, self.width as u32, self.height as u32);
        let delay = u64::from(frame.delay).max(2) * 10;
        match (frame.dispose, previous) {
            (gif::DisposalMethod::Background, _) => {
                for row in top..(top + frame.height as usize).min(self.height) {
                    let start = (row * self.width + left) * 4;
                    let end = (row * self.width + (left + frame_width).min(self.width)) * 4;
                    self.canvas[start..end].fill(0);
                }
            }
            (gif::DisposalMethod::Previous, Some(previous)) => self.canvas = previous,
            _ => {}
        }
        Some((buffer, delay))
    }
}

pub fn pixel_size(path: &PathBuf, gif: bool) -> (f64, f64) {
    if gif {
        let mut options = gif::DecodeOptions::new();
        options.set_color_output(gif::ColorOutput::RGBA);
        return File::open(path).ok().and_then(|file| options.read_info(file).ok()).map_or((0.0, 0.0), |decoder| (f64::from(decoder.width()), f64::from(decoder.height())));
    }
    let output = std::process::Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height", "-of", "csv=p=0"])
        .arg(path)
        .output();
    let text = output.map(|output| String::from_utf8_lossy(&output.stdout).into_owned()).unwrap_or_default();
    let mut values = text.trim().split(',').filter_map(|value| value.parse::<f64>().ok());
    (values.next().unwrap_or(0.0), values.next().unwrap_or(0.0))
}

thread_local! {
    static PLAYER: std::cell::RefCell<Option<Player>> = const { std::cell::RefCell::new(None) };
}

pub fn advance_frame(id: u64, path: PathBuf, window: Weak<EditorWindow>) {
    let frame = PLAYER.with(|cell| {
        let mut player = cell.borrow_mut();
        let frame = player.as_mut().and_then(Player::next);
        if frame.is_some() {
            return frame;
        }
        *player = Player::open(&path);
        player.as_mut().and_then(Player::next)
    });
    let Some((buffer, delay)) = frame else { return };
    if let Some(window) = window.upgrade() {
        window.set_media_image(Image::from_rgba8(buffer));
    }
    with(move |c| c.schedule_frame(id, path, window, delay));
}

impl MediaTab {
    pub fn new(id: u64, path: PathBuf, gif: bool, size: (f64, f64), created: i64, saved_automatically: bool) -> MediaTab {
        let title = format!("{} · {}", time::clock(created), if gif { "GIF" } else { "Video" });
        MediaTab { id, path, gif, size, created, saved_automatically, is_saved: saved_automatically, title, timer: slint::Timer::default() }
    }

    pub fn subtitle(&self) -> String {
        if !self.is_saved {
            return "Sin guardar".into();
        }
        let name = self.path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        format!("{}: {name}", if self.saved_automatically { "Guardado automático" } else { "Guardado" })
    }

    pub fn show(&mut self, window: Weak<EditorWindow>) {
        if let Some(handle) = window.upgrade() {
            let size = std::fs::metadata(&self.path).map(|meta| meta.len()).unwrap_or(0);
            let size = file_size(size, sharek_common::region_label::decimal_separator());
            handle.set_media_info(format!("{} · {} × {} · {size}", if self.gif { "GIF" } else { "Video MP4" }, self.size.0.round(), self.size.1.round()).into());
            handle.set_media_video(!self.gif);
            handle.set_media_image(Image::default());
        }
        if self.gif {
            PLAYER.with(|cell| *cell.borrow_mut() = Player::open(&self.path));
            advance_frame(self.id, self.path.clone(), window);
        } else {
            let path = self.path.clone();
            std::thread::spawn(move || {
                let output = std::process::Command::new("ffmpeg")
                    .args(["-v", "error", "-i"])
                    .arg(&path)
                    .args(["-frames:v", "1", "-f", "image2pipe", "-vcodec", "png", "-"])
                    .output();
                let Some(decoded) = output.ok().and_then(|output| image::decode(&output.stdout)) else { return };
                let buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&decoded.rgba, decoded.width, decoded.height);
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(window) = window.upgrade() {
                        window.set_media_image(Image::from_rgba8(buffer));
                    }
                });
            });
        }
    }

    pub fn start_timer(&self, delay: u64, callback: impl FnOnce() + 'static) {
        let mut callback = Some(callback);
        self.timer.start(slint::TimerMode::SingleShot, Duration::from_millis(delay), move || {
            if let Some(callback) = callback.take() {
                callback();
            }
        });
    }

    pub fn hide(&mut self) {
        self.timer.stop();
        PLAYER.with(|cell| *cell.borrow_mut() = None);
    }

    fn save_to(&mut self, destination: PathBuf) -> Result<(), String> {
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        if destination.exists() {
            std::fs::remove_file(&destination).map_err(|error| error.to_string())?;
        }
        if self.is_saved {
            std::fs::copy(&self.path, &destination).map_err(|error| error.to_string())?;
        } else if std::fs::rename(&self.path, &destination).is_err() {
            std::fs::copy(&self.path, &destination).map_err(|error| error.to_string())?;
            let _ = std::fs::remove_file(&self.path);
        }
        self.path = destination;
        self.is_saved = true;
        self.saved_automatically = false;
        history::record(&self.path);
        Ok(())
    }

    pub fn action(&mut self, name: &str) -> ActionResult {
        let mut result = ActionResult { message: None, close: false };
        let extension = if self.gif { "gif" } else { "mp4" };
        match name {
            "copy" => result.message = Some(ToDaemon::CopyFile { id: self.id, path: self.path.clone(), gif: self.gif }),
            "save" => {
                self.saved_automatically = false;
                if !self.is_saved {
                    let destination = image::unique_path(&Settings::load().save_directory(), self.created, extension);
                    if let Err(error) = self.save_to(destination) {
                        result.message = Some(ToDaemon::Toast { title: "No se pudo guardar".into(), detail: Some(error) });
                    }
                }
            }
            "save-as" => {
                let (id, directory) = (self.id, Settings::load().save_directory());
                let name = image::unique_path(&directory, self.created, extension).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let filter = if self.gif { ("GIF animado", "*.gif") } else { ("Video MP4", "*.mp4") };
                std::thread::spawn(move || {
                    let Some(connection) = zbus::blocking::Connection::session().ok() else { return };
                    if let Some(path) = portal::save_file(&connection, "Guardar como", &directory, &name, &[filter]) {
                        let _ = slint::invoke_from_event_loop(move || with(move |c| c.media_saved_as(id, path)));
                    }
                });
            }
            "reveal" => {
                let path = self.path.clone();
                std::thread::spawn(move || {
                    if let Ok(connection) = zbus::blocking::Connection::session() {
                        portal::show_items(&connection, &[path]);
                    }
                });
            }
            "play" => {
                let uri = portal::file_uri(&self.path);
                std::thread::spawn(move || {
                    if let Ok(connection) = zbus::blocking::Connection::session() {
                        portal::open_uri(&connection, &uri);
                    }
                });
            }
            "discard" => {
                result.message = Some(ToDaemon::Discard { id: self.id });
                if !self.is_saved {
                    let _ = std::fs::remove_file(&self.path);
                } else if self.saved_automatically {
                    let path = self.path.clone();
                    std::thread::spawn(move || {
                        if let Ok(connection) = zbus::blocking::Connection::session() {
                            portal::trash(&connection, &path);
                        }
                        history::remove(&[path]);
                    });
                }
                result.close = true;
            }
            _ => {}
        }
        result
    }

    pub fn saved_as(&mut self, path: PathBuf) -> Option<String> {
        self.save_to(path).err()
    }
}

fn file_size(bytes: u64, separator: char) -> String {
    if bytes < 1_048_576 {
        return format!("{} KB", bytes.div_ceil(1024));
    }
    format!("{:.1} MB", bytes as f64 / 1_048_576.0).replace('.', &separator.to_string())
}

#[cfg(test)]
mod tests {
    use super::file_size;

    #[test]
    fn small_files_read_in_kilobytes_and_large_ones_in_megabytes() {
        assert_eq!(file_size(27_863, ','), "28 KB");
        assert_eq!(file_size(1_572_864, ','), "1,5 MB");
    }
}
