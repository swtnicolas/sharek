use crate::app::{App, Event};
use crate::chrome::Chrome;
use smithay_client_toolkit::dispatch2::Dispatch2;
use smithay_client_toolkit::reexports::calloop::channel::Sender;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use wayland_client::{Connection, QueueHandle};
use wayland_protocols_plasma::screencast::v1::client::zkde_screencast_stream_unstable_v1::{self, ZkdeScreencastStreamUnstableV1};

pub const POINTER_EMBEDDED: u32 = 2;
const VIDEO_RATE: u32 = 30;
const GIF_RATE: u32 = 15;
const ERROR_LIMIT: usize = 400;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Video,
    Gif,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Format::Video => "mp4",
            Format::Gif => "gif",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Format::Video => "Video",
            Format::Gif => "GIF",
        }
    }

    pub fn scale(self, ratio: f64) -> f64 {
        match self {
            Format::Video => ratio,
            Format::Gif => 1.0,
        }
    }
}

pub struct Recording {
    pub generation: u64,
    pub format: Format,
    pub path: PathBuf,
    pub created: i64,
    pub size: (u32, u32),
    pub pixels: (u32, u32),
    pub saved_automatically: bool,
    pub audio: bool,
    pub stream: ZkdeScreencastStreamUnstableV1,
    pub process: Option<u32>,
    pub chrome: Chrome,
    pub stopping: Option<bool>,
}

pub struct StreamData;

impl Dispatch2<ZkdeScreencastStreamUnstableV1, App> for StreamData {
    fn event(&self, app: &mut App, stream: &ZkdeScreencastStreamUnstableV1, event: zkde_screencast_stream_unstable_v1::Event, _: &Connection, _: &QueueHandle<App>) {
        app.stream_event(stream, event);
    }
}

pub fn even(value: f64) -> u32 {
    ((value.round().max(0.0) as u32) & !1).max(2)
}

pub fn pixel_size(format: Format, size: (u32, u32), ratio: f64) -> (u32, u32) {
    match format {
        Format::Video => (even(f64::from(size.0) * ratio), even(f64::from(size.1) * ratio)),
        Format::Gif => size,
    }
}

fn words(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_string).collect()
}

fn capture(node: u32, keepalive: u32, raw: &str, pixels: (u32, u32), rate: u32) -> Vec<String> {
    let mut args = words(&format!("pipewiresrc path={node} do-timestamp=true keepalive-time={keepalive} ! videoconvert ! videoscale add-borders=false !"));
    args.push(format!("video/x-raw,format={raw},width={},height={}", pixels.0, pixels.1));
    args.extend(words(&format!("! videorate ! video/x-raw,framerate={rate}/1")));
    args
}

pub fn video_pipeline(node: u32, pixels: (u32, u32), audio: bool, path: &Path) -> Vec<String> {
    let mut args = words("-q -e mp4mux name=mux faststart=true ! filesink");
    args.push(format!("location={}", path.display()));
    args.extend(capture(node, 1000, "I420", pixels, VIDEO_RATE));
    args.extend(words(
        "! x264enc speed-preset=ultrafast tune=zerolatency pass=qual quantizer=23 threads=4 ! h264parse ! queue ! mux.",
    ));
    if audio {
        args.extend(words("pulsesrc device=@DEFAULT_MONITOR@ do-timestamp=true ! audioconvert ! audioresample ! audio/x-raw,rate=48000,channels=2 ! fdkaacenc ! queue ! mux."));
    }
    args
}

pub fn gif_pipeline(node: u32, pixels: (u32, u32)) -> Vec<String> {
    let mut args = words("-q -e");
    args.extend(capture(node, 1000 / GIF_RATE, "BGRx", pixels, GIF_RATE));
    args.extend(words("! fdsink fd=1"));
    args
}

fn helper(name: &str) -> Result<PathBuf, String> {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(name)))
        .filter(|path| path.exists())
        .ok_or_else(|| format!("Falta {name}; instala ShareK con scripts/install.sh"))
}

