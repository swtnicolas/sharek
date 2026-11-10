use crate::model::Tool;

pub fn tool(tool: Tool) -> &'static str {
    match tool {
        Tool::Select => "M 6 3 L 6 19 L 10 15 L 13 21 L 15.5 20 L 12.5 14 L 18 14 Z",
        Tool::Rectangle => "M 4 6 L 20 6 L 20 18 L 4 18 Z",
        Tool::Ellipse => "M 3 12 A 9 7 0 1 0 21 12 A 9 7 0 1 0 3 12 Z",
        Tool::Freehand => "M 3 17 C 6 8 9 8 10 13 C 11 18 14 18 16 10 C 17 7 19 6 21 7",
        Tool::Line => "M 5 19 L 19 5",
        Tool::Arrow => "M 5 19 L 19 5 M 11 5 L 19 5 L 19 13",
        Tool::TextOutline => "M 6 20 L 12 4 L 18 20 M 8.5 14 L 15.5 14",
        Tool::TextBackground => "M 3 5 L 21 5 L 21 19 L 3 19 Z M 8 9 L 16 9 M 12 9 L 12 16",
        Tool::SpeechBalloon => "M 4 5 L 20 5 L 20 15 L 11 15 L 6 20 L 7 15 L 4 15 Z",
        Tool::Step => "M 3 12 A 9 9 0 1 0 21 12 A 9 9 0 1 0 3 12 Z M 10 9 L 12.5 7.5 L 12.5 16.5",
        Tool::Magnify => "M 4 10 A 6 6 0 1 0 16 10 A 6 6 0 1 0 4 10 Z M 14.5 14.5 L 20 20 M 10 7 L 10 13 M 7 10 L 13 10",
        Tool::SmartEraser => "M 3 16 L 12 7 L 18 13 L 11 20 L 6 20 Z M 7.5 11.5 L 13.5 17.5 M 11 20 L 21 20",
        Tool::Blur => "M 12 3 C 12 3 5 11 5 15 A 7 7 0 0 0 19 15 C 19 11 12 3 12 3 Z",
        Tool::Pixelate => "M 4 4 L 10 4 L 10 10 L 4 10 Z M 14 4 L 20 4 L 20 10 L 14 10 Z M 4 14 L 10 14 L 10 20 L 4 20 Z M 14 14 L 20 14 L 20 20 L 14 20 Z",
        Tool::Highlight => "M 14 4 L 20 10 L 11 19 L 5 19 L 5 13 Z M 9 9 L 15 15",
        Tool::Spotlight => "M 3 12 A 9 6 0 1 0 21 12 A 9 6 0 1 0 3 12 Z M 9 12 A 3 3 0 1 0 15 12 A 3 3 0 1 0 9 12 Z",
        Tool::Crop => "M 7 3 L 7 17 L 21 17 M 3 7 L 17 7 L 17 21",
        Tool::CutOut => "M 3 12 L 21 12 M 12 3 L 12 8 M 9 5.5 L 12 8 L 15 5.5 M 12 21 L 12 16 M 9 18.5 L 12 16 L 15 18.5",
    }
}

const COPY: &str = "M 8 8 L 19 8 L 19 20 L 8 20 Z M 5 16 L 5 4 L 16 4";
const SAVE: &str = "M 5 4 L 16 4 L 20 8 L 20 19 A 1 1 0 0 1 19 20 L 5 20 A 1 1 0 0 1 4 19 L 4 5 A 1 1 0 0 1 5 4 Z M 8 4 L 8 9 L 15 9 L 15 4 M 7 20 L 7 14 L 17 14 L 17 20";
const SAVE_AS: &str = "M 11 20 L 5 20 A 1 1 0 0 1 4 19 L 4 5 A 1 1 0 0 1 5 4 L 15 4 L 19 8 L 19 10 M 8 4 L 8 8.5 L 14 8.5 L 14 4 M 7 20 L 7 14 L 12 14 M 13.5 21 L 14 18 L 19.5 12.5 L 22 15 L 16.5 20.5 Z";
const DISCARD: &str = "M 5 7 L 19 7 M 9 7 L 9 4 L 15 4 L 15 7 M 7 7 L 8 20 L 16 20 L 17 7";

