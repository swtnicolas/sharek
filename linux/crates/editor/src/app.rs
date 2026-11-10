use crate::canvas::{CaptureTab, Changes};
use crate::document::Document;
use crate::history::{Command, HistoryView};
use crate::icons;
use crate::media::MediaTab;
use crate::model::{self, Frame, Rgba, RgbaImage, ShapeKind, ShapeStyle, StyleStore, Tool, option};
use crate::render::{self, Renderer};
use crate::thumbnail::Thumbnail;
use crate::view;
use crate::{EditorWindow, OptionItem, Swatch, TabItem, TextWindow, ToolGroup, ToolItem};
use sharek_common::image::{self, RgbImage};
use sharek_common::ipc::{self, ToDaemon, ToEditor};
use sharek_common::mapped;
use sharek_common::portal;
use sharek_common::settings::Settings;
use sharek_common::text::Font;
use sharek_common::history;
use slint::winit_030::WinitWindowAccessor;
use slint::{ComponentHandle, Image, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, VecModel};
use std::cell::RefCell;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tiny_skia::PixmapMut;

const LOCAL_IDS: u64 = 1 << 48;
const RELEASE_DELAY: Duration = Duration::from_secs(2);

type SceneKey = (usize, u32, u32, u32, u32, u32, u32);

struct Scene {
    key: SceneKey,
    pixels: Vec<u8>,
    drawn: Vec<u64>,
}
const LINE_WIDTHS: [f32; 12] = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 8.0, 10.0, 12.0, 16.0, 20.0];
const RADII: [f32; 7] = [0.0, 3.0, 6.0, 10.0, 15.0, 20.0, 30.0];
const FONT_SIZES: [f32; 14] = [10.0, 12.0, 14.0, 16.0, 18.0, 20.0, 24.0, 28.0, 32.0, 40.0, 48.0, 64.0, 80.0, 96.0];
const SWATCHES: [(u32, &str); 16] = [
    (0xF23C3CFF, "Rojo"),
    (0xFF9500FF, "Naranja"),
    (0xFFCC00FF, "Amarillo"),
    (0x34C759FF, "Verde"),
    (0x30B0C7FF, "Turquesa"),
    (0x0A84FFFF, "Azul"),
    (0xAF52DEFF, "Morado"),
    (0xFF2D55FF, "Rosa"),
    (0xFFFFFFFF, "Blanco"),
    (0xC7C7CCFF, "Gris claro"),
    (0x8E8E93FF, "Gris"),
    (0x3A3A3CFF, "Gris oscuro"),
    (0x2A2F38FF, "Pizarra"),
    (0x000000FF, "Negro"),
    (0xFFFF00FF, "Amarillo flúor"),
    (0x00000000, "Transparente"),
];
const FRAME_SWATCHES: [u32; 9] = [0xFFFFFFFF, 0xF2F3F5FF, 0x8B939CFF, 0x2A2F38FF, 0x000000FF, 0xFF3D7FFF, 0xFF9F1CFF, 0x2EC4B6FF, 0x5B5BFFFF];
const GROUP_RANKS: [i32; 4] = [3, 5, 2, 1];
const COPY_COMPACT_LEVEL: usize = 4;
const BUTTON: f32 = 34.0;
const GROUP_BUTTON: f32 = 48.0;
const GROUP_GAP: f32 = 10.0;
const SEPARATOR: f32 = 13.0;
const fn hex(value: u32) -> Rgba {
    Rgba { r: ((value >> 16) & 0xff) as f32 / 255.0, g: ((value >> 8) & 0xff) as f32 / 255.0, b: (value & 0xff) as f32 / 255.0, a: 1.0 }
}

const NEUTRAL: Rgba = hex(0xd5d9de);
const DIM: Rgba = hex(0x9aa1a9);
const POINTER: Rgba = hex(0xe8eaed);
const FILE: Rgba = hex(0x5b9bff);
const RECOGNIZE: Rgba = hex(0x2ec4b6);
const PIN: Rgba = hex(0xff9f1c);
const HISTORY: Rgba = hex(0xffb020);
const TRANSFORM: Rgba = hex(0x34c759);
const ERASE: Rgba = hex(0xff7ab6);
const CONCEAL: Rgba = hex(0xaf7bff);
const FOLDER: Rgba = hex(0xffc94d);
const DANGER: Rgba = hex(0xff6b6b);

pub enum Tab {
    Capture(Box<CaptureTab>),
    Media(Box<MediaTab>),
}

impl Tab {
    fn id(&self) -> u64 {
        match self {
            Tab::Capture(tab) => tab.doc.id,
            Tab::Media(tab) => tab.id,
        }
    }

    fn title(&self) -> String {
        match self {
            Tab::Capture(tab) => tab.doc.title.clone(),
            Tab::Media(tab) => tab.title.clone(),
        }
    }

    fn edited(&self) -> bool {
        match self {
            Tab::Capture(tab) => tab.doc.has_unsaved_changes(),
            Tab::Media(tab) => !tab.is_saved,
        }
    }
}

#[derive(Clone)]
pub struct Link {
    stream: Arc<Mutex<UnixStream>>,
}

impl Link {
    pub fn new(stream: UnixStream) -> Link {
        Link { stream: Arc::new(Mutex::new(stream)) }
    }

    pub fn send(&self, message: &ToDaemon, payload: &[u8]) {
        if let Ok(mut stream) = self.stream.lock() {
            let _ = ipc::send(&mut *stream, message, payload);
        }
    }
}

pub struct Controller {
    window: EditorWindow,
    tabs: Vec<Tab>,
    current: usize,
    renderer: Renderer,
    styles: StyleStore,
    tool: Tool,
    link: Link,
    coalescing: Option<(String, u64, Instant)>,
    choices: Vec<(String, Vec<f32>)>,
    clipboard_owner: Option<u64>,
    clipboard_timer: slint::Timer,
    redraw_pending: bool,
    edit_shown: Option<u64>,
    scene: Option<Scene>,
    text_windows: Vec<TextWindow>,
    received: u64,
    exiting: bool,
    pending_paste: bool,
    history: Option<HistoryView>,
    local_ids: u64,
    frame: Frame,
    picking: Option<String>,
    group_tools: [Tool; 4],
    drag: Option<crate::drag::Drag>,
    drag_timer: slint::Timer,
    release_timer: slint::Timer,
}

thread_local! {
    static CONTROLLER: RefCell<Option<Controller>> = const { RefCell::new(None) };
}

pub fn try_with<R>(body: impl FnOnce(&mut Controller) -> R) -> Option<R> {
    CONTROLLER.with(|cell| cell.try_borrow_mut().ok().and_then(|mut controller| controller.as_mut().map(body)))
}

fn answer(body: impl FnOnce(&mut Controller) -> bool) -> bool {
    CONTROLLER.with(|cell| cell.try_borrow_mut().ok().and_then(|mut controller| controller.as_mut().map(body)).unwrap_or(false))
}

pub fn with(body: impl FnOnce(&mut Controller) + 'static) {
    let deferred = CONTROLLER.with(|cell| match cell.try_borrow_mut() {
        Ok(mut controller) => {
            if let Some(controller) = controller.as_mut() {
                body(controller);
            }
            None
        }
        Err(_) => Some(body),
    });
    if let Some(body) = deferred {
        slint::Timer::single_shot(Duration::ZERO, move || with(body));
    }
}

fn color(value: Rgba) -> slint::Color {
    let channel = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    slint::Color::from_argb_u8(channel(value.a), channel(value.r), channel(value.g), channel(value.b))
}

fn rgba(value: slint::Color) -> Rgba {
    Rgba { r: f32::from(value.red()) / 255.0, g: f32::from(value.green()) / 255.0, b: f32::from(value.blue()) / 255.0, a: f32::from(value.alpha()) / 255.0 }
}

fn trim_memory() {
    unsafe {
        libc::malloc_trim(0);
    }
}

