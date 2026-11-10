use crate::chrome::{Chrome, ChromeAction};
use crate::clipboard::{self, Bytes, Clipboard, Payload};
use crate::dbus;
use crate::screenshot::{self, Shot, Target};
use crate::scroll::{self, Scroller, Scrolling};
use crate::shortcuts;
use crate::editor::{self, EditorLink};
use crate::memory;
use crate::overlay::{FrozenScreen, Mode, Overlay, Request, Selection};
use crate::pin::{PinRequest, Pins, Placement};
use crate::record::{self, Format, Recording, StreamData};
use sharek_common::ipc::{ToDaemon, ToEditor};
use crate::toast::{self, Toast};
use crate::tray::Tray;
use crate::windows::{self, Rect};
use crate::wl::{self, Globals, WmBase};
use sharek_common::image::{self, RgbImage};
use sharek_common::settings::Settings;
use sharek_common::{history, ocr, paths, time};
use smithay_client_toolkit::reexports::calloop::channel::{self, Sender};
use smithay_client_toolkit::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay_client_toolkit::reexports::calloop::{EventLoop, LoopHandle, LoopSignal};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::compositor::CompositorHandler;
use smithay_client_toolkit::seat::keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers};
use smithay_client_toolkit::seat::pointer::{PointerEvent, PointerEventKind, PointerHandler};
use smithay_client_toolkit::shell::wlr_layer::{LayerShellHandler, LayerSurface, LayerSurfaceConfigure};
use smithay_client_toolkit::shell::xdg::popup::{Popup, PopupConfigure, PopupHandler};
use smithay_client_toolkit::shm::raw::RawPool;
use smithay_client_toolkit::shm::{Shm, ShmHandler};
use smithay_client_toolkit::{
    delegate_registry,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{Capability, SeatHandler, SeatState},
};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use wayland_client::globals::registry_queue_init;
use wayland_client::protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_surface};
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::{Shape, WpCursorShapeDeviceV1};
use wayland_client::{Connection, QueueHandle};
use wayland_protocols_plasma::screencast::v1::client::zkde_screencast_stream_unstable_v1::{self, ZkdeScreencastStreamUnstableV1};


type OutputChoice = (wl_output::WlOutput, String, Rect, (u32, u32), f64);
const TICK: Duration = Duration::from_millis(500);
const STOP_TIMEOUT: Duration = Duration::from_secs(15);
const RELEASE_DELAY: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toggle {
    CopyOnCapture,
    AutoSave,
    OpenEditor,
    ShowsCursor,
    RecordsAudio,
    LaunchAtLogin,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    CaptureRegion,
    CaptureScreen,
    CaptureWindow,
    ScrollingCapture,
    RecognizeText,
    PinRegion,
    PinClipboard,
    ClosePins,
    RecordVideo,
    RecordGif,
    PickColor,
    Measure,
    ShowHistory,
    StopRecording,
    Toggle(Toggle),
    ChooseFolder,
    OpenFolder,
    Quit,
}

