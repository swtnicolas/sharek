use std::collections::HashMap;
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols_plasma::plasma_window_management::client::{
    org_kde_plasma_stacking_order::{self, OrgKdePlasmaStackingOrder},
    org_kde_plasma_window::{self, OrgKdePlasmaWindow},
    org_kde_plasma_window_management::{self, OrgKdePlasmaWindowManagement},
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.width && y < self.y + self.height
    }

    pub fn intersection(&self, other: &Rect) -> Option<Rect> {
        let left = self.x.max(other.x);
        let top = self.y.max(other.y);
        let right = (self.x + self.width).min(other.x + other.width);
        let bottom = (self.y + self.height).min(other.y + other.height);
        (right > left && bottom > top).then_some(Rect { x: left, y: top, width: right - left, height: bottom - top })
    }

    pub fn integral(&self) -> Rect {
        let (left, top) = (self.x.floor(), self.y.floor());
        Rect { x: left, y: top, width: (self.x + self.width).ceil() - left, height: (self.y + self.height).ceil() - top }
    }
}

#[derive(Default)]
struct Info {
    proxy: Option<OrgKdePlasmaWindow>,
    geometry: Option<Rect>,
    state: u32,
    app_id: String,
    title: String,
    desktops: Vec<String>,
    pid: u32,
}

#[derive(Default)]
struct Collector {
    uuids: Vec<String>,
    windows: HashMap<String, Info>,
    order: Vec<String>,
}

const ACTIVE: u32 = 0x1;
const MINIMIZED: u32 = 0x2;
const ON_ALL_DESKTOPS: u32 = 0x40;

fn current_desktop(dbus: &zbus::blocking::Connection) -> Option<String> {
    let proxy = zbus::blocking::Proxy::new(dbus, "org.kde.KWin", "/VirtualDesktopManager", "org.kde.KWin.VirtualDesktopManager").ok()?;
    proxy.get_property::<String>("current").ok()
}

pub fn visible(dbus: &zbus::blocking::Connection, outputs: &[Rect]) -> Vec<Rect> {
    let Ok(connection) = Connection::connect_to_env() else { return Vec::new() };
    let Ok((globals, mut queue)) = registry_queue_init::<Collector>(&connection) else { return Vec::new() };
    let qh = queue.handle();
    let Ok(manager) = globals.bind::<OrgKdePlasmaWindowManagement, _, _>(&qh, 17..=18, ()) else { return Vec::new() };
    let mut collector = Collector::default();
    if queue.roundtrip(&mut collector).is_err() {
        return Vec::new();
    }
    for uuid in collector.uuids.clone() {
        let proxy = manager.get_window_by_uuid(uuid.clone(), &qh, uuid.clone());
        collector.windows.entry(uuid).or_default().proxy = Some(proxy);
    }
    manager.get_stacking_order(&qh, ());
    let _ = queue.roundtrip(&mut collector);
    let desktop = current_desktop(dbus);
    let mut rects = Vec::new();
    for uuid in collector.order.iter().rev() {
        let Some(info) = collector.windows.get(uuid) else { continue };
        let Some(geometry) = info.geometry else { continue };
        let on_desktop = info.state & ON_ALL_DESKTOPS != 0
            || info.desktops.is_empty()
            || desktop.as_ref().is_none_or(|current| info.desktops.contains(current));
        let desktop_background = info.app_id == "org.kde.plasmashell" && outputs.contains(&geometry);
        if info.state & MINIMIZED == 0 && on_desktop && !desktop_background && geometry.width > 4.0 && geometry.height > 4.0 {
            rects.push(geometry);
        }
    }
    rects
}

pub fn activate(pid: u32, history: bool) -> bool {
    let Ok(connection) = Connection::connect_to_env() else { return false };
    let Ok((globals, mut queue)) = registry_queue_init::<Collector>(&connection) else { return false };
    let qh = queue.handle();
    let Ok(manager) = globals.bind::<OrgKdePlasmaWindowManagement, _, _>(&qh, 17..=18, ()) else { return false };
    let mut collector = Collector::default();
    if queue.roundtrip(&mut collector).is_err() {
        return false;
    }
    for uuid in collector.uuids.clone() {
        let proxy = manager.get_window_by_uuid(uuid.clone(), &qh, uuid.clone());
        collector.windows.entry(uuid).or_default().proxy = Some(proxy);
    }
    if queue.roundtrip(&mut collector).is_err() {
        return false;
    }
    let mut activated = false;
    for info in collector.windows.values().filter(|info| info.pid == pid && info.title.starts_with(sharek_common::ipc::HISTORY_TITLE) == history) {
        if let Some(proxy) = &info.proxy {
            let state = if info.state & MINIMIZED != 0 { ACTIVE | MINIMIZED } else { ACTIVE };
            proxy.set_state(state, ACTIVE);
            activated = true;
        }
    }
    let _ = queue.roundtrip(&mut collector);
    activated
}

impl Dispatch<WlRegistry, GlobalListContents> for Collector {
    fn event(_: &mut Self, _: &WlRegistry, _: <WlRegistry as wayland_client::Proxy>::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<OrgKdePlasmaWindowManagement, ()> for Collector {
    fn event(collector: &mut Self, _: &OrgKdePlasmaWindowManagement, event: org_kde_plasma_window_management::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let org_kde_plasma_window_management::Event::WindowWithUuid { uuid, .. } = event {
            collector.uuids.push(uuid);
        }
    }
}

impl Dispatch<OrgKdePlasmaStackingOrder, ()> for Collector {
    fn event(collector: &mut Self, _: &OrgKdePlasmaStackingOrder, event: org_kde_plasma_stacking_order::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let org_kde_plasma_stacking_order::Event::Window { uuid } = event {
            collector.order.push(uuid);
        }
    }
}

impl Dispatch<OrgKdePlasmaWindow, String> for Collector {
    fn event(collector: &mut Self, _: &OrgKdePlasmaWindow, event: org_kde_plasma_window::Event, uuid: &String, _: &Connection, _: &QueueHandle<Self>) {
        let info = collector.windows.entry(uuid.clone()).or_default();
        match event {
            org_kde_plasma_window::Event::Geometry { x, y, width, height } => {
                info.geometry = Some(Rect { x: f64::from(x), y: f64::from(y), width: f64::from(width), height: f64::from(height) });
            }
            org_kde_plasma_window::Event::StateChanged { flags } => info.state = flags,
            org_kde_plasma_window::Event::AppIdChanged { app_id } => info.app_id = app_id,
            org_kde_plasma_window::Event::TitleChanged { title } => info.title = title,
            org_kde_plasma_window::Event::VirtualDesktopEntered { id } => info.desktops.push(id),
            org_kde_plasma_window::Event::VirtualDesktopLeft { is } => info.desktops.retain(|desktop| *desktop != is),
            org_kde_plasma_window::Event::PidChanged { pid } => info.pid = pid,
            _ => {}
        }
    }
}