pub fn start(link: Link) {
    let window = EditorWindow::new().expect("editor window");
    let _ = slint::set_xdg_app_id(sharek_common::paths::APP_ID);
    let font = Font::bold().expect("Noto Sans Bold");
    let tool = std::fs::read_to_string(sharek_common::paths::config_dir().join("last-tool")).ok().and_then(|name| Tool::named(name.trim())).unwrap_or(Tool::Rectangle);
    window.set_thresholds(ModelRc::new(VecModel::from(thresholds().to_vec())));
    window.set_undo_icon(icons::UNDO.into());
    window.set_history_color(color(HISTORY));
    window.set_redo_icon(icons::REDO.into());
    window.set_eyedropper_icon(icons::EYEDROPPER.into());
    window.set_fit_icon(icons::FIT.into());
    let image_tools = [
        ("rotate", "Girar 90°", "Gira la imagen en el sentido del reloj. Con Mayús, al revés.", icons::ROTATE),
        ("flip", "Voltear", "Refleja la imagen de izquierda a derecha. Con Mayús, de arriba abajo.", icons::FLIP),
        ("frame", "Marco", "Agrega margen, fondo, esquinas redondeadas, borde o sombra a toda la imagen.", icons::FRAME),
    ];
    let image_tools: Vec<ToolItem> = image_tools
        .iter()
        .map(|(name, title, help, icon)| ToolItem { name: (*name).into(), title: (*title).into(), help: (*help).into(), icon: (*icon).into(), stroke: color(action_color(name)), ..Default::default() })
        .collect();
    window.set_image_tools(ModelRc::new(VecModel::from(image_tools)));
    let floats = |values: &[f32]| ModelRc::new(VecModel::from(values.to_vec()));
    window.set_frame_margins(floats(&Frame::MARGINS));
    window.set_frame_radii(floats(&Frame::RADII));
    window.set_frame_borders(floats(&Frame::BORDERS));
    let frame_swatches: Vec<Swatch> = FRAME_SWATCHES.iter().map(|value| Swatch { color: rgba_hex(*value), label: SharedString::default() }).collect();
    window.set_frame_swatches(ModelRc::new(VecModel::from(frame_swatches)));
    let swatches: Vec<Swatch> = SWATCHES
        .iter()
        .map(|(value, label)| Swatch {
            color: rgba_hex(*value),
            label: (*label).into(),
        })
        .collect();
    window.set_swatches(ModelRc::new(VecModel::from(swatches)));
    bind(&window);
    let weak = window.as_weak();
    window.window().on_winit_window_event(move |_, event| {
        if matches!(event, slint::winit_030::winit::event::WindowEvent::ScaleFactorChanged { .. })
            && let Some(window) = weak.upgrade() {
                window.set_repaint(!window.get_repaint());
            }
        slint::winit_030::EventResult::Propagate
    });
    let controller = Controller {
        window,
        tabs: Vec::new(),
        current: 0,
        renderer: Renderer::new(font),
        styles: StyleStore::load(),
        tool,
        link,
        coalescing: None,
        choices: Vec::new(),
        clipboard_owner: None,
        clipboard_timer: slint::Timer::default(),
        redraw_pending: false,
        edit_shown: None,
        scene: None,
        text_windows: Vec::new(),
        received: 0,
        exiting: false,
        pending_paste: false,
        history: None,
        local_ids: LOCAL_IDS,
        frame: Frame::load(),
        picking: None,
        group_tools: std::array::from_fn(|index| model::SECTIONS[index].1[0]),
        drag: None,
        drag_timer: slint::Timer::default(),
        release_timer: slint::Timer::default(),
    };
    CONTROLLER.with(|cell| *cell.borrow_mut() = Some(controller));
    with(|c| {
        c.refresh_groups();
        c.refresh_frame();
    });
}

fn rgba_hex(value: u32) -> slint::Color {
    slint::Color::from_argb_u8((value & 0xFF) as u8, (value >> 24) as u8, (value >> 16 & 0xFF) as u8, (value >> 8 & 0xFF) as u8)
}

fn thresholds() -> [f32; 5] {
    let fixed = |compact: bool| {
        let copy = if compact { BUTTON + 6.0 } else { 104.0 };
        20.0 + copy + 5.0 * BUTTON + SEPARATOR + 2.0 * BUTTON + SEPARATOR + SEPARATOR + 3.0 * BUTTON + SEPARATOR + BUTTON + 8.0
    };
    std::array::from_fn(|level| {
        let groups: f32 = model::SECTIONS
            .iter()
            .zip(GROUP_RANKS)
            .map(|((_, tools), rank)| if rank as usize <= level { GROUP_BUTTON } else { tools.len() as f32 * BUTTON })
            .sum();
        fixed(level >= COPY_COMPACT_LEVEL) + groups + GROUP_GAP * (model::SECTIONS.len() - 1) as f32
    })
}

fn bright(value: Rgba) -> Option<Rgba> {
    let luminance = 0.2126 * value.r + 0.7152 * value.g + 0.0722 * value.b;
    (value.is_visible() && luminance >= 0.18).then_some(Rgba { a: 1.0, ..value })
}

fn legible(value: Rgba) -> Option<Rgba> {
    value.is_visible().then(|| bright(value).unwrap_or(DIM))
}

pub fn icon_colors(tool: Tool, style: Option<ShapeStyle>) -> (Rgba, Rgba) {
    let meaning = match tool {
        Tool::Select => Some(POINTER),
        Tool::Crop | Tool::CutOut => Some(TRANSFORM),
        Tool::SmartEraser => Some(ERASE),
        Tool::Blur | Tool::Pixelate | Tool::Spotlight => Some(CONCEAL),
        _ => None,
    };
    if let Some(color) = meaning {
        return (color, Rgba::CLEAR);
    }
    let Some(style) = style else { return (NEUTRAL, Rgba::CLEAR) };
    let fill = if style.fill.is_visible() { style.fill } else { Rgba::CLEAR };
    let line = |color: Rgba| if style.line_width > 0.0 { legible(color) } else { None };
    match tool {
        Tool::Rectangle | Tool::Ellipse | Tool::SpeechBalloon => (line(style.stroke).or(legible(style.fill)).unwrap_or(NEUTRAL), fill),
        Tool::Line | Tool::Arrow | Tool::Freehand | Tool::Magnify => (legible(style.stroke).unwrap_or(NEUTRAL), Rgba::CLEAR),
        Tool::Highlight => (legible(style.fill).unwrap_or(NEUTRAL), fill),
        Tool::TextOutline => (bright(style.text).or(line(style.stroke)).unwrap_or(NEUTRAL), Rgba::CLEAR),
        Tool::TextBackground | Tool::Step => (legible(style.text).unwrap_or(NEUTRAL), fill),
        _ => (NEUTRAL, Rgba::CLEAR),
    }
}

const WINDOW_MINIMUM: (f32, f32) = (720.0, 480.0);
const WINDOW_CHROME: (f32, f32) = (80.0, 200.0);
const WINDOW_SCREEN_SHARE: f32 = 0.85;

pub fn fitted_window(content: (f32, f32), screen: Option<(f32, f32)>, toolbar: f32) -> (f32, f32) {
    let (screen_width, screen_height) = screen.unwrap_or((1920.0, 1080.0));
    let fit = |content: f32, chrome: f32, minimum: f32, screen: f32| (content + chrome).max(minimum).min(screen * WINDOW_SCREEN_SHARE).max(WINDOW_MINIMUM.0.min(minimum)).round();
    (fit(content.0, WINDOW_CHROME.0, WINDOW_MINIMUM.0.max(toolbar), screen_width), fit(content.1, WINDOW_CHROME.1, WINDOW_MINIMUM.1, screen_height))
}

pub fn selected_text(text: &str, anchor: i32, cursor: i32) -> &str {
    let (start, end) = (anchor.min(cursor).max(0) as usize, anchor.max(cursor).max(0) as usize);
    if start == end {
        return text;
    }
    text.get(start..end.min(text.len())).unwrap_or(text)
}

pub fn action_color(name: &str) -> Rgba {
    match name {
        "save" | "save-as" => FILE,
        "ocr" | "ocr-area" => RECOGNIZE,
        "pin" => PIN,
        "undo" | "redo" => HISTORY,
        "rotate" | "flip" | "frame" | "play" => TRANSFORM,
        "reveal" => FOLDER,
        "discard" => DANGER,
        _ => NEUTRAL,
    }
}