impl Action {
    pub fn named(name: &str) -> Option<Action> {
        Some(match name {
            "region" => Action::CaptureRegion,
            "screen" => Action::CaptureScreen,
            "window" => Action::CaptureWindow,
            "scroll" => Action::ScrollingCapture,
            "ocr" => Action::RecognizeText,
            "pin" => Action::PinRegion,
            "pin-clipboard" => Action::PinClipboard,
            "close-pins" => Action::ClosePins,
            "video" => Action::RecordVideo,
            "gif" => Action::RecordGif,
            "color" => Action::PickColor,
            "ruler" => Action::Measure,
            "history" => Action::ShowHistory,
            "stop" => Action::StopRecording,
            "quit" => Action::Quit,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    Capture,
    Pin,
    Recognize,
    Scroll,
    Video,
    Gif,
    Measure,
    Pick,
}

pub enum Event {
    Action(Action, bool),
    Shot(Result<Shot, String>),
    Encoded { id: u64, png: Vec<u8>, save: Option<PathBuf> },
    CopyPng(u64, Vec<u8>),
    Recognized(Result<String, String>),
    ClipboardSnapshot(Vec<(String, Bytes)>),
    ClipboardImage(Vec<u8>),
    FolderChosen(PathBuf),
    Failure(String, String),
    RecordingEnded(u64, Result<(), String>),
    ScrollFinished(u64, scroll::Finished),
    ScrollStatus(u64, scroll::Status),
    ShortcutsChanged,
    Editor(ToDaemon, Vec<u8>),
    EditorGone,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ImageRequest {
    Paste,
    Pin,
}

pub struct App {
    pub connection: Connection,
    pub qh: QueueHandle<App>,
    pub registry: RegistryState,
    pub outputs: OutputState,
    pub seats: SeatState,
    pub clipboard: Clipboard,
    pub dbus: zbus::blocking::Connection,
    pub settings: Settings,
    pub sender: Sender<Event>,
    pub handle: LoopHandle<'static, App>,
    pub signal: LoopSignal,
    pub globals: Globals,
    overlay: Option<(Overlay, Purpose)>,
    pub pins: Pins,
    xdg: Option<WmBase>,
    seat: Option<wl_seat::WlSeat>,
    pointer: Option<wl_pointer::WlPointer>,
    cursor_shape: Option<WpCursorShapeDeviceV1>,
    pinch: Option<wayland_protocols::wp::pointer_gestures::zv1::client::zwp_pointer_gesture_pinch_v1::ZwpPointerGesturePinchV1>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    keyboard_focus: Option<wl_surface::WlSurface>,
    ctrl: bool,
    alt: bool,
    toast: Option<Toast>,
    toasts: u64,
    editor: Option<EditorLink>,
    announced_owner: Option<Option<u64>>,
    image_request: Option<ImageRequest>,
    tray: ksni::Handle<Tray>,
    recording: Option<Recording>,
    scrolling: Option<Scrolling>,
    release_pending: bool,
    busy: bool,
    next_id: u64,
}

pub fn run(dbus: zbus::blocking::Connection, registered: Result<(), String>, initial: Option<Action>) {
    let connection = Connection::connect_to_env().expect("Wayland connection");
    let (globals, queue) = registry_queue_init::<App>(&connection).expect("Wayland registry");
    let qh = queue.handle();
    let mut event_loop: EventLoop<'static, App> = EventLoop::try_new().expect("event loop");
    let handle = event_loop.handle();
    WaylandSource::new(connection.clone(), queue).insert(handle.clone()).expect("Wayland source");

    let (sender, receiver) = channel::channel::<Event>();
    handle
        .insert_source(receiver, |event, _, app: &mut App| {
            if let channel::Event::Msg(event) = event {
                app.handle_event(event);
            }
        })
        .expect("event channel");

    let mut settings = Settings::load();
    if !settings.login_item_configured && is_installed() {
        settings.login_item_configured = true;
        set_launch_at_login(true);
        settings.save();
    }
    let _ = std::fs::remove_dir_all(paths::recordings_dir());

    let tray = Tray::new(sender.clone(), settings.clone(), launch_at_login());
    let tray = futures_lite::future::block_on(ksni::TrayMethods::assume_sni_available(tray, true).spawn()).expect("status notifier item");
    match registered {
        Ok(()) => shortcuts::spawn(dbus.clone(), sender.clone()),
        Err(error) => dbus::notify(&dbus, "Atajos globales no registrados", &format!("Instala ShareK con scripts/install.sh para que el portal lo identifique ({error})")),
    }
    crate::service::serve(&dbus, sender.clone());

    let manager = globals.bind(&qh, 1..=1, clipboard::ManagerData).ok();
    let wayland_globals = Globals::bind(&globals, &qh).expect("required Wayland globals");
    let xdg = WmBase::bind(&globals, &qh);
    let mut app = App {
        registry: RegistryState::new(&globals),
        outputs: OutputState::new(&globals, &qh),
        seats: SeatState::new(&globals, &qh),
        clipboard: Clipboard::new(manager),
        connection,
        qh,
        dbus,
        settings,
        sender: sender.clone(),
        handle,
        signal: event_loop.get_signal(),
        globals: wayland_globals,
        overlay: None,
        pins: Pins::new(),
        xdg,
        seat: None,
        pointer: None,
        cursor_shape: None,
        pinch: None,
        keyboard: None,
        keyboard_focus: None,
        ctrl: false,
        alt: false,
        toast: None,
        toasts: 0,
        editor: None,
        announced_owner: None,
        image_request: None,
        tray,
        recording: None,
        scrolling: None,
        release_pending: true,
        busy: false,
        next_id: 0,
    };
    if let Some(action) = initial {
        let _ = sender.send(Event::Action(action, false));
    }
    let _ = app.handle.insert_source(Timer::from_duration(RELEASE_DELAY), |_, _, app: &mut App| {
        app.release_pending = false;
        memory::release();
        TimeoutAction::Drop
    });
    event_loop.run(None, &mut app, |_| {}).expect("event loop run");
}

fn is_installed() -> bool {
    let bin = std::env::var_os("XDG_BIN_HOME").map(PathBuf::from).unwrap_or_else(|| paths::home().join(".local/bin"));
    std::env::current_exe().ok().and_then(|exe| exe.parent().map(|parent| parent == bin)).unwrap_or(false)
}

pub fn launch_at_login() -> bool {
    paths::autostart_file().exists()
}

pub fn set_launch_at_login(enabled: bool) {
    let file = paths::autostart_file();
    if enabled {
        let executable = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("sharek"));
        let entry = format!(
            "[Desktop Entry]\nType=Application\nName=ShareK\nExec={}\nIcon={}\nX-GNOME-Autostart-enabled=true\nNoDisplay=true\n",
            executable.display(),
            paths::APP_ID
        );
        if let Some(parent) = file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(file, entry);
    } else {
        let _ = std::fs::remove_file(file);
    }
}

impl App {
    fn update_tray(&self, change: impl FnOnce(&mut Tray) + Send) {
        futures_lite::future::block_on(self.tray.update(change));
    }

    pub fn fail(&self, summary: &str, detail: &str) {
        dbus::notify(&self.dbus, summary, detail);
    }

    fn next_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn schedule_release(&mut self) {
        if std::mem::replace(&mut self.release_pending, true) {
            return;
        }
        let _ = self.handle.insert_source(Timer::from_duration(RELEASE_DELAY), |_, _, app: &mut App| {
            app.release_pending = false;
            memory::release();
            TimeoutAction::Drop
        });
    }

    fn handle_event(&mut self, event: Event) {
        self.schedule_release();
        match event {
            Event::Action(action, from_menu) => self.dispatch(action, from_menu),
            Event::Shot(result) => {
                self.busy = false;
                match result {
                    Ok(shot) => self.deliver(shot.image, shot.scale),
                    Err(error) => self.fail("No se pudo capturar", &error),
                }
            }
            Event::Encoded { id, png, save } => {
                let png = Arc::new(png);
                if self.settings.copy_on_capture {
                    self.clipboard.copy(id, Payload::Png(png.clone()), &self.connection, &self.qh, &self.sender);
                    self.sync_clipboard_owner();
                }
                if let Some(path) = save {
                    let sender = self.sender.clone();
                    std::thread::spawn(move || match image::write_bytes(&png, &path) {
                        Ok(()) => history::record(&path),
                        Err(error) => {
                            let _ = sender.send(Event::Failure("No se pudo guardar".into(), error.to_string()));
                        }
                    });
                }
            }
            Event::CopyPng(id, png) => {
                self.clipboard.copy(id, Payload::Png(Arc::new(png)), &self.connection, &self.qh, &self.sender);
                self.sync_clipboard_owner();
            }
            Event::Recognized(result) => match result {
                Ok(text) if text.trim().is_empty() => self.show_toast("No se encontró texto", None),
                Ok(text) => {
                    self.clipboard.set_text(text.clone(), &self.qh);
                    self.sync_clipboard_owner();
                    self.show_toast("Texto copiado", Some(&text));
                }
                Err(error) => self.fail("No se pudo reconocer el texto", &error),
            },
            Event::ClipboardSnapshot(items) => {
                self.clipboard.snapshot_ready(items, &self.qh);
                self.sync_clipboard_owner();
            }
            Event::ClipboardImage(bytes) => self.clipboard_image(bytes),
            Event::Editor(message, payload) => self.editor_message(message, payload),
            Event::EditorGone => {
                self.editor = None;
                self.announced_owner = None;
            }
            Event::FolderChosen(path) => {
                self.settings.save_directory = Some(path);
                self.settings.save();
                let settings = self.settings.clone();
                self.update_tray(move |tray| tray.settings = settings);
            }
            Event::Failure(summary, detail) => self.fail(&summary, &detail),
            Event::ShortcutsChanged => {
                let keys = crate::shortcuts::bound(&self.dbus);
                self.update_tray(move |tray| tray.keys = keys);
            }
            Event::RecordingEnded(generation, result) => self.recording_ended(generation, result),
            Event::ScrollFinished(generation, result) => self.scroll_finished(generation, result),
            Event::ScrollStatus(generation, status) => {
                if let Some(scrolling) = self.scrolling.as_mut().filter(|scrolling| scrolling.generation == generation) {
                    scrolling.show(status);
                }
            }
        }
    }

    fn dispatch(&mut self, action: Action, from_menu: bool) {
        if from_menu && is_capture(action) {
            let sender = self.sender.clone();
            let _ = self.handle.insert_source(Timer::from_duration(Duration::from_millis(200)), move |_, _, _| {
                let _ = sender.send(Event::Action(action, false));
                TimeoutAction::Drop
            });
            return;
        }
        if self.recording.is_some() || self.scrolling.is_some() {
            if matches!(action, Action::StopRecording | Action::RecordVideo | Action::RecordGif | Action::ScrollingCapture) {
                self.stop_session(true);
                return;
            }
            if is_capture(action) {
                return;
            }
        }
        match action {
            Action::CaptureRegion => self.select(Mode::Select, Purpose::Capture),
            Action::PinRegion => self.select(Mode::Select, Purpose::Pin),
            Action::RecognizeText => self.select(Mode::Select, Purpose::Recognize),
            Action::ScrollingCapture => self.select(Mode::Select, Purpose::Scroll),
            Action::RecordVideo => self.select(Mode::Select, Purpose::Video),
            Action::RecordGif => self.select(Mode::Select, Purpose::Gif),
            Action::Measure => self.select(Mode::Measure, Purpose::Measure),
            Action::PickColor => self.select(Mode::Pick, Purpose::Pick),
            Action::CaptureScreen => self.capture(Target::ActiveScreen),
            Action::CaptureWindow => self.capture(Target::ActiveWindow),
            Action::Toggle(toggle) => self.toggle(toggle),
            Action::ChooseFolder => {
                let (dbus, sender, current) = (self.dbus.clone(), self.sender.clone(), self.settings.save_directory());
                std::thread::spawn(move || {
                    if let Some(path) = dbus::choose_folder(&dbus, "Elegir carpeta de guardado", &current) {
                        let _ = sender.send(Event::FolderChosen(path));
                    }
                });
            }
            Action::ShowHistory => self.editor_send(ToEditor::ShowHistory, editor::Payload::None),
            Action::PinClipboard => self.request_image(ImageRequest::Pin),
            Action::ClosePins => {
                self.pins.close_all();
                self.pins_changed();
            }
            Action::OpenFolder => {
                let directory = self.settings.save_directory();
                let _ = std::fs::create_dir_all(&directory);
                dbus::open_uri(&self.dbus, &dbus::file_uri(&directory));
            }
            Action::StopRecording => {}
            Action::Quit => {
                self.stop_session(false);
                if let Some(recording) = self.recording.as_ref() {
                    let _ = std::fs::remove_file(&recording.path);
                }
                self.signal.stop();
            }
        }
    }

    fn capture(&mut self, target: Target<'static>) {
        if self.busy {
            return;
        }
        self.busy = true;
        let (dbus, sender, cursor) = (self.dbus.clone(), self.sender.clone(), self.settings.shows_cursor);
        std::thread::spawn(move || {
            let _ = sender.send(Event::Shot(screenshot::capture(&dbus, target, cursor)));
        });
    }

    fn select(&mut self, mode: Mode, purpose: Purpose) {
        if self.busy || self.overlay.is_some() {
            return;
        }
        let outputs: Vec<OutputChoice> = self
            .outputs
            .outputs()
            .filter_map(|output| {
                let info = self.outputs.info(&output)?;
                let (x, y) = info.logical_position?;
                let (width, height) = info.logical_size?;
                let mode = info.modes.iter().find(|mode| mode.current)?;
                let rect = Rect { x: f64::from(x), y: f64::from(y), width: f64::from(width), height: f64::from(height) };
                Some((output.clone(), info.name.clone()?, rect, (mode.dimensions.0 as u32, mode.dimensions.1 as u32), f64::from(info.scale_factor)))
            })
            .collect();
        let rects: Vec<Rect> = outputs.iter().map(|output| output.2).collect();
        let finder = (mode == Mode::Select).then(|| {
            let dbus = self.dbus.clone();
            std::thread::spawn(move || windows::visible(&dbus, &rects))
        });
        let mut frozen = Vec::new();
        for (output, name, rect, pixels, scale) in outputs {
            let Ok(mut pool) = RawPool::new(pixels.0 as usize * pixels.1 as usize * 3, &self.globals.shm) else { continue };
            let captured = screenshot::capture_with(&self.dbus, Target::Screen(&name), self.settings.shows_cursor, |width, height| {
                let length = width as usize * height as usize * 3;
                if pool.len() < length {
                    pool.resize(length).map_err(|error| error.to_string())?;
                }
                Ok(&mut pool.mmap()[..length])
            });
            match captured {
                Ok((_, width, height, _)) => frozen.push(FrozenScreen {
                    output,
                    name,
                    origin: (rect.x, rect.y),
                    size: (rect.width, rect.height),
                    pool,
                    width,
                    height,
                    scale,
                }),
                Err(error) => {
                    self.fail("No se pudo capturar la pantalla", &error);
                    return;
                }
            }
        }
        let windows = finder.and_then(|finder| finder.join().ok()).unwrap_or_default();
        match Overlay::new(mode, frozen, windows, &self.globals, &self.qh) {
            Some(overlay) => self.overlay = Some((overlay, purpose)),
            None => self.fail("No se pudo mostrar la selección", "No hay pantallas disponibles"),
        }
    }

    fn handle_request(&mut self, request: Option<Request>) {
        match request {
            Some(Request::Finish(selection)) => {
                if let Some((overlay, purpose)) = self.overlay.take() {
                    overlay.destroy();
                    memory::release();
                    if let Some(selection) = selection {
                        self.selected(selection, purpose);
                    }
                }
            }
            Some(Request::CopyText { text, title, detail }) => {
                self.clipboard.set_text(text, &self.qh);
                self.show_toast(&title, Some(&detail));
                if self.overlay.as_ref().is_some_and(|(overlay, _)| overlay.is_finished()) {
                    self.handle_request(Some(Request::Finish(None)));
                }
            }
            None => {}
        }
    }

    fn selected(&mut self, selection: Selection, purpose: Purpose) {
        match purpose {
            Purpose::Capture => {
                if let Some(image) = selection.image {
                    self.deliver(image, selection.ratio);
                }
            }
            Purpose::Pin => {
                if let Some(image) = selection.image {
                    self.pin(image, selection.ratio, Some((selection.output, selection.global)));
                }
            }
            Purpose::Recognize => {
                if let Some(image) = selection.image {
                    self.recognize(image, selection.ratio);
                }
            }
            Purpose::Scroll => self.start_scrolling(selection),
            Purpose::Video => self.start_recording(Format::Video, selection),
            Purpose::Gif => self.start_recording(Format::Gif, selection),
            Purpose::Measure | Purpose::Pick => {}
        }
    }

    pub fn chrome(&mut self) -> Option<&mut Chrome> {
        match (self.recording.as_mut(), self.scrolling.as_mut()) {
            (Some(recording), _) => Some(&mut recording.chrome),
            (None, Some(scrolling)) => Some(&mut scrolling.chrome),
            (None, None) => None,
        }
    }

    fn stop_session(&mut self, keep: bool) {
        if let Some(scrolling) = self.scrolling.as_mut() {
            if keep {
                scrolling.stop();
            } else {
                scrolling.cancel();
            }
            return;
        }
        self.stop_recording(keep);
    }

    fn start_scrolling(&mut self, selection: Selection) {
        let screens = placements(&self.outputs);
        let Some(placement) = screens.iter().find(|placement| placement.name == selection.output) else {
            self.fail("No se pudo iniciar la captura con desplazamiento", "La pantalla seleccionada ya no está disponible");
            return;
        };
        let region = Rect { x: selection.global.x.round(), y: selection.global.y.round(), width: selection.global.width.round(), height: selection.global.height.round() };
        if region.width < 2.0 || region.height < 2.0 {
            return;
        }
        let scroller = self.globals.fake_input.clone().map(|input| Scroller { input, connection: self.connection.clone() });
        let status = if scroller.is_some() { scroll::Status::Automatic } else { scroll::Status::Manual };
        let mut chrome = Chrome::new(&placement.output, placement.rect, region, status.title(), status.offers_manual(), &self.globals, &self.qh);
        if let Some(scroller) = &scroller {
            scroller.park(region);
            chrome.guard(screens.iter().map(|screen| (&screen.output, screen.rect)), region, &self.globals, &self.qh);
        }
        let generation = self.next_id();
        self.scrolling = Some(Scrolling::start(generation, region, chrome, scroller, self.dbus.clone(), self.sender.clone()));
        self.update_tray(|tray| tray.recording = true);
        let _ = self.handle.insert_source(Timer::from_duration(TICK), move |_, _, app: &mut App| match app.scrolling.as_mut() {
            Some(scrolling) if scrolling.generation == generation => {
                scrolling.chrome.tick();
                TimeoutAction::ToDuration(TICK)
            }
            _ => TimeoutAction::Drop,
        });
    }

    fn scroll_finished(&mut self, generation: u64, result: scroll::Finished) {
        if !self.scrolling.as_ref().is_some_and(|scrolling| scrolling.generation == generation) {
            return;
        }
        let Some(scrolling) = self.scrolling.take() else { return };
        scrolling.chrome.destroy();
        self.update_tray(|tray| tray.recording = false);
        match result {
            Ok(Some((image, scale))) => self.deliver(image, scale),
            Ok(None) => {}
            Err(error) => self.fail("No se pudo completar la captura con desplazamiento", &error),
        }
        memory::release();
    }

    fn start_recording(&mut self, format: Format, selection: Selection) {
        let Some(screencast) = self.globals.screencast.clone() else {
            self.fail("No se pudo grabar", "KWin no ofrece zkde_screencast_unstable_v1");
            return;
        };
        let Some(placement) = placements(&self.outputs).into_iter().find(|placement| placement.name == selection.output) else {
            self.fail("No se pudo grabar", "La pantalla seleccionada ya no está disponible");
            return;
        };
        let region = Rect { x: selection.global.x.round(), y: selection.global.y.round(), width: selection.global.width.round(), height: selection.global.height.round() };
        if region.width < 2.0 || region.height < 2.0 {
            return;
        }
        let size = (region.width as u32, region.height as u32);
        let created = time::now();
        let saved_automatically = self.settings.auto_save;
        let directory = if saved_automatically { self.settings.save_directory() } else { paths::recordings_dir() };
        if let Err(error) = std::fs::create_dir_all(&directory) {
            self.fail("No se pudo grabar", &error.to_string());
            return;
        }
        let path = image::unique_path(&directory, created, format.extension());
        let stream = screencast.stream_region(region.x as i32, region.y as i32, size.0, size.1, format.scale(selection.ratio), record::POINTER_EMBEDDED, &self.qh, StreamData);
        let chrome = Chrome::new(&placement.output, placement.rect, region, format.title(), false, &self.globals, &self.qh);
        let generation = self.next_id();
        self.recording = Some(Recording {
            generation,
            format,
            path,
            created,
            size,
            pixels: record::pixel_size(format, size, selection.ratio),
            saved_automatically,
            audio: format == Format::Video && self.settings.records_audio,
            stream,
            process: None,
            chrome,
            stopping: None,
        });
        self.update_tray(|tray| tray.recording = true);
        let _ = self.handle.insert_source(Timer::from_duration(TICK), move |_, _, app: &mut App| match app.recording.as_mut() {
            Some(recording) if recording.generation == generation => {
                recording.chrome.tick();
                TimeoutAction::ToDuration(TICK)
            }
            _ => TimeoutAction::Drop,
        });
    }

    pub fn stream_event(&mut self, stream: &ZkdeScreencastStreamUnstableV1, event: zkde_screencast_stream_unstable_v1::Event) {
        let Some(recording) = self.recording.as_mut().filter(|recording| &recording.stream == stream) else { return };
        let generation = recording.generation;
        match event {
            zkde_screencast_stream_unstable_v1::Event::Created { node } => {
                if recording.process.is_some() || recording.stopping.is_some() {
                    return;
                }
                match record::launch(recording, node, self.sender.clone()) {
                    Ok(pid) => recording.process = Some(pid),
                    Err(error) => self.recording_ended(generation, Err(error)),
                }
            }
            zkde_screencast_stream_unstable_v1::Event::Failed { error } => match recording.process {
                Some(_) => self.stop_recording(true),
                None => self.recording_ended(generation, Err(error)),
            },
            zkde_screencast_stream_unstable_v1::Event::Closed => self.stop_recording(true),
            _ => {}
        }
    }

    fn stop_recording(&mut self, keep: bool) {
        let Some(recording) = self.recording.as_mut() else { return };
        if recording.stopping.is_some() {
            return;
        }
        recording.stopping = Some(keep);
        recording.chrome.show_processing();
        let generation = recording.generation;
        let Some(pid) = recording.process else {
            self.recording_ended(generation, if keep { Err("La grabación no tiene cuadros.".into()) } else { Ok(()) });
            return;
        };
        record::signal(pid, if keep { libc::SIGINT } else { libc::SIGKILL });
        let _ = self.handle.insert_source(Timer::from_duration(STOP_TIMEOUT), move |_, _, app: &mut App| {
            if let Some(pid) = app.recording.as_ref().filter(|recording| recording.generation == generation).and_then(|recording| recording.process) {
                record::signal(pid, libc::SIGKILL);
            }
            TimeoutAction::Drop
        });
    }

    fn recording_ended(&mut self, generation: u64, result: Result<(), String>) {
        if !self.recording.as_ref().is_some_and(|recording| recording.generation == generation) {
            return;
        }
        let Some(recording) = self.recording.take() else { return };
        recording.stream.close();
        recording.chrome.destroy();
        self.update_tray(|tray| tray.recording = false);
        let keep = recording.stopping.unwrap_or(true);
        let written = std::fs::metadata(&recording.path).is_ok_and(|meta| meta.len() > 0);
        match result {
            Ok(()) if keep && written => {
                let size = (f64::from(recording.size.0), f64::from(recording.size.1));
                self.deliver_media(recording.path, recording.format == Format::Gif, size, recording.created, recording.saved_automatically);
            }
            outcome => {
                let _ = std::fs::remove_file(&recording.path);
                if keep {
                    let detail = outcome.err().unwrap_or_else(|| "La grabación no tiene cuadros.".into());
                    self.fail("No se pudo terminar la grabación", &detail);
                }
            }
        }
        memory::release();
    }

    fn deliver_media(&mut self, path: PathBuf, gif: bool, size: (f64, f64), created: i64, saved_automatically: bool) {
        let id = self.next_id();
        if saved_automatically {
            history::record(&path);
        }
        if self.settings.copy_on_capture {
            self.clipboard.copy(id, Payload::File { path: path.clone(), gif }, &self.connection, &self.qh, &self.sender);
            self.sync_clipboard_owner();
        }
        if self.settings.open_editor {
            self.editor_send(ToEditor::OpenMedia { id, path, gif, width: size.0, height: size.1, created, saved_automatically }, editor::Payload::None);
        }
    }

    pub fn surface_scale(&mut self, surface: &wl_surface::WlSurface, scale: f64, qh: &QueueHandle<App>) {
        if let Some((overlay, _)) = self.overlay.as_mut() {
            overlay.set_scale(surface, scale, qh);
        }
        if let Some(toast) = self.toast.as_mut().filter(|toast| toast.owns_surface(surface)) {
            toast.set_scale(scale);
        }
        self.pins.set_scale(surface, scale);
        if let Some(chrome) = self.chrome() {
            chrome.set_scale(surface, scale);
        }
    }

    pub fn show_toast(&mut self, title: &str, detail: Option<&str>) {
        if let Some(previous) = self.toast.take() {
            previous.destroy();
        }
        self.toasts += 1;
        let generation = self.toasts;
        self.toast = Some(Toast::new(generation, title, detail, &self.globals, &self.qh));
        let _ = self.handle.insert_source(Timer::from_duration(Duration::from_millis(toast::VISIBLE_MS)), move |_, _, app: &mut App| {
            if app.toast.as_ref().is_some_and(|toast| toast.generation == generation)
                && let Some(toast) = app.toast.take() {
                    toast.destroy();
                }
            TimeoutAction::Drop
        });
    }

    fn toggle(&mut self, toggle: Toggle) {
        let settings = &mut self.settings;
        match toggle {
            Toggle::CopyOnCapture => settings.copy_on_capture = !settings.copy_on_capture,
            Toggle::AutoSave => settings.auto_save = !settings.auto_save,
            Toggle::OpenEditor => settings.open_editor = !settings.open_editor,
            Toggle::ShowsCursor => settings.shows_cursor = !settings.shows_cursor,
            Toggle::RecordsAudio => settings.records_audio = !settings.records_audio,
            Toggle::LaunchAtLogin => set_launch_at_login(!launch_at_login()),
        }
        settings.save();
        let (settings, login) = (settings.clone(), launch_at_login());
        self.update_tray(move |tray| {
            tray.settings = settings;
            tray.launch_at_login = login;
        });
    }

    pub fn sync_clipboard_owner(&mut self) {
        let owner = self.clipboard.owner();
        if self.announced_owner == Some(owner) {
            return;
        }
        if let Some(editor) = self.editor.as_mut() {
            editor.send(ToEditor::ClipboardOwner { id: owner }, editor::Payload::None);
            self.announced_owner = Some(owner);
        }
    }

    fn editor_send(&mut self, message: ToEditor, payload: editor::Payload) {
        if self.editor.is_none() {
            match EditorLink::spawn(self.sender.clone()) {
                Ok(link) => {
                    self.editor = Some(link);
                    self.announced_owner = None;
                }
                Err(error) => {
                    self.fail("No se pudo abrir el editor", &error);
                    return;
                }
            }
        }
        if let Some(editor) = self.editor.as_mut() {
            editor.send(message, payload);
        }
        self.sync_clipboard_owner();
    }

    fn editor_message(&mut self, message: ToDaemon, payload: Vec<u8>) {
        match message {
            ToDaemon::Copy { id } => {
                self.clipboard.copy(id, Payload::Png(Arc::new(payload)), &self.connection, &self.qh, &self.sender);
                self.sync_clipboard_owner();
            }
            ToDaemon::Changed { id } => self.clipboard.content_changed(id, Payload::Png(Arc::new(payload)), &self.qh),
            ToDaemon::Discard { id } => {
                self.clipboard.discard(id, &self.qh);
                self.sync_clipboard_owner();
            }
            ToDaemon::Release { id } => {
                self.clipboard.release(id);
                self.sync_clipboard_owner();
            }
            ToDaemon::CopyFile { id, path, gif } => {
                self.clipboard.copy(id, Payload::File { path, gif }, &self.connection, &self.qh, &self.sender);
                self.sync_clipboard_owner();
            }
            ToDaemon::CopyFiles { paths } => {
                let id = self.next_id();
                self.clipboard.copy(id, Payload::Files(Arc::new(paths)), &self.connection, &self.qh, &self.sender);
                self.sync_clipboard_owner();
            }
            ToDaemon::CopyText { text } => {
                self.clipboard.set_text(text, &self.qh);
                self.sync_clipboard_owner();
            }
            ToDaemon::Pin { width, height, scale } => {
                if payload.len() == width as usize * height as usize * 3 {
                    self.pin(RgbImage { width, height, data: payload.into() }, scale, None);
                }
            }
            ToDaemon::Toast { title, detail } => self.show_toast(&title, detail.as_deref()),
            ToDaemon::PasteRequest => self.request_image(ImageRequest::Paste),
            ToDaemon::Activate { history } => {
                if let Some(pid) = self.editor.as_ref().and_then(EditorLink::pid) {
                    std::thread::spawn(move || {
                        for _ in 0..50 {
                            if windows::activate(pid, history) {
                                return;
                            }
                            std::thread::sleep(Duration::from_millis(100));
                        }
                    });
                }
            }
            ToDaemon::Exiting { received } => {
                if self.editor.as_ref().is_some_and(|editor| editor.sent == received) {
                    if let Some(mut editor) = self.editor.take() {
                        editor.send(ToEditor::ExitAccepted, editor::Payload::None);
                    }
                    self.announced_owner = None;
                }
            }
        }
    }

    fn request_image(&mut self, request: ImageRequest) {
        if let Some(png) = self.clipboard.owned_png() {
            self.image_request = Some(request);
            self.clipboard_image((*png).clone());
            return;
        }
        self.image_request = Some(request);
        if !self.clipboard.read_image(&self.connection, &self.sender) {
            self.image_request = None;
            self.show_toast("El portapapeles no tiene una imagen", None);
        }
    }

    fn clipboard_image(&mut self, bytes: Vec<u8>) {
        match self.image_request.take() {
            Some(ImageRequest::Paste) => {
                if let Some(editor) = self.editor.as_mut() {
                    editor.send(ToEditor::PasteImage { scale: 1.0 }, editor::Payload::Bytes(Arc::new(bytes)));
                }
            }
            Some(ImageRequest::Pin) => match image::decode(&bytes) {
                Some(decoded) => {
                    let image = RgbImage::from_rgba_over(decoded.width, decoded.height, &decoded.rgba, [255, 255, 255]);
                    self.pin(image, decoded.scale, None);
                }
                None => self.show_toast("El portapapeles no tiene una imagen", None),
            },
            None => {}
        }
    }

    pub fn pin(&mut self, image: RgbImage, scale: f64, at: Option<(String, Rect)>) {
        let placements = placements(&self.outputs);
        let wanted = match &at {
            Some((name, _)) => Some(name.clone()),
            None => self.dbus.call_method(Some("org.kde.KWin"), "/KWin", Some("org.kde.KWin"), "activeOutputName", &()).ok().and_then(|reply| reply.body().deserialize::<String>().ok()),
        };
        let Some(placement) = placements.iter().find(|placement| Some(&placement.name) == wanted.as_ref()).or(placements.first()).cloned() else {
            self.fail("No se pudo fijar la imagen", "No hay pantallas disponibles");
            return;
        };
        match self.pins.add(image, scale, placement, at.map(|(_, rect)| rect), &self.globals, &self.qh) {
            Ok(()) => self.pins_changed(),
            Err(error) => self.fail("No se pudo fijar la imagen", &error),
        }
    }

    fn pins_changed(&self) {
        let has_pins = !self.pins.is_empty();
        self.update_tray(move |tray| tray.has_pins = has_pins);
        memory::release();
    }

    fn pin_request(&mut self, request: PinRequest) {
        match request {
            PinRequest::Changed => self.pins_changed(),
            PinRequest::Copy(image, scale) => {
                let (id, sender) = (self.next_id(), self.sender.clone());
                std::thread::spawn(move || match image::encode_png(&image, scale, png::Compression::Fast) {
                    Ok(png) => {
                        let _ = sender.send(Event::CopyPng(id, png));
                    }
                    Err(error) => {
                        let _ = sender.send(Event::Failure("No se pudo copiar la imagen".into(), error.to_string()));
                    }
                });
            }
            PinRequest::Edit(image, scale) => {
                let id = self.next_id();
                let (width, height) = (image.width, image.height);
                self.editor_send(ToEditor::Open { id, width, height, scale, created: time::now(), saved: None }, editor::Payload::Image(Arc::new(image)));
            }
            PinRequest::SaveAs(image, scale) => {
                let (dbus, sender, directory) = (self.dbus.clone(), self.sender.clone(), self.settings.save_directory());
                std::thread::spawn(move || {
                    let name = image::unique_path(&directory, time::now(), "png").file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
                    let Some(path) = dbus::save_file(&dbus, "Guardar como", &directory, &name, &[("Imagen PNG", "*.png"), ("Imagen JPEG", "*.jpg")]) else { return };
                    match image::write(&image, scale, &path) {
                        Ok(()) => history::record(&path),
                        Err(error) => {
                            let _ = sender.send(Event::Failure("No se pudo guardar".into(), error.to_string()));
                        }
                    }
                });
            }
            PinRequest::Recognize(image, scale) => self.recognize(image, scale),
        }
    }

    fn recognize(&mut self, image: RgbImage, scale: f64) {
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let _ = sender.send(Event::Recognized(ocr::recognize(&image, scale)));
        });
    }

    pub fn deliver(&mut self, image: RgbImage, scale: f64) {
        let id = self.next_id();
        let image = Arc::new(image);
        let created = time::now();
        let save = self.settings.auto_save.then(|| image::unique_path(&self.settings.save_directory(), created, "png"));
        if self.settings.open_editor || !(self.settings.copy_on_capture || self.settings.auto_save) {
            let (width, height) = (image.width, image.height);
            self.editor_send(ToEditor::Open { id, width, height, scale, created, saved: save.clone() }, editor::Payload::Image(image.clone()));
        }
        if self.settings.copy_on_capture || self.settings.auto_save {
            let (sender, image) = (self.sender.clone(), image.clone());
            std::thread::spawn(move || match image::encode_png(&image, scale, png::Compression::Fast) {
                Ok(png) => {
                    let _ = sender.send(Event::Encoded { id, png, save });
                }
                Err(error) => {
                    let _ = sender.send(Event::Failure("No se pudo codificar la captura".into(), error.to_string()));
                }
            });
        }
    }
}

fn placements(outputs: &OutputState) -> Vec<Placement> {
    outputs
        .outputs()
        .filter_map(|output| {
            let info = outputs.info(&output)?;
            let (x, y) = info.logical_position?;
            let (width, height) = info.logical_size?;
            let rect = Rect { x: f64::from(x), y: f64::from(y), width: f64::from(width), height: f64::from(height) };
            Some(Placement { name: info.name.clone()?, output, rect })
        })
        .collect()
}

fn is_capture(action: Action) -> bool {
    matches!(
        action,
        Action::CaptureRegion
            | Action::CaptureScreen
            | Action::CaptureWindow
            | Action::ScrollingCapture
            | Action::RecognizeText
            | Action::PinRegion
            | Action::RecordVideo
            | Action::RecordGif
            | Action::PickColor
            | Action::Measure
    )
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seats
    }
    fn new_seat(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat) {
        self.clipboard.attach_seat(&seat, qh);
        self.seat.get_or_insert(seat);
    }
    fn new_capability(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat, capability: Capability) {
        self.clipboard.attach_seat(&seat, qh);
        self.seat.get_or_insert(seat.clone());
        if capability == Capability::Pointer && self.pointer.is_none()
            && let Ok(pointer) = self.seats.get_pointer(qh, &seat) {
                self.cursor_shape = self.globals.cursor_device(&pointer, qh);
                self.pinch = self.globals.pinch(&pointer, qh);
                self.pointer = Some(pointer);
            }
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            self.keyboard = self.seats.get_keyboard(qh, &seat, None).ok();
        }
    }
    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, _: Capability) {}
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl CompositorHandler for App {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, qh: &QueueHandle<Self>, surface: &wl_surface::WlSurface, _: u32) {
        if let Some((overlay, _)) = self.overlay.as_mut() {
            overlay.frame(surface, qh);
        }
    }
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl LayerShellHandler for App {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface) {
        if self.chrome().is_some_and(|chrome| chrome.owns_layer(layer)) {
            self.stop_session(true);
            return;
        }
        if self.overlay.as_ref().is_some_and(|(overlay, _)| overlay.owns_layer(layer).is_some()) {
            self.handle_request(Some(Request::Finish(None)));
        }
        if self.pins.closed(layer) {
            self.pins_changed();
        }
    }

    fn configure(&mut self, connection: &Connection, qh: &QueueHandle<Self>, layer: &LayerSurface, configure: LayerSurfaceConfigure, _: u32) {
        if let Some(toast) = self.toast.as_mut().filter(|toast| toast.owns(layer)) {
            toast.configure();
            return;
        }
        if self.pins.owns_layer(layer) {
            self.pins.configure(layer, connection, qh);
            return;
        }
        if let Some(chrome) = self.chrome().filter(|chrome| chrome.owns_layer(layer)) {
            chrome.configure(layer);
            return;
        }
        if let Some((overlay, _)) = self.overlay.as_mut()
            && let Some(index) = overlay.owns_layer(layer) {
                overlay.configure(index, configure.new_size, &self.globals, qh);
            }
    }
}

