use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use wayland_client::backend::{Backend, ObjectId};
use wayland_client::protocol::wl_data_device::{self, WlDataDevice};
use wayland_client::protocol::wl_data_device_manager::{DndAction, WlDataDeviceManager};
use wayland_client::protocol::wl_data_source::{self, WlDataSource};
use wayland_client::protocol::wl_pointer::{self, WlPointer};
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle};

const PNG: &str = "image/png";
const URI_LIST: &str = "text/uri-list";

#[derive(Default)]
struct State {
    seat: Option<WlSeat>,
    manager: Option<WlDataDeviceManager>,
    press: Option<u32>,
    payload: Option<(Arc<Vec<u8>>, String)>,
    source: Option<WlDataSource>,
}

pub struct Drag {
    connection: Connection,
    queue: EventQueue<State>,
    state: State,
    device: WlDataDevice,
    surface: WlSurface,
    _pointer: WlPointer,
}

impl Drag {
    pub fn attach(window: &slint::Window) -> Option<Drag> {
        let handle = window.window_handle();
        let RawDisplayHandle::Wayland(display) = handle.display_handle().ok()?.as_raw() else { return None };
        let RawWindowHandle::Wayland(surface) = handle.window_handle().ok()?.as_raw() else { return None };
        let backend = unsafe { Backend::from_foreign_display(display.display.as_ptr().cast()) };
        let connection = Connection::from_backend(backend);
        let surface_id = unsafe { ObjectId::from_ptr(WlSurface::interface(), surface.surface.as_ptr().cast()) }.ok()?;
        let surface = WlSurface::from_id(&connection, surface_id).ok()?;
        let mut queue = connection.new_event_queue();
        let handle = queue.handle();
        connection.display().get_registry(&handle, ());
        let mut state = State::default();
        queue.roundtrip(&mut state).ok()?;
        let seat = state.seat.clone()?;
        let manager = state.manager.clone()?;
        let pointer = seat.get_pointer(&handle, ());
        let device = manager.get_data_device(&seat, &handle, ());
        connection.flush().ok()?;
        Some(Drag { connection, queue, state, device, surface, _pointer: pointer })
    }

    pub fn pump(&mut self) {
        let _ = self.queue.dispatch_pending(&mut self.state);
        let _ = self.connection.flush();
    }

    pub fn active(&self) -> bool {
        self.state.source.is_some()
    }

    pub fn start(&mut self, png: Vec<u8>, file: PathBuf) -> bool {
        self.pump();
        let (Some(serial), Some(manager)) = (self.state.press, self.state.manager.clone()) else { return false };
        if std::fs::write(&file, &png).is_err() {
            return false;
        }
        let uri = format!("file://{}\r\n", percent_encode(&file.to_string_lossy()));
        let handle = self.queue.handle();
        let source = manager.create_data_source(&handle, ());
        source.offer(PNG.into());
        source.offer(URI_LIST.into());
        if source.version() >= 3 {
            source.set_actions(DndAction::Copy);
        }
        self.device.start_drag(Some(&source), &self.surface, None, serial);
        self.state.payload = Some((Arc::new(png), uri));
        self.state.source = Some(source);
        let _ = self.connection.flush();
        true
    }
}

pub fn drag_file(created: i64) -> Option<PathBuf> {
    let directory = sharek_common::paths::cache_dir().join("drag");
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).ok()?;
    let name = sharek_common::image::unique_path(&directory, created, "png");
    Some(name)
}

fn percent_encode(path: &str) -> String {
    path.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => (byte as char).to_string(),
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

impl Dispatch<WlRegistry, ()> for State {
    fn event(state: &mut State, registry: &WlRegistry, event: wl_registry::Event, _: &(), _: &Connection, handle: &QueueHandle<State>) {
        if let wl_registry::Event::Global { name, interface, version } = event {
            match interface.as_str() {
                "wl_seat" if state.seat.is_none() => state.seat = Some(registry.bind(name, version.min(7), handle, ())),
                "wl_data_device_manager" => state.manager = Some(registry.bind(name, version.min(3), handle, ())),
                _ => {}
            }
        }
    }
}

impl Dispatch<WlPointer, ()> for State {
    fn event(state: &mut State, _: &WlPointer, event: wl_pointer::Event, _: &(), _: &Connection, _: &QueueHandle<State>) {
        if let wl_pointer::Event::Button { serial, state: wayland_client::WEnum::Value(wl_pointer::ButtonState::Pressed), .. } = event {
            state.press = Some(serial);
        }
    }
}

impl Dispatch<WlDataSource, ()> for State {
    fn event(state: &mut State, source: &WlDataSource, event: wl_data_source::Event, _: &(), _: &Connection, _: &QueueHandle<State>) {
        match event {
            wl_data_source::Event::Send { mime_type, fd } => {
                let Some((png, uri)) = state.payload.clone() else { return };
                std::thread::spawn(move || {
                    let mut file = std::fs::File::from(fd);
                    let _ = if mime_type == PNG { file.write_all(&png) } else { file.write_all(uri.as_bytes()) };
                });
            }
            wl_data_source::Event::Cancelled | wl_data_source::Event::DndFinished => {
                source.destroy();
                state.source = None;
                state.payload = None;
            }
            _ => {}
        }
    }
}

wayland_client::delegate_noop!(State: ignore WlSeat);
wayland_client::delegate_noop!(State: ignore WlDataDeviceManager);
wayland_client::delegate_noop!(State: ignore WlSurface);

impl Dispatch<WlDataDevice, ()> for State {
    fn event(_: &mut State, _: &WlDataDevice, _: wl_data_device::Event, _: &(), _: &Connection, _: &QueueHandle<State>) {}

    fn event_created_child(opcode: u16, handle: &QueueHandle<State>) -> Arc<dyn wayland_client::backend::ObjectData> {
        match opcode {
            wl_data_device::EVT_DATA_OFFER_OPCODE => handle.make_data::<wayland_client::protocol::wl_data_offer::WlDataOffer, ()>(()),
            _ => panic!("unexpected child object"),
        }
    }
}

wayland_client::delegate_noop!(State: ignore wayland_client::protocol::wl_data_offer::WlDataOffer);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_uris_escape_spaces_and_accents() {
        assert_eq!(percent_encode("/home/a/Imágenes/mi captura.png"), "/home/a/Im%C3%A1genes/mi%20captura.png");
    }
}