fn bind(window: &EditorWindow) {
    window.on_select_tool(|name| with(move |c| c.select_tool_named(&name)));
    window.on_select_tab(|index| with(move |c| c.switch_tab(index as usize)));
    window.on_close_tab(|index| with(move |c| c.close_tab(index as usize, false)));
    window.on_pointer(|kind, button, x, y, shift, _ctrl| with(move |c| c.pointer(&kind, &button, x, y, shift)));
    window.on_scrolled(|dx, dy, x, y, ctrl| with(move |c| c.scrolled(dx, dy, x, y, ctrl)));
    window.on_key(|text, ctrl, shift, alt| answer(|c| c.key(&text, ctrl, shift, alt)));
    window.on_text_history(|redo| answer(|c| c.text_history(redo)));
    window.on_caret(|cursor, anchor| with(move |c| c.caret_moved(cursor, anchor)));
    window.on_option_color(|key, value| with(move |c| c.after_control(|c| c.option_color(&key, value))));
    window.on_option_choice(|key, index| with(move |c| c.after_control(|c| c.option_choice(&key, index as usize))));
    window.on_option_check(|key, value| with(move |c| c.after_control(|c| c.option_check(&key, value))));
    window.on_option_button(|key| with(move |c| c.after_control(|c| c.option_button(&key))));
    window.on_action(|name, shift| with(move |c| c.toolbar(&name, shift)));
    window.on_pick_color(|key| with(move |c| c.start_picking(&key)));
    window.on_drag_out(|| with(|c| c.drag_out()));
    window.on_zoom(|kind| {
        with(move |c| match kind.as_str() {
            "in" => c.zoom(Some(1.25)),
            "out" => c.zoom(Some(0.8)),
            "fit" => c.zoom(None),
            _ => c.actual_size(),
        })
    });
    window.on_frame_set(|key, value| with(move |c| c.after_control(|c| c.frame_set(&key, value))));
    window.on_frame_color(|key, value| with(move |c| c.after_control(|c| c.frame_color(&key, value))));
    window.on_frame_apply(|| with(|c| c.after_control(|c| c.frame_apply())));
    window.on_text_changed(|text| with(move |c| c.text_changed(&text)));
    window.on_text_finished(|| with(|c| c.text_finished()));
    window.on_context_action(|name| with(move |c| c.context_action(&name)));
    window.on_canvas_resized(|| with(|c| c.request_redraw()));
    window.window().on_close_requested(|| {
        with(|c| c.close_all());
        slint::CloseRequestResponse::HideWindow
    });
}

impl Controller {
    pub fn handle(&mut self, message: ToEditor, payload: Vec<u8>) {
        self.received += 1;
        let offer_exit = !matches!(message, ToEditor::ExitAccepted);
        match message {
            ToEditor::Open { id, width, height, scale, created, saved } => {
                if payload.len() == width as usize * height as usize * 3 {
                    let base = RgbImage { width, height, data: payload.into() };
                    let doc = Document::new(id, base, scale.max(0.1) as f32, created, saved);
                    self.open(Tab::Capture(Box::new(CaptureTab::new(doc, self.tool))));
                }
            }
            ToEditor::OpenMedia { id, path, gif, width, height, created, saved_automatically } => {
                let tab = MediaTab::new(id, path, gif, (width, height), created, saved_automatically);
                self.open(Tab::Media(Box::new(tab)));
            }
            ToEditor::ShowHistory => self.show_history(),
            ToEditor::ClipboardOwner { id } => self.clipboard_owner = id,
            ToEditor::PasteImage { scale } => self.paste_image(&payload, scale),
            ToEditor::ExitAccepted => {
                if self.exiting && self.tabs.is_empty() {
                    let _ = slint::quit_event_loop();
                }
            }
        }
        if offer_exit {
            self.maybe_exit();
        }
    }

    fn open(&mut self, tab: Tab) {
        self.exiting = false;
        self.tabs.push(tab);
        self.current = self.tabs.len() - 1;
        self.refresh_tabs();
        self.refresh_tab_view();
        if !self.window.window().is_visible() {
            let (width, height) = self.window_size();
            self.window.window().set_size(slint::LogicalSize::new(width, height));
        }
        let _ = self.window.show();
        self.link.send(&ToDaemon::Activate { history: false }, &[]);
        self.window.invoke_focus_canvas();
        if self.drag.is_none() {
            slint::Timer::single_shot(Duration::from_millis(200), || with(|c| c.attach_drag()));
        }
    }

    fn window_size(&self) -> (f32, f32) {
        let content = match self.tabs.get(self.current) {
            Some(Tab::Capture(tab)) => tab.doc.canvas(),
            Some(Tab::Media(tab)) => (tab.size.0 as f32, tab.size.1 as f32),
            None => (0.0, 0.0),
        };
        let screen = self.window.window().with_winit_window(|window| {
            window.available_monitors().map(|monitor| monitor.size().to_logical::<f32>(monitor.scale_factor())).map(|size| (size.width, size.height)).reduce(|a, b| (a.0.min(b.0), a.1.min(b.1)))
        });
        fitted_window(content, screen.flatten(), thresholds()[0])
    }

    fn attach_drag(&mut self) {
        if self.drag.is_some() || !self.window.window().is_visible() {
            return;
        }
        self.drag = crate::drag::Drag::attach(self.window.window());
        if self.drag.is_some() {
            self.drag_timer.start(slint::TimerMode::Repeated, Duration::from_millis(100), || {
                with(|c| {
                    if let Some(drag) = c.drag.as_mut() {
                        drag.pump();
                    }
                })
            });
        }
    }

    fn detach_drag(&mut self) {
        self.drag_timer.stop();
        self.drag = None;
    }

    fn drag_out(&mut self) {
        if self.drag.as_ref().is_none_or(|drag| drag.active()) {
            return;
        }
        let Some((_, image, scale)) = self.rendered() else { return };
        let Some(Tab::Capture(tab)) = self.tabs.get(self.current) else { return };
        let Some(file) = crate::drag::drag_file(tab.doc.created) else { return };
        let Ok(png) = image::encode_png(&image, f64::from(scale), png::Compression::Fast) else { return };
        if !self.drag.as_mut().is_some_and(|drag| drag.start(png, file)) {
            self.toast("No se pudo arrastrar la captura", None);
        }
    }

    fn capture(&mut self) -> Option<&mut CaptureTab> {
        match self.tabs.get_mut(self.current) {
            Some(Tab::Capture(tab)) => Some(tab),
            _ => None,
        }
    }

    fn media(&mut self) -> Option<&mut MediaTab> {
        match self.tabs.get_mut(self.current) {
            Some(Tab::Media(tab)) => Some(tab),
            _ => None,
        }
    }

    fn refresh_tabs(&self) {
        let items: Vec<TabItem> = self.tabs.iter().map(|tab| TabItem { title: tab.title().into(), edited: tab.edited() }).collect();
        self.window.set_tabs(ModelRc::new(VecModel::from(items)));
        self.window.set_current_tab(self.current as i32);
        let title = match self.tabs.get(self.current) {
            Some(Tab::Capture(tab)) => format!("{} — {}", tab.doc.title, tab.doc.subtitle()),
            Some(Tab::Media(tab)) => format!("{} — {}", tab.title, tab.subtitle()),
            None => "ShareK".into(),
        };
        self.window.set_window_title(title.into());
    }

    fn refresh_tab_view(&mut self) {
        mapped::page_out();
        let is_media = matches!(self.tabs.get(self.current), Some(Tab::Media(_)));
        self.window.set_media(is_media);
        let actions: Vec<ToolItem> = (if is_media { &icons::ACTIONS_MEDIA[..] } else { &icons::ACTIONS_CAPTURE[..] })
            .iter()
            .map(|(name, title, help, icon)| ToolItem {
                name: (*name).into(),
                title: (*title).into(),
                help: (*help).into(),
                icon: (*icon).into(),
                stroke: color(action_color(name)),
                ..Default::default()
            })
            .collect();
        self.window.set_actions(ModelRc::new(VecModel::from(actions)));
        if is_media {
            let window = self.window.as_weak();
            if let Some(media) = self.media() {
                media.show(window);
            }
            return;
        }
        let tool = self.capture().map(|tab| tab.tool).unwrap_or(self.tool);
        self.window.set_tool(tool.name().into());
        self.refresh_options();
        self.refresh_editing();
        self.request_redraw();
    }

    fn switch_tab(&mut self, index: usize) {
        if index >= self.tabs.len() || index == self.current {
            return;
        }
        self.finish_text();
        if let Some(media) = self.media() {
            media.hide();
        }
        self.current = index;
        self.scene = None;
        self.refresh_tabs();
        self.refresh_tab_view();
    }

