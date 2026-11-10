use crate::image::{self, RgbImage};
use std::io::{Read, Write};
use std::process::{Command, Stdio};

pub fn recognize(image: &RgbImage, scale: f64) -> Result<String, String> {
    let program = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("sharek-ocr")))
        .filter(|path| path.exists())
        .ok_or("El reconocimiento de texto no está instalado")?;
    let png = image::encode_png(image, scale, png::Compression::Fast).map_err(|error| error.to_string())?;
    let mut child = Command::new(program).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|error| error.to_string())?;
    let mut stdin = child.stdin.take().ok_or("sin stdin")?;
    let writer = std::thread::spawn(move || stdin.write_all(&png));
    let mut text = String::new();
    child.stdout.take().ok_or("sin stdout")?.read_to_string(&mut text).map_err(|error| error.to_string())?;
    let _ = writer.join();
    let mut errors = String::new();
    if let Some(mut stderr) = child.stderr.take() {
        let _ = stderr.read_to_string(&mut errors);
    }
    let status = child.wait().map_err(|error| error.to_string())?;
    if status.success() { Ok(text.trim_end().to_string()) } else { Err(errors.trim().to_string()) }
}
