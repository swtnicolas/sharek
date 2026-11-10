use crate::app::{App, Event};
use crate::dbus::file_uri;
use smithay_client_toolkit::dispatch2::Dispatch2;
use smithay_client_toolkit::reexports::calloop::channel::Sender;
use std::io::{Read, Write};
use std::os::fd::{AsFd, OwnedFd};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{Connection, Proxy, QueueHandle};
use wayland_protocols::ext::data_control::v1::client::{
    ext_data_control_device_v1::{self, ExtDataControlDeviceV1},
    ext_data_control_manager_v1::ExtDataControlManagerV1,
    ext_data_control_offer_v1::{self, ExtDataControlOfferV1},
    ext_data_control_source_v1::{self, ExtDataControlSourceV1},
};

const HISTORY_LIMIT: usize = 5;
const SNAPSHOT_LIMIT: usize = 64 << 20;
const TEXT_TYPES: [&str; 5] = ["text/plain;charset=utf-8", "text/plain", "UTF8_STRING", "STRING", "TEXT"];

pub type Bytes = Arc<Vec<u8>>;

#[derive(Clone)]
pub enum Payload {
    Png(Bytes),
    File { path: PathBuf, gif: bool },
    Files(Arc<Vec<PathBuf>>),
    Text(Arc<String>),
    Snapshot(Arc<Vec<(String, Bytes)>>),
}

impl Payload {
    fn mime_types(&self) -> Vec<String> {
        match self {
            Payload::Png(_) => vec!["image/png".into()],
            Payload::File { gif, .. } => {
                let mut types = vec!["text/uri-list".into(), "x-special/gnome-copied-files".into()];
                if *gif {
                    types.push("image/gif".into());
                }
                types
            }
            Payload::Files(_) => vec!["text/uri-list".into(), "x-special/gnome-copied-files".into()],
            Payload::Text(_) => TEXT_TYPES.iter().map(|t| t.to_string()).collect(),
            Payload::Snapshot(items) => items.iter().map(|(mime, _)| mime.clone()).collect(),
        }
    }

    fn send(&self, mime: &str, fd: OwnedFd) {
        let payload = self.clone();
        let mime = mime.to_string();
        std::thread::spawn(move || {
            let mut file = std::fs::File::from(fd);
            let _ = match &payload {
                Payload::Png(bytes) => file.write_all(bytes),
                Payload::Text(text) => file.write_all(text.as_bytes()),
                Payload::Snapshot(items) => match items.iter().find(|(m, _)| *m == mime) {
                    Some((_, bytes)) => file.write_all(bytes),
                    None => Ok(()),
                },
                Payload::File { path, .. } => match mime.as_str() {
                    "image/gif" => std::fs::File::open(path).and_then(|mut source| std::io::copy(&mut source, &mut file).map(|_| ())),
                    "x-special/gnome-copied-files" => file.write_all(format!("copy\n{}", file_uri(path)).as_bytes()),
                    _ => file.write_all(format!("{}\r\n", file_uri(path)).as_bytes()),
                },
                Payload::Files(paths) => {
                    let uris: Vec<String> = paths.iter().map(|path| file_uri(path)).collect();
                    match mime.as_str() {
                        "x-special/gnome-copied-files" => file.write_all(format!("copy\n{}", uris.join("\n")).as_bytes()),
                        _ => file.write_all(uris.iter().map(|uri| format!("{uri}\r\n")).collect::<String>().as_bytes()),
                    }
                }
            };
        });
    }
}

struct Item {
    id: u64,
    payload: Payload,
}

pub struct Clipboard {
    manager: Option<ExtDataControlManagerV1>,
    device: Option<ExtDataControlDeviceV1>,
    offer: Option<ExtDataControlOfferV1>,
    source: Option<(ExtDataControlSourceV1, Payload)>,
    history: Vec<Item>,
    baseline: Option<Payload>,
    owns: bool,
    pending: Vec<Item>,
    snapshotting: bool,
}

pub struct ManagerData;
pub struct DeviceData;
pub struct SourceData;
#[derive(Default)]
pub struct OfferData(Mutex<Vec<String>>);

impl Clipboard {
    pub fn new(manager: Option<ExtDataControlManagerV1>) -> Clipboard {
        Clipboard { manager, device: None, offer: None, source: None, history: Vec::new(), baseline: None, owns: false, pending: Vec::new(), snapshotting: false }
    }