    pub fn request_redraw(&mut self) {
        if self.redraw_pending {
            return;
        }
        self.redraw_pending = true;
        slint::Timer::single_shot(Duration::ZERO, || with(|c| c.redraw()));
    }

    fn redraw(&mut self) {
        self.redraw_pending = false;
        let size = (self.window.get_canvas_width(), self.window.get_canvas_height());
        let device = self.window.window().scale_factor();
        let zoom_label;
        {
            let Some(Tab::Capture(tab)) = self.tabs.get_mut(self.current) else { return };
            let canvas = tab.doc.canvas();
            let resized = tab.view.size != (size.0.max(1.0), size.1.max(1.0));
            tab.view.size = (size.0.max(1.0), size.1.max(1.0));
            tab.view.device = device;
            if !tab.fitted || (tab.auto_fit && resized) {
                tab.fitted = true;
                tab.auto_fit = true;
                tab.view.zoom = 1.0;
                if !tab.view.fits_at_actual_size(canvas) {
                    tab.view.fit(canvas);
                }
            }
            tab.view.clamp(canvas);
            zoom_label = format!("{} %", (tab.view.zoom * 100.0).round());
        }
        let Some(Tab::Capture(tab)) = self.tabs.get_mut(self.current) else { return };
        let (width, height) = tab.view.device_size();
        let transform = tab.view.device_transform(tab.doc.canvas());
        let key = (
            Arc::as_ptr(&tab.doc.state.base) as usize,
            transform.sx.to_bits(),
            transform.tx.to_bits(),
            transform.ty.to_bits(),
            width,
            height,
            tab.doc.scale.to_bits(),
        );
        let state = &tab.doc.state;
        let live = tab.live_from();
        let settled = render::fingerprints(&state.shapes[..live]);
        let mut scene = match self.scene.take() {
            Some(scene) if scene.key == key && settled.starts_with(&scene.drawn) => scene,
            reused => {
                let mut pixels = reused.map(|scene| scene.pixels).unwrap_or_default();
                pixels.resize(width as usize * height as usize * 4, 0);
                view::render_base(&state.base, tab.doc.scale, &mut tab.mips, transform, &mut pixels, width, height);
                Scene { key, pixels, drawn: Vec::new() }
            }
        };
        if scene.drawn.len() < live
            && let Some(mut pixmap) = PixmapMut::from_bytes(&mut scene.pixels, width, height) {
                self.renderer.draw_shapes(state, tab.doc.scale, &mut pixmap, transform, scene.drawn.len()..live);
            }
        scene.drawn = settled;
        let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&scene.pixels, width, height);
        self.scene = Some(scene);
        if let Some(mut pixmap) = PixmapMut::from_bytes(buffer.make_mut_bytes(), width, height) {
            self.renderer.draw_shapes(state, tab.doc.scale, &mut pixmap, transform, live..state.shapes.len());
            tab.draw_overlay(&mut pixmap, transform, &self.renderer.font);
        }
        self.renderer.forget_removed(state);
        self.window.set_canvas(Image::from_rgba8_premultiplied(buffer));
        self.window.set_zoom_label(zoom_label.into());
        self.window.set_has_selection(tab.selected.is_some());
        self.refresh_editing();
        self.release_timer.start(slint::TimerMode::SingleShot, RELEASE_DELAY, mapped::page_out);
    }

    fn refresh_editing(&mut self) {
        let font_size_scale;
        let shape = match self.tabs.get(self.current) {
            Some(Tab::Capture(tab)) => tab.editing.and_then(|id| tab.doc.state.shapes.iter().find(|shape| shape.id == id)),
            _ => None,
        };
        let Some(id) = shape.map(|shape| shape.id) else {
            if self.edit_shown.take().is_some() || self.window.get_editing() {
                self.window.set_editing(false);
                self.window.invoke_focus_canvas();
            }
            return;
        };
        let props = {
            let Some(Tab::Capture(tab)) = self.tabs.get(self.current) else { return };
            let Some(shape) = tab.doc.state.shapes.iter().find(|shape| shape.id == id) else { return };
            font_size_scale = tab.view.zoom;
            let canvas = tab.doc.canvas();
            let rect = shape.rect();
            let (x, y) = tab.view.to_view(canvas, model::Point::new(rect.x, rect.y));
            let padding = crate::geometry::padding(shape);
            (x, y, rect.width * tab.view.zoom, rect.height * tab.view.zoom, padding, shape.style.font_size, shape.text.clone())
        };
        let (x, y, width, height, padding, font_size, text) = props;
        let window = &self.window;
        window.set_edit_x(x);
        window.set_edit_y(y);
        window.set_edit_width(width);
        window.set_edit_height(height);
        window.set_edit_padding_x((padding.0 + 1.0) * font_size_scale);
        window.set_edit_padding_y(padding.1 * font_size_scale);
        window.set_edit_font_size(font_size * font_size_scale);
        if self.edit_shown != Some(id) {
            self.edit_shown = Some(id);
            window.set_editing(true);
            window.invoke_begin_text(text.into());
        }
    }

    fn apply(&mut self, changes: Changes) {
        if let Some(tool) = changes.tool {
            self.set_tool(tool);
        }
        if changes.content {
            self.coalescing = None;
            self.document_changed();
        }
        if changes.selection || changes.content {
            self.coalescing = None;
            self.refresh_options();
        }
        if changes.selection {
            let selected = self.capture().is_some_and(|tab| tab.selected.is_some());
            self.window.set_has_selection(selected);
        }
        if changes.begin_edit || changes.end_edit {
            self.redraw();
        } else if changes.redraw || changes.content {
            self.request_redraw();
        }
        let selecting = self.capture().is_some_and(|tab| tab.selecting_text);
        self.window.set_selecting_text(selecting);
        if let Some(area) = changes.recognize {
            self.recognize_area(area);
        }
    }

    fn recognize_area(&mut self, area: model::Rect) {
        let Some((_, image, scale)) = self.rendered() else { return };
        let pixels = model::Rect { x: area.x * scale, y: area.y * scale, width: area.width * scale, height: area.height * scale }.integral();
        if let Some(crop) = image.crop(pixels.x as i64, pixels.y as i64, pixels.width as i64, pixels.height as i64) {
            crate::ocr::recognize(Arc::new(crop), scale);
        }
    }

    fn document_changed(&mut self) {
        self.refresh_tabs();
        let Some(Tab::Capture(tab)) = self.tabs.get(self.current) else { return };
        if self.clipboard_owner == Some(tab.doc.id) {
            let id = tab.doc.id;
            self.clipboard_timer.start(slint::TimerMode::SingleShot, Duration::from_millis(600), move || {
                with(move |c| c.send_png(id, false));
            });
        }
    }

    fn set_tool(&mut self, tool: Tool) {
        let font = &self.renderer.font;
        let Some(Tab::Capture(tab)) = self.tabs.get_mut(self.current) else { return };
        let changes = tab.set_tool(tool, font);
        self.tool = tool;
        if tool != Tool::Crop && tool != Tool::CutOut {
            let _ = std::fs::create_dir_all(sharek_common::paths::config_dir());
            let _ = std::fs::write(sharek_common::paths::config_dir().join("last-tool"), tool.name());
        }
        self.window.set_tool(tool.name().into());
        if let Some(index) = model::SECTIONS.iter().position(|(_, tools)| tools.contains(&tool)) {
            self.group_tools[index] = tool;
        }
        self.picking = None;
        self.window.set_picking(false);
        self.apply(changes);
        self.refresh_options();
        self.refresh_groups();
        self.window.invoke_focus_canvas();
    }

    fn refresh_groups(&mut self) {
        let current = self.capture().map(|tab| tab.tool).unwrap_or(self.tool);
        let groups: Vec<ToolGroup> = model::SECTIONS
            .iter()
            .zip(GROUP_RANKS)
            .enumerate()
            .map(|(index, ((_, tools), rank))| {
                let shown = if tools.contains(&current) { current } else { self.group_tools[index] };
                let items: Vec<ToolItem> = tools
                    .iter()
                    .map(|tool| {
                        let (stroke, fill) = icon_colors(*tool, tool.kind().map(|kind| self.styles.style(kind)));
                        ToolItem {
                            name: tool.name().into(),
                            title: tool.title().into(),
                            help: tool.help().into(),
                            icon: icons::tool(*tool).into(),
                            key: tool.shortcut().to_uppercase().to_string().into(),
                            stroke: color(stroke),
                            fill: color(fill),
                        }
                    })
                    .collect();
                ToolGroup { tools: ModelRc::new(VecModel::from(items)), current: tools.iter().position(|tool| *tool == shown).unwrap_or(0) as i32, rank }
            })
            .collect();
        self.window.set_groups(ModelRc::new(VecModel::from(groups)));
    }

    fn refresh_frame(&self) {
        let frame = &self.frame;
        self.window.set_frame_margin(frame.margin);
        self.window.set_frame_radius(frame.radius);
        self.window.set_frame_border(frame.border);
        self.window.set_frame_shadow(frame.shadow);
        self.window.set_frame_background(color(frame.background));
        self.window.set_frame_border_color(color(frame.border_color));
    }

    fn frame_set(&mut self, key: &str, value: f32) {
        match key {
            "margin" => self.frame.margin = value,
            "radius" => self.frame.radius = value,
            "border" => self.frame.border = value,
            "shadow" => self.frame.shadow = value > 0.5,
            _ => return,
        }
        self.frame.save();
        self.refresh_frame();
    }

    fn frame_color(&mut self, key: &str, value: slint::Color) {
        match key {
            "background" => self.frame.background = rgba(value),
            "border" => self.frame.border_color = rgba(value),
            _ => return,
        }
        self.frame.save();
        self.refresh_frame();
    }

    fn frame_apply(&mut self) {
        let frame = self.frame;
        let font = &self.renderer.font;
        let Some(Tab::Capture(tab)) = self.tabs.get_mut(self.current) else { return };
        let changes = tab.apply_frame(&frame, font);
        self.apply(changes);
    }

    fn transform_image(&mut self, rotate: bool, alternate: bool) {
        let font = &self.renderer.font;
        let Some(Tab::Capture(tab)) = self.tabs.get_mut(self.current) else { return };
        let changes = if rotate { tab.rotate(!alternate, font) } else { tab.flip(!alternate, font) };
        self.apply(changes);
    }

    fn start_picking(&mut self, key: &str) {
        self.picking = Some(key.to_string());
        self.window.set_picking(true);
        self.window.invoke_focus_canvas();
    }

    fn pick_at(&mut self, key: &str, x: f32, y: f32) {
        let Some((_, image, scale)) = self.rendered() else { return };
        let Some(tab) = self.capture() else { return };
        let point = tab.view.to_canvas(tab.doc.canvas(), x, y);
        let px = ((point.x * scale) as i64).clamp(0, i64::from(image.width) - 1) as u32;
        let py = ((point.y * scale) as i64).clamp(0, i64::from(image.height) - 1) as u32;
        let value = color(Rgba::from_rgb8(image.pixel(px, py)));
        self.option_color(key, value);
    }

    fn toolbar(&mut self, name: &str, shift: bool) {
        match name {
            "undo" => self.undo(true),
            "redo" => self.undo(false),
            "rotate" => self.transform_image(true, shift),
            "flip" => self.transform_image(false, shift),
            _ => self.action(name),
        }
    }

    fn select_tool_named(&mut self, name: &str) {
        if let Some(tool) = Tool::named(name) {
            self.set_tool(tool);
        }
    }

    fn refresh_options(&mut self) {
        let Some(Tab::Capture(tab)) = self.tabs.get(self.current) else { return };
        let tool = tab.tool;
        let selected = tab.selected_shape();
        let kind = selected.map(|shape| shape.kind).or(tool.kind());
        let style = selected.map(|shape| shape.style).or(kind.map(|kind| self.styles.style(kind)));
        let mut items = Vec::new();
        let mut choices = Vec::new();
        let name = match selected.map(|shape| shape.kind) {
            Some(ShapeKind::Image) => "Imagen",
            Some(kind) => model::SECTIONS.iter().flat_map(|(_, tools)| tools.iter()).find(|tool| tool.kind() == Some(kind)).map_or("", |tool| tool.title()),
            None => tool.title(),
        };
        self.window.set_options_title(name.into());
        let hint = |text: &str| OptionItem { kind: "hint".into(), label: text.into(), ..Default::default() };
        match tool {
            Tool::Crop => {
                items.push(hint("Arrastra para elegir el área. Enter o doble clic aplica; Esc cancela."));
                items.push(OptionItem { kind: "button".into(), key: "apply-crop".into(), label: "Aplicar".into(), ..Default::default() });
                items.push(OptionItem { kind: "button".into(), key: "cancel-crop".into(), label: "Cancelar".into(), ..Default::default() });
            }
            Tool::CutOut => items.push(hint("Arrastra en horizontal para quitar columnas o en vertical para quitar filas.")),
            _ => match (kind, style) {
                (Some(ShapeKind::SmartEraser), _) => items.push(hint("Rellena el área con el color del punto donde empiezas a arrastrar.")),
                (Some(kind), Some(style)) => {
                    let options = kind.options();
                    let color_item = |key: &str, label: &str, value: Rgba| OptionItem { kind: "color".into(), key: key.into(), label: label.into(), color: color(value), ..Default::default() };
                    if options & option::TEXT != 0 {
                        items.push(color_item("text", "Texto", style.text));
                    }
                    if options & option::STROKE != 0 {
                        items.push(color_item("stroke", kind.stroke_label(), style.stroke));
                    }
                    if options & option::FILL != 0 {
                        items.push(color_item("fill", kind.fill_label(), style.fill));
                    }
                    let mut choice = |key: &str, label: &str, values: &[f32], current: f32, title: &dyn Fn(f32) -> String| {
                        let mut values = values.to_vec();
                        if !values.contains(&current) {
                            values.push(current);
                            values.sort_by(f32::total_cmp);
                        }
                        let titles: Vec<SharedString> = values.iter().map(|value| title(*value).into()).collect();
                        let selected = values.iter().position(|value| *value == current).unwrap_or(0) as i32;
                        items.push(OptionItem {
                            kind: "choice".into(),
                            key: key.into(),
                            label: label.into(),
                            choices: ModelRc::new(VecModel::from(titles)),
                            selected,
                            ..Default::default()
                        });
                        choices.push((key.to_string(), values));
                    };
                    let pixels = |value: f32| format!("{} px", model::number(value));
                    if options & option::LINE_WIDTH != 0 {
                        choice("lineWidth", "Grosor", &LINE_WIDTHS, style.line_width, &pixels);
                    }
                    if options & option::CORNER_RADIUS != 0 {
                        choice("cornerRadius", "Esquinas", &RADII, style.corner_radius, &pixels);
                    }
                    if options & option::FONT_SIZE != 0 {
                        choice("fontSize", "Tamaño", &FONT_SIZES, style.font_size, &|value| format!("{} pt", model::number(value)));
                    }
                    if options & option::STRENGTH != 0 {
                        choice("strength", kind.strength_label(), kind.strength_choices(), style.strength, &|value| kind.strength_title(value));
                    }
                    if options & option::SHADOW != 0 {
                        items.push(OptionItem { kind: "shadow".into(), key: "shadow".into(), label: "Sombra".into(), checked: style.shadow, ..Default::default() });
                    }
                }
                _ => items.push(hint("Haz clic en una forma para seleccionarla y arrástrala para moverla.")),
            },
        }
        self.choices = choices;
        self.window.set_options(ModelRc::new(VecModel::from(items)));
    }

    fn change_style(&mut self, key: &str, change: impl FnOnce(&mut ShapeStyle) + Copy) {
        let font = &self.renderer.font;
        let Some(Tab::Capture(tab)) = self.tabs.get_mut(self.current) else { return };
        let Some(selected) = tab.selected_shape().map(|shape| (shape.id, shape.kind)) else {
            if let Some(kind) = tab.tool.kind() {
                let mut style = self.styles.style(kind);
                change(&mut style);
                self.styles.save(kind, style);
            }
            self.refresh_options();
            self.refresh_groups();
            return;
        };
        let Some(before) = tab.style_changed(font, change) else { return };
        let now = Instant::now();
        let coalesced = self.coalescing.as_ref().is_some_and(|(k, id, at)| k == key && *id == selected.0 && now.duration_since(*at) < Duration::from_millis(1500));
        if coalesced {
            tab.doc.touch();
        } else {
            tab.doc.register(before, "Cambiar estilo");
        }
        self.coalescing = Some((key.to_string(), selected.0, now));
        if let Some(style) = tab.selected_shape().map(|shape| shape.style) {
            self.styles.save(selected.1, style);
        }
        self.document_changed();
        self.refresh_options();
        self.refresh_groups();
        self.request_redraw();
    }

    fn option_color(&mut self, key: &str, value: slint::Color) {
        let value = rgba(value);
        match key {
            "text" => self.change_style("text", move |style| style.text = value),
            "stroke" => self.change_style("stroke", move |style| style.stroke = value),
            "fill" => self.change_style("fill", move |style| style.fill = value),
            _ => {}
        }
    }

    fn option_choice(&mut self, key: &str, index: usize) {
        let Some(value) = self.choices.iter().find(|(k, _)| k == key).and_then(|(_, values)| values.get(index).copied()) else { return };
        match key {
            "lineWidth" => self.change_style("lineWidth", move |style| style.line_width = value),
            "cornerRadius" => self.change_style("cornerRadius", move |style| style.corner_radius = value),
            "fontSize" => self.change_style("fontSize", move |style| style.font_size = value),
            "strength" => self.change_style("strength", move |style| style.strength = value),
            _ => {}
        }
    }

    fn option_check(&mut self, key: &str, value: bool) {
        if key == "shadow" {
            self.change_style("shadow", move |style| style.shadow = value);
        }
    }

    fn option_button(&mut self, key: &str) {
        let Some(tab) = self.capture() else { return };
        let changes = match key {
            "apply-crop" => tab.apply_crop_now(),
            "cancel-crop" => tab.cancel_crop_now(),
            _ => return,
        };
        self.apply(changes);
    }

    fn pointer(&mut self, kind: &str, button: &str, x: f32, y: f32, shift: bool) {
        if let Some(key) = self.picking.clone() {
            if kind == "down" {
                self.picking = None;
                self.window.set_picking(false);
                if button == "left" {
                    self.pick_at(&key, x, y);
                }
            }
            return;
        }
        let font = &self.renderer.font;
        let styles = &self.styles;
        let Some(Tab::Capture(tab)) = self.tabs.get_mut(self.current) else { return };
        let canvas = tab.doc.canvas();
        let point = tab.view.to_canvas(canvas, x, y);
        let changes = match kind {
            "down" => tab.press(point, (x, y), button, shift, styles, font),
            "move" => tab.motion(point, (x, y), shift),
            "up" => tab.release(point),
            "double" => tab.double_click(point, font),
            _ => return,
        };
        self.apply(changes);
    }

    fn scrolled(&mut self, dx: f32, dy: f32, x: f32, y: f32, ctrl: bool) {
        let Some(tab) = self.capture() else { return };
        tab.auto_fit = false;
        let canvas = tab.doc.canvas();
        if ctrl {
            let factor = (dy / 240.0).exp();
            let zoom = tab.view.zoom * factor;
            tab.view.zoom_at(canvas, zoom, (x, y));
        } else {
            tab.view.scroll.0 -= dx;
            tab.view.scroll.1 -= dy;
            tab.view.clamp(canvas);
        }
        self.request_redraw();
    }

    fn zoom(&mut self, factor: Option<f32>) {
        let Some(tab) = self.capture() else { return };
        tab.auto_fit = factor.is_none();
        let canvas = tab.doc.canvas();
        let center = (tab.view.size.0 / 2.0, tab.view.size.1 / 2.0);
        match factor {
            Some(factor) => {
                let zoom = tab.view.zoom * factor;
                tab.view.zoom_at(canvas, zoom, center);
            }
            None => tab.view.fit(canvas),
        }
        self.request_redraw();
    }

    fn actual_size(&mut self) {
        let Some(tab) = self.capture() else { return };
        tab.auto_fit = false;
        let canvas = tab.doc.canvas();
        let center = (tab.view.size.0 / 2.0, tab.view.size.1 / 2.0);
        tab.view.zoom_at(canvas, 1.0, center);
        self.request_redraw();
    }

    pub fn key(&mut self, text: &str, ctrl: bool, shift: bool, alt: bool) -> bool {
        let lower = text.to_lowercase();
        if ctrl {
            match lower.as_str() {
                "s" if shift => self.action("save-as"),
                "s" => self.action("save"),
                "w" => self.close_tab(self.current, false),
                "\u{8}" => self.action("discard"),
                "z" if shift => self.undo(false),
                "z" => self.undo(true),
                "y" => self.undo(false),
                "c" => self.action("copy"),
                "v" => self.paste(),
                "+" | "=" => self.zoom(Some(1.25)),
                "-" => self.zoom(Some(0.8)),
                "0" => self.actual_size(),
                "9" => self.zoom(None),
                "\t" => {
                    if !self.tabs.is_empty() {
                        let next = if shift { (self.current + self.tabs.len() - 1) % self.tabs.len() } else { (self.current + 1) % self.tabs.len() };
                        self.switch_tab(next);
                    }
                }
                _ => return false,
            }
            return true;
        }
        if text == "\u{1b}" && self.picking.take().is_some() {
            self.window.set_picking(false);
            return true;
        }
        let Some(Tab::Capture(tab)) = self.tabs.get_mut(self.current) else { return false };
        if tab.editing.is_some() {
            return false;
        }
        let step = if shift { 10.0 } else { 1.0 };
        let changes = match text {
            "\u{1b}" => tab.escape(),
            "\n" | "\r" => tab.enter(),
            "\u{7f}" | "\u{8}" => tab.delete_selection(),
            "\u{F702}" => tab.nudge(-step, 0.0),
            "\u{F703}" => tab.nudge(step, 0.0),
            "\u{F700}" => tab.nudge(0.0, -step),
            "\u{F701}" => tab.nudge(0.0, step),
            _ => {
                let Some(tool) = (!alt && !shift).then(|| lower.chars().next()).flatten().filter(|_| lower.chars().count() == 1).and_then(Tool::matching) else {
                    return false;
                };
                self.set_tool(tool);
                return true;
            }
        };
        self.apply(changes);
        true
    }

    fn undo(&mut self, backwards: bool) {
        let font = &self.renderer.font;
        let Some(Tab::Capture(tab)) = self.tabs.get_mut(self.current) else { return };
        let mut changes = Changes::default();
        tab.end_editing(false, font, &mut changes);
        let done = if backwards { tab.doc.undo() } else { tab.doc.redo() };
        if done {
            tab.restore_finished();
            self.coalescing = None;
            self.document_changed();
            self.refresh_options();
            self.request_redraw();
        }
    }

    fn after_control(&mut self, body: impl FnOnce(&mut Controller)) {
        body(self);
        slint::Timer::single_shot(Duration::ZERO, || with(|c| c.window.invoke_restore_focus()));
    }

    fn text_history(&mut self, redo: bool) -> bool {
        let Some(tab) = self.capture() else { return false };
        if !tab.text_history_belongs_to_document(redo) {
            return false;
        }
        self.undo(!redo);
        true
    }

    fn caret_moved(&mut self, cursor: i32, anchor: i32) {
        let Some(tab) = self.capture() else { return };
        if tab.editing.is_none() {
            return;
        }
        tab.caret = Some((cursor.max(0) as usize, anchor.max(0) as usize));
        self.request_redraw();
    }

    fn text_changed(&mut self, text: &str) {
        let font = &self.renderer.font;
        let Some(Tab::Capture(tab)) = self.tabs.get_mut(self.current) else { return };
        tab.text_changed(text, font);
        self.request_redraw();
    }

    fn finish_text(&mut self) {
        let font = &self.renderer.font;
        let Some(Tab::Capture(tab)) = self.tabs.get_mut(self.current) else { return };
        if tab.editing.is_some() {
            let changes = tab.finish_editing(font);
            self.apply(changes);
        }
    }

    fn text_finished(&mut self) {
        self.finish_text();
    }

    fn context_action(&mut self, name: &str) {
        let Some(tab) = self.capture() else { return };
        let changes = match name {
            "duplicate" => tab.duplicate_selection(),
            "front" => tab.reorder(true),
            "back" => tab.reorder(false),
            "delete" => tab.delete_selection(),
            _ => return,
        };
        self.apply(changes);
    }

    fn rendered(&mut self) -> Option<(u64, Arc<RgbImage>, f32)> {
        self.finish_text();
        let Some(Tab::Capture(tab)) = self.tabs.get_mut(self.current) else { return None };
        let image = tab.doc.rendered(&mut self.renderer);
        Some((tab.doc.id, image, tab.doc.scale))
    }

    fn send_png(&mut self, id: u64, copy: bool) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id() == id) else { return };
        let Tab::Capture(tab) = &mut self.tabs[index] else { return };
        let image = tab.doc.rendered(&mut self.renderer);
        let scale = tab.doc.scale;
        let link = self.link.clone();
        std::thread::spawn(move || {
            if let Ok(png) = image::encode_png(&image, f64::from(scale), png::Compression::Fast) {
                let message = if copy { ToDaemon::Copy { id } } else { ToDaemon::Changed { id } };
                link.send(&message, &png);
            }
        });
    }

    fn toast(&self, title: &str, detail: Option<&str>) {
        self.link.send(&ToDaemon::Toast { title: title.into(), detail: detail.map(str::to_string) }, &[]);
    }

    pub fn notify(&self, title: &str, detail: &str) {
        self.toast(title, Some(detail));
    }

    pub fn schedule_frame(&mut self, id: u64, path: PathBuf, window: slint::Weak<EditorWindow>, delay: u64) {
        let Some(Tab::Media(media)) = self.tabs.get(self.current) else { return };
        if media.id != id {
            return;
        }
        media.start_timer(delay, move || crate::media::advance_frame(id, path, window));
    }

    pub fn media_saved_as(&mut self, id: u64, path: PathBuf) {
        let Some(Tab::Media(media)) = self.tabs.iter_mut().find(|tab| tab.id() == id) else { return };
        if let Some(error) = media.saved_as(path) {
            self.toast("No se pudo guardar", Some(&error));
        }
        self.refresh_tabs();
    }

    pub fn action(&mut self, name: &str) {
        if let Some(media) = self.media() {
            let result = media.action(name);
            if let Some(message) = result.message {
                self.link.send(&message, &[]);
            }
            if result.close {
                self.close_tab(self.current, true);
            } else {
                self.refresh_tabs();
            }
            return;
        }
        match name {
            "copy" => {
                if let Some((id, _, _)) = self.rendered() {
                    self.send_png(id, true);
                    self.clipboard_owner = Some(id);
                }
            }
            "save" => self.save(None),
            "save-as" => {
                let Some(Tab::Capture(tab)) = self.tabs.get(self.current) else { return };
                let id = tab.doc.id;
                let directory = Settings::load().save_directory();
                let name = image::unique_path(&directory, tab.doc.created, "png").file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                std::thread::spawn(move || {
                    let Some(connection) = zbus::blocking::Connection::session().ok() else { return };
                    let chosen = portal::save_file(&connection, "Guardar como", &directory, &name, &[("Imagen PNG", "*.png"), ("Imagen JPEG", "*.jpg")]);
                    if let Some(path) = chosen {
                        let _ = slint::invoke_from_event_loop(move || {
                            with(move |c| {
                                if let Some(index) = c.tabs.iter().position(|tab| tab.id() == id) {
                                    c.current = index;
                                    c.save(Some(path));
                                }
                            })
                        });
                    }
                });
            }
            "pin" => {
                if let Some((_, image, scale)) = self.rendered() {
                    self.link.send(&ToDaemon::Pin { width: image.width, height: image.height, scale: f64::from(scale) }, &image.data);
                }
            }
            "ocr" => {
                if let Some((_, image, scale)) = self.rendered() {
                    crate::ocr::recognize(image, scale);
                }
            }
            "ocr-area" => {
                if let Some(tab) = self.capture() {
                    let changes = tab.select_text_area();
                    self.apply(changes);
                    self.window.invoke_focus_canvas();
                }
            }
            "discard" => self.discard(),
            _ => {}
        }
    }

    fn save(&mut self, path: Option<PathBuf>) {
        let directory = Settings::load().save_directory();
        let Some((_, image, scale)) = self.rendered() else { return };
        let Some(Tab::Capture(tab)) = self.tabs.get_mut(self.current) else { return };
        let path = path.or_else(|| tab.doc.saved.clone()).unwrap_or_else(|| image::unique_path(&directory, tab.doc.created, "png"));
        tab.doc.mark_saved(path.clone(), false);
        write_in_background(image, scale, path, self.link.clone());
        self.refresh_tabs();
    }

    fn discard(&mut self) {
        let font = &self.renderer.font;
        let Some(Tab::Capture(tab)) = self.tabs.get_mut(self.current) else { return };
        let mut changes = Changes::default();
        tab.end_editing(false, font, &mut changes);
        let id = tab.doc.id;
        self.link.send(&ToDaemon::Discard { id }, &[]);
        if let (true, Some(path)) = (tab.doc.auto_saved, tab.doc.saved.clone()) {
            std::thread::spawn(move || {
                if let Ok(connection) = zbus::blocking::Connection::session() {
                    portal::trash(&connection, &path);
                }
                history::remove(&[path]);
            });
        }
        self.close_tab(self.current, true);
    }

    fn close_tab(&mut self, index: usize, discarded: bool) {
        if index >= self.tabs.len() {
            return;
        }
        if index == self.current {
            self.finish_text();
        }
        let tab = self.tabs.remove(index);
        match tab {
            Tab::Capture(mut tab) => {
                if !discarded {
                    if let (true, true, Some(path)) = (tab.doc.auto_saved, tab.doc.has_unsaved_changes(), tab.doc.saved.clone()) {
                        let image = tab.doc.rendered(&mut self.renderer);
                        write_in_background(image, tab.doc.scale, path, self.link.clone());
                    }
                    self.link.send(&ToDaemon::Release { id: tab.doc.id }, &[]);
                }
            }
            Tab::Media(mut media) => {
                media.hide();
                if !discarded {
                    self.link.send(&ToDaemon::Release { id: media.id }, &[]);
                }
            }
        }
        self.scene = None;
        if self.tabs.is_empty() {
            self.current = 0;
            self.detach_drag();
            let _ = self.window.hide();
            self.window.set_canvas(Image::default());
            self.window.set_media_image(Image::default());
            trim_memory();
            self.maybe_exit();
            return;
        }
        if self.current >= self.tabs.len() || index < self.current {
            self.current = self.current.saturating_sub(1).min(self.tabs.len() - 1);
        }
        self.refresh_tabs();
        self.refresh_tab_view();
    }

    fn close_all(&mut self) {
        while !self.tabs.is_empty() {
            let last = self.tabs.len() - 1;
            self.current = last;
            self.close_tab(last, false);
        }
    }

    pub fn text_window_closed(&mut self) {
        self.text_windows.retain(|window| window.window().is_visible());
        self.maybe_exit();
    }

    pub fn show_text(&mut self, text: String) {
        let window = match TextWindow::new() {
            Ok(window) => window,
            Err(_) => return,
        };
        window.set_text((if text.is_empty() { "No se encontró texto.".to_string() } else { text }).into());
        let weak = window.as_weak();
        window.on_copy_range(move |anchor, cursor| {
            let Some(window) = weak.upgrade() else { return };
            let text = window.get_text().to_string();
            let copied = selected_text(&text, anchor, cursor).to_string();
            with(move |c| c.link.send(&ToDaemon::CopyText { text: copied }, &[]));
            let _ = window.hide();
            with(|c| c.text_window_closed());
        });
        let weak = window.as_weak();
        window.on_dismiss(move || {
            if let Some(window) = weak.upgrade() {
                let _ = window.hide();
            }
            with(|c| c.text_window_closed());
        });
        window.window().on_close_requested(|| {
            slint::Timer::single_shot(Duration::ZERO, || with(|c| c.text_window_closed()));
            slint::CloseRequestResponse::HideWindow
        });
        let _ = window.show();
        self.text_windows.push(window);
    }

    fn maybe_exit(&mut self) {
        if self.tabs.is_empty() && self.history.is_none() && self.text_windows.iter().all(|window| !window.window().is_visible()) {
            self.exiting = true;
            self.link.send(&ToDaemon::Exiting { received: self.received }, &[]);
        }
    }

    fn local_id(&mut self) -> u64 {
        self.local_ids += 1;
        self.local_ids
    }

    fn show_history(&mut self) {
        if self.history.is_none() {
            self.history = HistoryView::new();
        }
        let Some(history) = self.history.as_mut() else {
            self.toast("No se pudo abrir el historial", None);
            return;
        };
        self.exiting = false;
        history.show();
        self.link.send(&ToDaemon::Activate { history: true }, &[]);
    }

    pub fn history_input(&mut self, body: impl FnOnce(&mut HistoryView) -> Option<Command>) {
        let Some(history) = self.history.as_mut() else { return };
        if let Some(command) = body(history) {
            self.run_history(command);
        }
    }

    pub fn history_key(&mut self, text: &str, ctrl: bool) -> bool {
        let Some(history) = self.history.as_mut() else { return false };
        let (handled, command) = history.key(text, ctrl);
        if let Some(command) = command {
            self.run_history(command);
        }
        handled
    }

    pub fn history_thumbnail(&mut self, path: PathBuf, made: Thumbnail) {
        if let Some(history) = self.history.as_mut() {
            history.thumbnail_ready(path, made);
        }
    }

    pub fn history_watch(&mut self) {
        if let Some(history) = self.history.as_mut().filter(|history| history.is_visible())
            && history.changed_on_disk() {
                history.reload();
            }
    }

    pub fn history_reload(&mut self) {
        if let Some(history) = self.history.as_mut() {
            history.reload();
        }
    }

    pub fn history_closed(&mut self) {
        self.history = None;
        trim_memory();
        self.maybe_exit();
    }

    fn run_history(&mut self, command: Command) {
        match command {
            Command::Open(entries) => {
                for entry in entries {
                    let id = self.local_id();
                    let created = entry.seconds();
                    std::thread::spawn(move || match entry.media() {
                        Some(kind) => {
                            let gif = kind == history::MediaKind::Gif;
                            let size = crate::media::pixel_size(&entry.path, gif);
                            let _ = slint::invoke_from_event_loop(move || {
                                with(move |c| {
                                    let mut tab = MediaTab::new(id, entry.path, gif, size, created, false);
                                    tab.is_saved = true;
                                    c.open(Tab::Media(Box::new(tab)));
                                })
                            });
                        }
                        None => {
                            let Some(decoded) = image::read(&entry.path) else { return };
                            let base = RgbImage::from_rgba_over(decoded.width, decoded.height, &decoded.rgba, [255, 255, 255]);
                            let _ = slint::invoke_from_event_loop(move || {
                                with(move |c| {
                                    let doc = Document::new(id, base, decoded.scale.max(0.1) as f32, created, Some(entry.path));
                                    let tab = CaptureTab::new(doc, c.tool);
                                    c.open(Tab::Capture(Box::new(tab)));
                                })
                            });
                        }
                    });
                }
            }
            Command::Copy(entries) => match entries.as_slice() {
                [single] if single.media().is_none() => {
                    let (id, link, path) = (self.local_id(), self.link.clone(), single.path.clone());
                    std::thread::spawn(move || {
                        let Ok(bytes) = std::fs::read(&path) else { return };
                        let png = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
                            Some(bytes)
                        } else {
                            image::decode(&bytes).and_then(|decoded| {
                                let rgb = RgbImage::from_rgba_over(decoded.width, decoded.height, &decoded.rgba, [255, 255, 255]);
                                image::encode_png(&rgb, decoded.scale, png::Compression::Fast).ok()
                            })
                        };
                        if let Some(png) = png {
                            link.send(&ToDaemon::Copy { id }, &png);
                        }
                    });
                }
                [single] => {
                    let id = self.local_id();
                    let gif = single.media() == Some(history::MediaKind::Gif);
                    self.link.send(&ToDaemon::CopyFile { id, path: single.path.clone(), gif }, &[]);
                }
                many => self.link.send(&ToDaemon::CopyFiles { paths: many.iter().map(|entry| entry.path.clone()).collect() }, &[]),
            },
            Command::Pin(entries) => {
                let link = self.link.clone();
                std::thread::spawn(move || {
                    for entry in entries {
                        let Some(decoded) = image::read(&entry.path) else { continue };
                        let rgb = RgbImage::from_rgba_over(decoded.width, decoded.height, &decoded.rgba, [255, 255, 255]);
                        link.send(&ToDaemon::Pin { width: rgb.width, height: rgb.height, scale: decoded.scale }, &rgb.data);
                    }
                });
            }
        }
    }

    fn paste(&mut self) {
        if self.capture().is_some() {
            self.pending_paste = true;
            self.link.send(&ToDaemon::PasteRequest, &[]);
        }
    }

    fn paste_image(&mut self, payload: &[u8], scale: f64) {
        if !std::mem::replace(&mut self.pending_paste, false) {
            return;
        }
        let Some(decoded) = image::decode(payload) else { return };
        let Some(mut pixmap) = tiny_skia::Pixmap::new(decoded.width, decoded.height) else { return };
        for (target, source) in pixmap.data_mut().as_chunks_mut::<4>().0.iter_mut().zip(decoded.rgba.as_chunks::<4>().0) {
            let alpha = u16::from(source[3]);
            for c in 0..3 {
                target[c] = ((u16::from(source[c]) * alpha + 127) / 255) as u8;
            }
            target[3] = source[3];
        }
        let image = RgbaImage { width: decoded.width, height: decoded.height, pixmap };
        let styles = &self.styles;
        let Some(Tab::Capture(tab)) = self.tabs.get_mut(self.current) else { return };
        let changes = tab.insert_image(image, decoded.scale.max(scale) as f32, styles);
        self.apply(changes);
    }
}