impl PopupHandler for App {
    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, popup: &Popup, _: PopupConfigure) {
        self.pins.popup_configure(popup);
    }

    fn done(&mut self, _: &Connection, _: &QueueHandle<Self>, popup: &Popup) {
        self.pins.popup_done(popup);
    }
}

impl PointerHandler for App {
    fn pointer_frame(&mut self, connection: &Connection, qh: &QueueHandle<Self>, _: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        for event in events {
            if self.chrome().is_some_and(|chrome| chrome.owns(&event.surface)) {
                if let PointerEventKind::Enter { serial } = event.kind {
                    wl::set_cursor(self.cursor_shape.as_ref(), serial, Shape::Default);
                }
                match self.chrome().and_then(|chrome| chrome.pointer(event, connection, qh)) {
                    Some(ChromeAction::Manual) => {
                        if let Some(scrolling) = self.scrolling.as_mut() {
                            scrolling.go_manual();
                        }
                    }
                    Some(ChromeAction::Stop) => self.stop_session(true),
                    Some(ChromeAction::Cancel) => self.stop_session(false),
                    None => {}
                }
                continue;
            }
            if self.pins.owns(&event.surface) {
                let outputs = &self.outputs;
                let (request, cursor) = self.pins.pointer(event, self.alt, self.seat.as_ref(), self.xdg.as_ref(), || placements(outputs), connection, &self.globals, qh);
                if let Some((serial, grabbing)) = cursor {
                    wl::set_cursor(self.cursor_shape.as_ref(), serial, if grabbing { Shape::Grabbing } else { Shape::Default });
                }
                if let Some(request) = request {
                    self.pin_request(request);
                }
                continue;
            }
            let Some((overlay, _)) = self.overlay.as_mut() else { continue };
            let Some(index) = overlay.owns(&event.surface) else { continue };
            let request = match event.kind {
                PointerEventKind::Enter { serial } => {
                    wl::set_cursor(self.cursor_shape.as_ref(), serial, Shape::Crosshair);
                    overlay.pointer_enter(index, event.position, qh);
                    None
                }
                PointerEventKind::Leave { .. } => {
                    overlay.pointer_leave(index, qh);
                    None
                }
                PointerEventKind::Motion { .. } => {
                    overlay.pointer_motion(index, event.position, qh);
                    None
                }
                PointerEventKind::Press { button, .. } => overlay.press(index, button, qh),
                PointerEventKind::Release { button, .. } => overlay.release(index, button, qh),
                PointerEventKind::Axis { .. } => None,
            };
            self.handle_request(request);
        }
    }
}