    pub fn attach_seat(&mut self, seat: &WlSeat, qh: &QueueHandle<App>) {
        if self.device.is_none()
            && let Some(manager) = &self.manager {
                self.device = Some(manager.get_data_device(seat, qh, DeviceData));
            }
    }

    fn offered_types(&self) -> Vec<String> {
        self.offer.as_ref().and_then(|offer| offer.data::<OfferData>()).map(|data| data.0.lock().unwrap().clone()).unwrap_or_default()
    }

    fn current(&self) -> Option<&Item> {
        self.history.last()
    }

    fn write(&mut self, payload: Payload, qh: &QueueHandle<App>) {
        let (Some(manager), Some(device)) = (&self.manager, &self.device) else { return };
        let source = manager.create_data_source(qh, SourceData);
        for mime in payload.mime_types() {
            source.offer(mime);
        }
        device.set_selection(Some(&source));
        if let Some((previous, _)) = self.source.replace((source, payload)) {
            previous.destroy();
        }
        self.owns = true;
    }

    pub fn copy(&mut self, id: u64, payload: Payload, connection: &Connection, qh: &QueueHandle<App>, sender: &Sender<Event>) {
        let item = Item { id, payload };
        if self.snapshotting {
            self.pending.retain(|pending| pending.id != id);
            self.pending.push(item);
            return;
        }
        if !self.owns {
            self.history.clear();
            self.baseline = None;
            if let Some(offer) = self.offer.clone() {
                self.snapshotting = true;
                self.pending.push(item);
                snapshot(&offer, self.offered_types(), connection, sender.clone());
                return;
            }
        }
        self.push(item, qh);
    }

    fn push(&mut self, item: Item, qh: &QueueHandle<App>) {
        self.history.retain(|existing| existing.id != item.id);
        let payload = item.payload.clone();
        self.history.push(item);
        if self.history.len() > HISTORY_LIMIT {
            let excess = self.history.len() - HISTORY_LIMIT;
            self.history.drain(..excess);
        }
        self.write(payload, qh);
    }

    pub fn snapshot_ready(&mut self, items: Vec<(String, Bytes)>, qh: &QueueHandle<App>) {
        self.snapshotting = false;
        if !items.is_empty() {
            self.baseline = Some(Payload::Snapshot(Arc::new(items)));
        }
        for item in std::mem::take(&mut self.pending) {
            self.push(item, qh);
        }
    }

    pub fn set_text(&mut self, text: String, qh: &QueueHandle<App>) {
        self.history.clear();
        self.baseline = None;
        self.write(Payload::Text(Arc::new(text)), qh);
        self.owns = false;
    }

    pub fn content_changed(&mut self, id: u64, payload: Payload, qh: &QueueHandle<App>) {
        let Some(item) = self.history.iter_mut().find(|item| item.id == id) else { return };
        item.payload = payload.clone();
        if self.owns && self.current().is_some_and(|current| current.id == id) {
            self.write(payload, qh);
        }
    }

    pub fn discard(&mut self, id: u64, qh: &QueueHandle<App>) {
        let was_current = self.current().is_some_and(|current| current.id == id);
        self.history.retain(|item| item.id != id);
        if !self.owns || !was_current {
            return;
        }
        if let Some(previous) = self.current().map(|item| item.payload.clone()) {
            self.write(previous, qh);
        } else if let Some(baseline) = self.baseline.take() {
            self.write(baseline, qh);
            self.owns = false;
        } else if let Some(device) = &self.device {
            device.set_selection(None);
            if let Some((source, _)) = self.source.take() {
                source.destroy();
            }
            self.owns = false;
        }
    }

    pub fn release(&mut self, id: u64) {
        self.history.retain(|item| item.id != id);
        if self.history.is_empty() {
            self.baseline = None;
        }
    }

    pub fn owner(&self) -> Option<u64> {
        if self.owns { self.current().map(|item| item.id) } else { None }
    }

    pub fn owned_png(&self) -> Option<Bytes> {
        match (&self.source, self.owns) {
            (Some((_, Payload::Png(bytes))), true) => Some(bytes.clone()),
            _ => None,
        }
    }

    pub fn read_image(&self, connection: &Connection, sender: &Sender<Event>) -> bool {
        let Some(offer) = &self.offer else { return false };
        let types = self.offered_types();
        let Some(mime) = ["image/png", "image/jpeg"].into_iter().find(|mime| types.iter().any(|t| t == mime)) else { return false };
        let Ok((reader, writer)) = std::io::pipe() else { return false };
        offer.receive(mime.into(), writer.as_fd());
        drop(writer);
        let _ = connection.flush();
        let sender = sender.clone();
        std::thread::spawn(move || {
            let mut data = Vec::new();
            let _ = reader.take(SNAPSHOT_LIMIT as u64).read_to_end(&mut data);
            let _ = sender.send(Event::ClipboardImage(data));
        });
        true
    }