fn write_in_background(image: Arc<RgbImage>, scale: f32, path: PathBuf, link: Link) {
    std::thread::spawn(move || match image::write(&image, f64::from(scale), &path) {
        Ok(()) => history::record(&path),
        Err(error) => link.send(&ToDaemon::Toast { title: "No se pudo guardar".into(), detail: Some(error.to_string()) }, &[]),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_icons_take_the_configured_colors() {
        let mut style = ShapeKind::Rectangle.default_style();
        style.stroke = Rgba::from_rgb8([0, 132, 255]);
        style.fill = Rgba::from_rgb8([48, 209, 88]);
        assert_eq!(icon_colors(Tool::Rectangle, Some(style)), (style.stroke, style.fill));
        let step = ShapeKind::Step.default_style();
        assert_eq!(icon_colors(Tool::Step, Some(step)), (step.text, step.fill));
    }

    #[test]
    fn tools_without_a_color_of_their_own_take_the_color_of_their_purpose() {
        assert_eq!(icon_colors(Tool::Blur, Some(ShapeKind::Blur.default_style())), (CONCEAL, Rgba::CLEAR));
        assert_eq!(icon_colors(Tool::Crop, None), (TRANSFORM, Rgba::CLEAR));
        assert_eq!(icon_colors(Tool::Select, None), (POINTER, Rgba::CLEAR));
    }

    #[test]
    fn the_editor_window_fits_the_capture_within_the_screen() {
        assert_eq!(fitted_window((300.0, 200.0), Some((1920.0, 1080.0)), 1250.0), (1250.0, 480.0));
        assert_eq!(fitted_window((1400.0, 600.0), Some((1920.0, 1080.0)), 1250.0), (1480.0, 800.0));
        assert_eq!(fitted_window((3000.0, 2000.0), Some((1920.0, 1080.0)), 1250.0), (1632.0, 918.0));
        assert_eq!(fitted_window((300.0, 200.0), Some((1280.0, 720.0)), 1250.0), (1088.0, 480.0));
    }

    #[test]
    fn copying_recognized_text_takes_the_selection_or_everything() {
        assert_eq!(selected_text("año uno", 5, 0), "año ");
        assert_eq!(selected_text("año uno", 3, 3), "año uno");
    }

    #[test]
    fn a_dark_icon_color_falls_back_to_a_visible_gray() {
        let mut style = ShapeKind::Arrow.default_style();
        style.stroke = Rgba::BLACK;
        assert_eq!(icon_colors(Tool::Arrow, Some(style)).0, DIM);
    }

    #[test]
    fn outlined_dark_text_shows_its_outline_color_on_the_icon() {
        let mut style = ShapeKind::TextOutline.default_style();
        style.stroke = Rgba::YELLOW;
        assert_eq!(icon_colors(Tool::TextOutline, Some(style)).0, Rgba::YELLOW);
        style.line_width = 0.0;
        assert_eq!(icon_colors(Tool::TextOutline, Some(style)).0, NEUTRAL);
    }

    #[test]
    fn narrower_windows_fold_more_tool_groups() {
        let limits = thresholds();
        assert!(limits.windows(2).all(|pair| pair[0] > pair[1]));
    }
}
