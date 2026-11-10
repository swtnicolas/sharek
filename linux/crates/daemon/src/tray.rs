use crate::app::{Action, Event, Toggle};
use crate::shortcuts;
use ksni::menu::{CheckmarkItem, StandardItem};
use ksni::{Icon, MenuItem};
use sharek_common::settings::Settings;
use sharek_common::swirl;
use std::collections::HashMap;
use smithay_client_toolkit::reexports::calloop::channel::Sender;

pub struct Tray {
    pub sender: Sender<Event>,
    pub settings: Settings,
    pub launch_at_login: bool,
    pub recording: bool,
    pub has_pins: bool,
    pub keys: HashMap<String, Vec<String>>,
    icon: Vec<Icon>,
    stop_icon: Vec<Icon>,
}

fn argb(size: usize, rgba: &[u8]) -> Icon {
    let data = rgba.as_chunks::<4>().0.iter().flat_map(|p| [p[3], p[0], p[1], p[2]]).collect();
    Icon { width: size as i32, height: size as i32, data }
}

fn stop_icon(size: usize) -> Icon {
    let factor = 4;
    let n = (size * factor) as f64;
    let center = n / 2.0;
    let mut rgba = vec![0u8; size * size * 4];
    for y in 0..size {
        for x in 0..size {
            let (mut disc, mut square) = (0u32, 0u32);
            for sy in 0..factor {
                for sx in 0..factor {
                    let px = (x * factor + sx) as f64 + 0.5 - center;
                    let py = (y * factor + sy) as f64 + 0.5 - center;
                    if px.hypot(py) <= n * 0.47 {
                        disc += 1;
                        if px.abs() <= n * 0.16 && py.abs() <= n * 0.16 {
                            square += 1;
                        }
                    }
                }
            }
            let samples = (factor * factor) as u32;
            let pixel = &mut rgba[(y * size + x) * 4..][..4];
            let red = [0xFF_u32, 0x3B, 0x30];
            for channel in 0..3 {
                pixel[channel] = ((red[channel] * (disc - square) + 255 * square) / disc.max(1)) as u8;
            }
            pixel[3] = (disc * 255 / samples) as u8;
        }
    }
    argb(size, &rgba)
}

impl Tray {
    pub fn new(sender: Sender<Event>, settings: Settings, launch_at_login: bool) -> Tray {
        let sizes = [22, 32, 48, 64];
        let icon = sizes.iter().map(|&size| argb(size, &swirl::render_flat(size, 0.5 * size as f64 / 18.0, swirl::MENU_BAR))).collect();
        let stop_icon = sizes.iter().map(|&size| stop_icon(size)).collect();
        Tray { sender, settings, launch_at_login, recording: false, has_pins: false, keys: HashMap::new(), icon, stop_icon }
    }

    fn entry(&self, label: &str, action: Action) -> MenuItem<Self> {
        StandardItem {
            label: label.into(),
            shortcut: shortcuts::SHORTCUTS
                .iter()
                .find(|shortcut| shortcut.action == action)
                .and_then(|shortcut| self.keys.get(shortcut.id))
                .filter(|keys| !keys.is_empty())
                .map(|keys| vec![keys.clone()])
                .unwrap_or_default(),
            activate: Box::new(move |tray: &mut Tray| {
                let _ = tray.sender.send(Event::Action(action, true));
            }),
            ..Default::default()
        }
        .into()
    }

    fn toggle(&self, label: &str, checked: bool, toggle: Toggle) -> MenuItem<Self> {
        CheckmarkItem {
            label: label.into(),
            checked,
            activate: Box::new(move |tray: &mut Tray| {
                let _ = tray.sender.send(Event::Action(Action::Toggle(toggle), false));
            }),
            ..Default::default()
        }
        .into()
    }
}

impl ksni::Tray for Tray {
    const MENU_ON_ACTIVATE: bool = true;

    fn id(&self) -> String {
        "sharek".into()
    }

    fn title(&self) -> String {
        "ShareK".into()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        if self.recording { self.stop_icon.clone() } else { self.icon.clone() }
    }

    fn status(&self) -> ksni::Status {
        if self.recording { ksni::Status::NeedsAttention } else { ksni::Status::Active }
    }

    fn attention_icon_pixmap(&self) -> Vec<Icon> {
        self.stop_icon.clone()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        let title = format!("ShareK {}", env!("CARGO_PKG_VERSION"));
        ksni::ToolTip { title: if self.recording { format!("{title} · grabando") } else { title }, ..Default::default() }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let settings = &self.settings;
        let mut items = Vec::new();
        if self.recording {
            items.push(self.entry("Detener", Action::StopRecording));
            items.push(MenuItem::Separator);
        }
        items.extend([
            self.entry("Capturar región", Action::CaptureRegion),
            self.entry("Capturar pantalla completa", Action::CaptureScreen),
            self.entry("Capturar ventana activa", Action::CaptureWindow),
            self.entry("Captura con desplazamiento", Action::ScrollingCapture),
            self.entry("Capturar texto (OCR)", Action::RecognizeText),
            MenuItem::Separator,
            self.entry("Fijar región en pantalla", Action::PinRegion),
            self.entry("Fijar imagen del portapapeles", Action::PinClipboard),
            StandardItem {
                label: "Cerrar imágenes fijadas".into(),
                enabled: self.has_pins,
                activate: Box::new(|tray: &mut Tray| {
                    let _ = tray.sender.send(Event::Action(Action::ClosePins, false));
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            self.entry("Grabar video (MP4)", Action::RecordVideo),
            self.entry("Grabar GIF", Action::RecordGif),
            MenuItem::Separator,
            self.entry("Selector de color", Action::PickColor),
            self.entry("Regla", Action::Measure),
            self.entry("Historial…", Action::ShowHistory),
            MenuItem::Separator,
            self.toggle("Copiar al portapapeles al capturar", settings.copy_on_capture, Toggle::CopyOnCapture),
            self.toggle("Guardar automáticamente", settings.auto_save, Toggle::AutoSave),
            self.toggle("Abrir editor al capturar", settings.open_editor, Toggle::OpenEditor),
            self.toggle("Incluir cursor en capturas", settings.shows_cursor, Toggle::ShowsCursor),
            self.toggle("Grabar audio del sistema", settings.records_audio, Toggle::RecordsAudio),
            self.toggle("Abrir al iniciar sesión", self.launch_at_login, Toggle::LaunchAtLogin),
            MenuItem::Separator,
            self.entry("Elegir carpeta de guardado…", Action::ChooseFolder),
            self.entry("Abrir carpeta de guardado", Action::OpenFolder),
            MenuItem::Separator,
            self.entry("Salir de ShareK", Action::Quit),
        ]);
        items
    }
}