impl KeyboardHandler for App {
    fn enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, surface: &wl_surface::WlSurface, _: u32, _: &[u32], _: &[Keysym]) {
        self.keyboard_focus = Some(surface.clone());
    }
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, surface: &wl_surface::WlSurface, _: u32) {
        if self.keyboard_focus.as_ref() == Some(surface) {
            self.keyboard_focus = None;
        }
    }
    fn press_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, event: KeyEvent) {
        if let Some(focus) = self.keyboard_focus.clone().filter(|focus| self.pins.owns(focus)) {
            if let Some(request) = self.pins.key(&focus, event.keysym.raw()) {
                self.pin_request(request);
            }
            return;
        }
        let ctrl = self.ctrl;
        let request = self.overlay.as_mut().and_then(|(overlay, _)| overlay.key(event.keysym.raw(), ctrl));
        self.handle_request(request);
    }
    fn repeat_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: KeyEvent) {}
    fn release_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: KeyEvent) {}
    fn update_modifiers(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, modifiers: Modifiers, _: RawModifiers, _: u32) {
        self.ctrl = modifiers.ctrl;
        self.alt = modifiers.alt;
    }
}

impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.globals.shm
    }
}

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    registry_handlers![OutputState, SeatState];
}

delegate_registry!(App);
smithay_client_toolkit::delegate_dispatch2!(App);