fn tied_to_daemon(command: &mut Command) -> &mut Command {
    unsafe {
        command.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
            Ok(())
        })
    }
}

fn gstreamer(args: Vec<String>, stdout: Stdio) -> Result<Child, String> {
    tied_to_daemon(&mut Command::new("gst-launch-1.0"))
        .args(args)
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("No se pudo iniciar GStreamer (gst-launch-1.0): {error}"))
}

fn wait(child: &mut Child) -> Result<(), String> {
    let mut errors = String::new();
    if let Some(mut stderr) = child.stderr.take() {
        let _ = stderr.read_to_string(&mut errors);
    }
    match child.wait() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => {
            let detail = errors.lines().filter(|line| !line.trim().is_empty()).take(3).collect::<Vec<_>>().join("\n");
            let detail = if detail.is_empty() { status.to_string() } else { detail };
            Err(detail.chars().take(ERROR_LIMIT).collect())
        }
        Err(error) => Err(error.to_string()),
    }
}

pub fn launch(recording: &Recording, node: u32, sender: Sender<Event>) -> Result<u32, String> {
    let generation = recording.generation;
    match recording.format {
        Format::Video => {
            let mut gst = gstreamer(video_pipeline(node, recording.pixels, recording.audio, &recording.path), Stdio::null())?;
            let pid = gst.id();
            std::thread::spawn(move || {
                let result = wait(&mut gst);
                let _ = sender.send(Event::RecordingEnded(generation, result));
            });
            Ok(pid)
        }
        Format::Gif => {
            let encoder = helper("sharek-gif")?;
            let mut gst = gstreamer(gif_pipeline(node, recording.pixels), Stdio::piped())?;
            let pid = gst.id();
            let frames = gst.stdout.take().map(Stdio::from).unwrap_or_else(Stdio::null);
            let spawned = tied_to_daemon(&mut Command::new(encoder))
                .arg(&recording.path)
                .args([recording.pixels.0.to_string(), recording.pixels.1.to_string(), GIF_RATE.to_string()])
                .stdin(frames)
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn();
            let mut gif = match spawned {
                Ok(gif) => gif,
                Err(error) => {
                    let _ = gst.kill();
                    let _ = gst.wait();
                    return Err(error.to_string());
                }
            };
            std::thread::spawn(move || {
                let captured = wait(&mut gst);
                let encoded = wait(&mut gif);
                let result = match (encoded, captured) {
                    (Ok(()), _) => Ok(()),
                    (Err(_), Err(error)) | (Err(error), Ok(())) => Err(error),
                };
                let _ = sender.send(Event::RecordingEnded(generation, result));
            });
            Ok(pid)
        }
    }
}

pub fn signal(pid: u32, signal: i32) {
    unsafe {
        libc::kill(pid as i32, signal);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_pixels_are_rounded_down_to_even_sizes() {
        assert_eq!(pixel_size(Format::Video, (801, 451), 1.4), (1120, 630));
        assert_eq!(pixel_size(Format::Video, (1, 1), 1.0), (2, 2));
    }

    #[test]
    fn gif_pixels_match_the_logical_size() {
        assert_eq!(pixel_size(Format::Gif, (801, 451), 1.4), (801, 451));
    }

    #[test]
    fn a_saved_path_with_spaces_stays_one_argument() {
        let args = video_pipeline(7, (100, 100), false, Path::new("/home/me/Mis videos/a b.mp4"));
        assert!(args.contains(&"location=/home/me/Mis videos/a b.mp4".to_string()));
    }

    #[test]
    fn system_audio_is_muxed_only_when_enabled() {
        let has_audio = |audio| video_pipeline(7, (100, 100), audio, Path::new("/tmp/a.mp4")).iter().any(|arg| arg == "pulsesrc");
        assert!(has_audio(true));
        assert!(!has_audio(false));
    }
}
