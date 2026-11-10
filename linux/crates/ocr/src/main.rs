mod pipeline;

use ort::session::Session;
use ort::value::{Tensor, ValueType};
use pipeline::{Image, TextBox};
use sharek_common::{image, paths};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

const DETECTOR: &str = "PP-OCRv5_mobile_det.onnx";
const RECOGNIZER: &str = "latin_PP-OCRv5_mobile_rec.onnx";
const DICTIONARY: &str = "latin_PP-OCRv5_mobile_rec.dict.json";
const DET_MIN_SIDE: usize = 64;
const DET_MAX_SIDE: usize = 1280;
const REC_HEIGHT: usize = 48;
const REC_MIN_WIDTH: usize = 320;
const REC_MAX_WIDTH: usize = 3200;
const REC_MIN_SCORE: f32 = 0.5;
const THREADS: usize = 4;

fn models() -> PathBuf {
    paths::data_dir().join("ocr")
}

fn session(environment: &ort::environment::Environment, file: &str) -> Result<Session, String> {
    let path = models().join(file);
    Session::builder(environment)
        .and_then(|builder| builder.with_intra_threads(THREADS).map_err(Into::into))
        .and_then(|builder| builder.with_memory_pattern(false).map_err(Into::into))
        .and_then(|mut builder| builder.commit_from_file(&path))
        .map_err(|error| format!("No se pudo cargar {file}: {error}"))
}

fn input_height(session: &Session) -> usize {
    match session.inputs().first().map(|input| input.dtype()) {
        Some(ValueType::Tensor { shape, .. }) if shape.len() == 4 && shape[2] > 0 => shape[2] as usize,
        _ => REC_HEIGHT,
    }
}

fn run(png: &[u8]) -> Result<String, String> {
    if [DETECTOR, RECOGNIZER, DICTIONARY].iter().any(|file| !models().join(file).exists()) {
        return Err(format!("Faltan los modelos de OCR en {}. Descárgalos con scripts/fetch-ocr-models.sh", models().display()));
    }
    let decoded = image::decode(png).ok_or("La imagen no es un PNG ni un JPEG válido")?;
    let picture = Image { rgba: &decoded.rgba, width: decoded.width as usize, height: decoded.height as usize };
    let dictionary: Vec<String> = serde_json::from_slice(&std::fs::read(models().join(DICTIONARY)).map_err(|error| format!("No se pudo leer {DICTIONARY}: {error}"))?)
        .map_err(|error| format!("{DICTIONARY} no es válido: {error}"))?;
    let environment = ort::init().with_name("sharek-ocr").build().map_err(|error| format!("No se pudo iniciar ONNX Runtime: {error}"))?;

    let mut detector = session(&environment, DETECTOR)?;
    let size = pipeline::detection_size(picture.width, picture.height, DET_MIN_SIDE, DET_MAX_SIDE);
    let input = Tensor::from_array(([1usize, 3, size.1, size.0], pipeline::detection_input(&picture, size))).map_err(|error| error.to_string())?;
    let mut found = {
        let outputs = detector.run(ort::inputs![input]).map_err(|error| format!("Falló la detección: {error}"))?;
        let (shape, probability) = outputs[0].try_extract_tensor::<f32>().map_err(|error| error.to_string())?;
        let (height, width) = (shape[shape.len() - 2] as usize, shape[shape.len() - 1] as usize);
        pipeline::boxes(probability, width, height, (picture.width as f64 / width as f64, picture.height as f64 / height as f64))
    };
    drop(detector);
    pipeline::sort_boxes(&mut found);

    let mut recognizer = session(&environment, RECOGNIZER)?;
    let height = input_height(&recognizer);
    let mut lines: Vec<(TextBox, String)> = Vec::with_capacity(found.len());
    for text_box in found {
        let (data, width) = pipeline::recognition_input(&picture, &text_box, height, REC_MIN_WIDTH, REC_MAX_WIDTH);
        let input = Tensor::from_array(([1usize, 3, height, width], data)).map_err(|error| error.to_string())?;
        let outputs = recognizer.run(ort::inputs![input]).map_err(|error| format!("Falló el reconocimiento: {error}"))?;
        let (shape, probabilities) = outputs[0].try_extract_tensor::<f32>().map_err(|error| error.to_string())?;
        let (steps, classes) = (shape[1] as usize, shape[2] as usize);
        let (text, score) = pipeline::decode(probabilities, steps, classes, &dictionary);
        if score >= REC_MIN_SCORE {
            lines.push((text_box, text));
        }
    }
    Ok(pipeline::assemble(lines))
}

fn main() -> ExitCode {
    let mut png = Vec::new();
    if let Err(error) = std::io::stdin().read_to_end(&mut png) {
        eprintln!("No se pudo leer la imagen: {error}");
        return ExitCode::FAILURE;
    }
    match run(&png) {
        Ok(text) => {
            let mut stdout = std::io::stdout().lock();
            let _ = stdout.write_all(text.as_bytes());
            let _ = stdout.flush();
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