    fn cancelled(&mut self, source: &ExtDataControlSourceV1) {
        source.destroy();
        if self.source.as_ref().is_some_and(|(current, _)| current == source) {
            self.source = None;
            self.owns = false;
            self.history.clear();
            self.baseline = None;
        }
    }
}

fn snapshot(offer: &ExtDataControlOfferV1, types: Vec<String>, connection: &Connection, sender: Sender<Event>) {
    let mut readers = Vec::new();
    for mime in types {
        let Ok((reader, writer)) = std::io::pipe() else { continue };
        offer.receive(mime.clone(), writer.as_fd());
        drop(writer);
        readers.push((mime, reader));
    }
    let _ = connection.flush();
    std::thread::spawn(move || {
        let (results, receiver) = std::sync::mpsc::channel();
        let count = readers.len();
        for (mime, reader) in readers {
            let results = results.clone();
            std::thread::spawn(move || {
                let mut data = Vec::new();
                let complete = reader.take(SNAPSHOT_LIMIT as u64 + 1).read_to_end(&mut data).is_ok() && data.len() <= SNAPSHOT_LIMIT;
                let _ = results.send(complete.then_some((mime, Arc::new(data))));
            });
        }
        let mut items = Vec::new();
        let mut total = 0;
        for _ in 0..count {
            match receiver.recv_timeout(Duration::from_millis(800)) {
                Ok(Some((mime, data))) if total + data.len() <= SNAPSHOT_LIMIT => {
                    total += data.len();
                    items.push((mime, data));
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        let _ = sender.send(Event::ClipboardSnapshot(items));
    });
}

impl Dispatch2<ExtDataControlManagerV1, App> for ManagerData {
    fn event(&self, _: &mut App, _: &ExtDataControlManagerV1, _: <ExtDataControlManagerV1 as Proxy>::Event, _: &Connection, _: &QueueHandle<App>) {}
}

impl Dispatch2<ExtDataControlDeviceV1, App> for DeviceData {
    fn event(&self, app: &mut App, device: &ExtDataControlDeviceV1, event: ext_data_control_device_v1::Event, _: &Connection, _: &QueueHandle<App>) {
        let clipboard = &mut app.clipboard;
        match event {
            ext_data_control_device_v1::Event::Selection { id } => {
                if let Some(previous) = std::mem::replace(&mut clipboard.offer, id) {
                    previous.destroy();
                }
            }
            ext_data_control_device_v1::Event::PrimarySelection { id: Some(offer) } => offer.destroy(),
            ext_data_control_device_v1::Event::Finished => {
                device.destroy();
                clipboard.device = None;
            }
            _ => {}
        }
    }

    fn event_created_child(opcode: u16, qh: &QueueHandle<App>) -> Arc<dyn wayland_client::backend::ObjectData> {
        match opcode {
            ext_data_control_device_v1::EVT_DATA_OFFER_OPCODE => qh.make_data::<ExtDataControlOfferV1, _>(OfferData::default()),
            _ => panic!("unexpected child of ext_data_control_device_v1: {opcode}"),
        }
    }
}

impl Dispatch2<ExtDataControlOfferV1, App> for OfferData {
    fn event(&self, _: &mut App, _: &ExtDataControlOfferV1, event: ext_data_control_offer_v1::Event, _: &Connection, _: &QueueHandle<App>) {
        if let ext_data_control_offer_v1::Event::Offer { mime_type } = event {
            self.0.lock().unwrap().push(mime_type);
        }
    }
}

impl Dispatch2<ExtDataControlSourceV1, App> for SourceData {
    fn event(&self, app: &mut App, source: &ExtDataControlSourceV1, event: ext_data_control_source_v1::Event, _: &Connection, _: &QueueHandle<App>) {
        match event {
            ext_data_control_source_v1::Event::Send { mime_type, fd } => {
                if let Some((current, payload)) = &app.clipboard.source
                    && current == source {
                        payload.send(&mime_type, fd);
                    }
            }
            ext_data_control_source_v1::Event::Cancelled => {
                app.clipboard.cancelled(source);
                app.sync_clipboard_owner();
            }
            _ => {}
        }
    }
}