pub const UNDO: &str = "M 9 14 L 4 9 L 9 4 M 4 9 L 14 9 A 5 5 0 0 1 14 19 L 10 19";
pub const REDO: &str = "M 15 14 L 20 9 L 15 4 M 20 9 L 10 9 A 5 5 0 0 0 10 19 L 14 19";
pub const ROTATE: &str = "M 20 12 A 8 8 0 1 1 16.5 5.4 M 17 2 L 17 6 L 13 6";
pub const FLIP: &str = "M 12 3 L 12 21 M 9 6 L 3 18 L 9 18 Z M 15 6 L 21 18 L 15 18 Z";
pub const FRAME: &str = "M 3 6 A 3 3 0 0 1 6 3 L 18 3 A 3 3 0 0 1 21 6 L 21 18 A 3 3 0 0 1 18 21 L 6 21 A 3 3 0 0 1 3 18 Z M 7 7 L 17 7 L 17 17 L 7 17 Z";
pub const EYEDROPPER: &str = "M 13 6 L 18 11 M 16 4 A 2.5 2.5 0 0 1 20 8 L 17.5 10.5 L 13.5 6.5 Z M 15 9 L 7 17 L 5 19.5 L 4.5 19.5 L 4.5 19 L 7 17";
pub const FIT: &str = "M 4 9 L 4 4 L 9 4 M 15 4 L 20 4 L 20 9 M 20 15 L 20 20 L 15 20 M 9 20 L 4 20 L 4 15";

pub const ACTIONS_CAPTURE: [(&str, &str, &str, &str); 7] = [
    ("copy", "Copiar · Ctrl+C", "Arrástralo para soltar la captura en otra aplicación.", COPY),
    ("save", "Guardar · Ctrl+S", "Guarda la imagen en la carpeta de capturas.", SAVE),
    ("save-as", "Guardar como… · Ctrl+Mayús+S", "Elige el nombre, la carpeta y el formato.", SAVE_AS),
    ("ocr", "Extraer texto (OCR)", "Reconoce el texto de toda la imagen y lo muestra para copiarlo.", "M 4 8 L 4 4 L 8 4 M 16 4 L 20 4 L 20 8 M 20 16 L 20 20 L 16 20 M 8 20 L 4 20 L 4 16 M 8 9 L 16 9 M 12 9 L 12 16"),
    ("ocr-area", "Extraer texto de una zona", "Arrastra sobre el texto que quieres reconocer.", "M 4 7 L 4 4 L 7 4 M 10 4 L 14 4 M 17 4 L 20 4 L 20 7 M 20 10 L 20 14 M 20 17 L 20 20 L 17 20 M 14 20 L 10 20 M 7 20 L 4 20 L 4 17 M 4 14 L 4 10 M 9 9 L 15 9 M 12 9 L 12 16"),
    ("pin", "Fijar en pantalla", "Deja la imagen flotando sobre las demás ventanas.", "M 9 4 L 15 4 L 14 10 L 17 13 L 7 13 L 10 10 Z M 12 13 L 12 20"),
    ("discard", "Descartar · Ctrl+Retroceso", "Cierra la pestaña sin guardar.", DISCARD),
];

pub const ACTIONS_MEDIA: [(&str, &str, &str, &str); 6] = [
    ("copy", "Copiar · Ctrl+C", "", COPY),
    ("save", "Guardar · Ctrl+S", "Guarda la grabación en la carpeta de capturas.", SAVE),
    ("save-as", "Guardar como… · Ctrl+Mayús+S", "Elige el nombre y la carpeta.", SAVE_AS),
    ("play", "Abrir en el reproductor", "", "M 8 5 L 19 12 L 8 19 Z"),
    ("reveal", "Mostrar en la carpeta", "", "M 3 6 L 9 6 L 11 8 L 21 8 L 21 19 L 3 19 Z"),
    ("discard", "Descartar · Ctrl+Retroceso", "Cierra la pestaña y borra la grabación.", DISCARD),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ALL_TOOLS;

    #[test]
    fn the_macos_editor_draws_these_same_icons() {
        let swift = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../macos/Sources/ShareK/Editor/Icons.swift")).unwrap();
        for item in ALL_TOOLS {
            let name = format!("{item:?}");
            let case = format!("case .{}{}: Icon(commands: \"{}\")", name[..1].to_lowercase(), &name[1..], tool(item));
            assert!(swift.contains(&case), "{case}");
        }
        let actions = ACTIONS_CAPTURE.iter().chain(&ACTIONS_MEDIA).map(|action| action.3);
        for path in actions.chain([UNDO, REDO, ROTATE, FLIP, FRAME, EYEDROPPER, FIT]) {
            assert!(swift.contains(&format!("Icon(commands: \"{path}\")")), "{path}");
        }
    }
}
