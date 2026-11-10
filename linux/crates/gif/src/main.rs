mod encoder;

use encoder::Encoder;
use std::fs::File;
use std::io::{BufWriter, ErrorKind, Read};
use std::path::PathBuf;
use std::process::ExitCode;

fn run(path: &PathBuf, width: usize, height: usize, rate: f64) -> Result<(), String> {
    let file = File::create(path).map_err(|error| error.to_string())?;
    let mut encoder = Encoder::new(BufWriter::new(file), width, height).map_err(|error| error.to_string())?;
    let mut frame = vec![0u8; width * height * 4];
    let mut input = std::io::stdin().lock();
    let mut frames = 0u64;
    loop {
        match input.read_exact(&mut frame) {
            Ok(()) => {
                encoder.append(&frame, width * 4, frames as f64 / rate).map_err(|error| error.to_string())?;
                frames += 1;
            }
            Err(error) if error.kind() == ErrorKind::UnexpectedEof => break,
            Err(error) => return Err(error.to_string()),
        }
    }
    if encoder.is_empty() {
        return Err("La grabación no tiene cuadros.".into());
    }
    encoder.finish(frames as f64 / rate).map_err(|error| error.to_string())?;
    Ok(())
}

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let parsed = match arguments.as_slice() {
        [path, width, height, rate] => match (width.parse::<usize>(), height.parse::<usize>(), rate.parse::<f64>()) {
            (Ok(width), Ok(height), Ok(rate)) if width > 0 && height > 0 && rate > 0.0 => Some((PathBuf::from(path), width, height, rate)),
            _ => None,
        },
        _ => None,
    };
    let Some((path, width, height, rate)) = parsed else {
        eprintln!("uso: sharek-gif <archivo> <ancho> <alto> <cuadros por segundo>");
        return ExitCode::from(2);
    };
    match run(&path, width, height, rate) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ = std::fs::remove_file(&path);
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
