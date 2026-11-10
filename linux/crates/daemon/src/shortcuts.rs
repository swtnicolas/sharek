use crate::app::{Action, Event};
use crate::dbus::{self, Options, PORTAL, PORTAL_PATH};
use futures_lite::{StreamExt, future};
use std::collections::HashMap;
use smithay_client_toolkit::reexports::calloop::channel::Sender;
use zbus::blocking::{Connection, Proxy};
use zbus::Message;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, Value};

const INTERFACE: &str = "org.freedesktop.portal.GlobalShortcuts";
const BACKEND: &str = "org.freedesktop.impl.portal.desktop.kde";
const KGLOBALACCEL: &str = "org.kde.kglobalaccel";
const COMPONENT_PATH: &str = "/component/local_sharek";
const COMPONENT_INTERFACE: &str = "org.kde.kglobalaccel.Component";

pub struct Shortcut {
    pub id: &'static str,
    pub description: &'static str,
    pub trigger: &'static str,
    pub action: Action,
}

pub const SHORTCUTS: &[Shortcut] = &[
    Shortcut { id: "capture-region", description: "Capturar región", trigger: "Print", action: Action::CaptureRegion },
    Shortcut { id: "capture-screen", description: "Capturar pantalla completa", trigger: "SHIFT+Print", action: Action::CaptureScreen },
    Shortcut { id: "capture-window", description: "Capturar ventana activa", trigger: "LOGO+Print", action: Action::CaptureWindow },
    Shortcut { id: "record-video", description: "Grabar video (MP4)", trigger: "CTRL+asciicircum", action: Action::RecordVideo },
    Shortcut { id: "record-gif", description: "Grabar GIF", trigger: "CTRL+ampersand", action: Action::RecordGif },
    Shortcut { id: "scrolling-capture", description: "Captura con desplazamiento", trigger: "CTRL+asterisk", action: Action::ScrollingCapture },
    Shortcut { id: "pin-region", description: "Fijar región en pantalla", trigger: "CTRL+parenleft", action: Action::PinRegion },
    Shortcut { id: "recognize-text", description: "Capturar texto (OCR)", trigger: "CTRL+parenright", action: Action::RecognizeText },
    Shortcut { id: "pick-color", description: "Selector de color", trigger: "CTRL+exclam", action: Action::PickColor },
    Shortcut { id: "measure", description: "Regla", trigger: "CTRL+at", action: Action::Measure },
    Shortcut { id: "history", description: "Historial", trigger: "CTRL+SHIFT+h", action: Action::ShowHistory },
];

const CONTROL: i32 = 0x0400_0000;
const SHIFT: i32 = 0x0200_0000;
const ALT: i32 = 0x0800_0000;
const META: i32 = 0x1000_0000;
const KEYPAD: i32 = 0x2000_0000;
const KEY: i32 = 0x01ff_ffff;

fn key_name(key: i32) -> Option<String> {
    match key {
        0x0100_0009 => Some("Print".into()),
        0x21..=0x7e => char::from_u32(key as u32).map(|key| key.to_ascii_uppercase().to_string()),
        _ => None,
    }
}

pub fn keys(codes: &[i32]) -> Vec<String> {
    let readable = |code: &&i32| *code & KEYPAD == 0 && char::from_u32((*code & KEY) as u32).is_some_and(|key| key.is_ascii_alphanumeric());
    let Some(code) = codes.iter().find(readable).or(codes.first()).copied() else { return Vec::new() };
    let Some(key) = key_name(code & KEY) else { return Vec::new() };
    [(CONTROL, "Control"), (SHIFT, "Shift"), (ALT, "Alt"), (META, "Super")]
        .into_iter()
        .filter(|(bit, _)| code & bit != 0)
        .map(|(_, name)| name.to_string())
        .chain([key])
        .collect()
}

pub fn bound(connection: &Connection) -> HashMap<String, Vec<String>> {
    let reply = connection.call_method(Some(KGLOBALACCEL), COMPONENT_PATH, Some(COMPONENT_INTERFACE), "allShortcutInfos", &());
    let Ok(infos) = reply.and_then(|reply| reply.body().deserialize::<Vec<(String, String, String, String, String, String, Vec<i32>, Vec<i32>)>>()) else {
        return HashMap::new();
    };
    infos.into_iter().map(|(id, _, _, _, _, _, codes, _)| (id, keys(&codes))).collect()
}

pub fn spawn(connection: Connection, sender: Sender<Event>) {
    std::thread::Builder::new()
        .name("shortcuts".into())
        .spawn(move || {
            if let Err(error) = run(&connection, &sender) {
                let _ = sender.send(Event::Failure("No se pudieron registrar los atajos globales".into(), error.to_string()));
            }
        })
        .expect("spawn shortcuts thread");
}

enum Signal {
    Activated(Message),
    PortalOwner(bool),
    BackendOwner(bool),
    KeysChanged,
}

fn run(connection: &Connection, sender: &Sender<Event>) -> zbus::Result<()> {
    let proxy = Proxy::new(connection, PORTAL, PORTAL_PATH, INTERFACE)?;
    future::block_on(async {
        let activations = proxy.inner().receive_signal("Activated").await?.map(Signal::Activated);
        let owners = proxy.inner().receive_owner_changed().await?.map(|owner| Signal::PortalOwner(owner.is_some()));
        let backend = zbus::fdo::DBusProxy::new(connection.inner()).await?;
        let backends = backend
            .receive_name_owner_changed_with_args(&[(0, BACKEND)])
            .await?
            .filter_map(|signal| signal.args().ok().map(|args| Signal::BackendOwner(args.new_owner().is_some())));
        let accel = zbus::Proxy::new(connection.inner(), KGLOBALACCEL, "/kglobalaccel", "org.kde.KGlobalAccel").await?;
        let changes = accel.receive_signal("yourShortcutsChanged").await?.map(|_| Signal::KeysChanged);
        let mut signals = activations.or(owners).or(backends).or(changes);
        let mut session = Some(bind(connection)?);
        let _ = sender.send(Event::ShortcutsChanged);
        while let Some(signal) = signals.next().await {
            match signal {
                Signal::Activated(message) => {
                    let Ok((handle, id, _, _)) = message.body().deserialize::<(OwnedObjectPath, String, u64, dbus::Results)>() else { continue };
                    if session.as_ref() != Some(&handle) {
                        continue;
                    }
                    if let Some(shortcut) = SHORTCUTS.iter().find(|shortcut| shortcut.id == id) {
                        let _ = sender.send(Event::Action(shortcut.action, false));
                    }
                }
                Signal::PortalOwner(present) => {
                    let _ = dbus::register_app(connection);
                    session = if present { Some(bind(connection)?) } else { None };
                    let _ = sender.send(Event::ShortcutsChanged);
                }
                Signal::BackendOwner(present) => {
                    if let Some(stale) = session.take() {
                        let _ = connection.call_method(Some(PORTAL), stale.as_str(), Some("org.freedesktop.portal.Session"), "Close", &());
                    }
                    session = if present { Some(bind(connection)?) } else { None };
                    let _ = sender.send(Event::ShortcutsChanged);
                }
                Signal::KeysChanged => {
                    let _ = sender.send(Event::ShortcutsChanged);
                }
            }
        }
        Ok(())
    })
}

fn bind(connection: &Connection) -> zbus::Result<OwnedObjectPath> {
    let token = dbus::token();
    let session_token = dbus::token();
    let mut options = Options::new();
    options.insert("handle_token", Value::from(token.as_str()));
    options.insert("session_handle_token", Value::from(session_token.as_str()));
    let results = dbus::request(connection, INTERFACE, "CreateSession", &token, &(options,))?
        .ok_or_else(|| zbus::Error::Failure("el portal rechazó la sesión".into()))?;
    let session: String = results
        .get("session_handle")
        .and_then(|value| value.try_clone().ok())
        .and_then(|value| String::try_from(value).ok())
        .ok_or_else(|| zbus::Error::Failure("el portal no devolvió la sesión".into()))?;
    let session = OwnedObjectPath::try_from(session)?;

    let shortcuts: Vec<(&str, Options)> = SHORTCUTS
        .iter()
        .map(|shortcut| {
            let mut properties = Options::new();
            properties.insert("description", Value::from(shortcut.description));
            properties.insert("preferred_trigger", Value::from(shortcut.trigger));
            (shortcut.id, properties)
        })
        .collect();
    let token = dbus::token();
    let mut options = Options::new();
    options.insert("handle_token", Value::from(token.as_str()));
    let path: ObjectPath = session.as_ref();
    dbus::request(connection, INTERFACE, "BindShortcuts", &token, &(path, shortcuts, "", options))?;
    Ok(session)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_keys_read_as_the_keys_the_user_presses() {
        assert_eq!(keys(&[67_108_904, 100_663_353, 637_534_265]), ["Control", "Shift", "9"]);
        assert_eq!(keys(&[50_331_657]), ["Shift", "Print"]);
        assert_eq!(keys(&[285_212_681]), ["Super", "Print"]);
        assert_eq!(keys(&[100_663_368]), ["Control", "Shift", "H"]);
        assert!(keys(&[]).is_empty());
    }
    use crate::app::Action;
    use smithay_client_toolkit::reexports::calloop::channel::{self, Channel};
    use std::collections::HashMap;
    use std::io::{BufRead, BufReader};
    use std::process::{Child, Command, Stdio};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};
    use zbus::message::Header;
    use zbus::zvariant::OwnedValue;

    const CONFIG: &str = r#"<busconfig><type>session</type><listen>unix:tmpdir=/tmp</listen><policy context="default"><allow send_destination="*" eavesdrop="true"/><allow eavesdrop="true"/><allow own="*"/></policy></busconfig>"#;

    struct Bus {
        daemon: Child,
        address: String,
    }

    impl Bus {
        fn start() -> Bus {
            let config = std::env::temp_dir().join(format!("sharek-test-bus-{}.conf", std::process::id()));
            std::fs::write(&config, CONFIG).unwrap();
            let mut daemon = Command::new("dbus-daemon")
                .arg(format!("--config-file={}", config.display()))
                .args(["--nofork", "--print-address"])
                .stdout(Stdio::piped())
                .spawn()
                .expect("dbus-daemon");
            let mut address = String::new();
            BufReader::new(daemon.stdout.take().unwrap()).read_line(&mut address).unwrap();
            let _ = std::fs::remove_file(config);
            Bus { daemon, address: address.trim().to_string() }
        }

        fn connect(&self) -> Connection {
            zbus::blocking::connection::Builder::address(self.address.as_str()).unwrap().build().unwrap()
        }

        fn portal(&self, calls: &Arc<Mutex<Calls>>) -> Connection {
            zbus::blocking::connection::Builder::address(self.address.as_str())
                .unwrap()
                .name(PORTAL)
                .unwrap()
                .serve_at(PORTAL_PATH, Registry(calls.clone()))
                .unwrap()
                .serve_at(PORTAL_PATH, GlobalShortcuts(calls.clone()))
                .unwrap()
                .build()
                .unwrap()
        }
    }

    impl Drop for Bus {
        fn drop(&mut self) {
            let _ = self.daemon.kill();
            let _ = self.daemon.wait();
        }
    }

    #[derive(Default)]
    struct Calls {
        log: Vec<&'static str>,
        bound: Vec<OwnedObjectPath>,
        closed: Vec<OwnedObjectPath>,
    }

    struct Session(Arc<Mutex<Calls>>, OwnedObjectPath);

    #[zbus::interface(name = "org.freedesktop.portal.Session")]
    impl Session {
        fn close(&self) {
            let mut calls = self.0.lock().unwrap();
            calls.log.push("Close");
            calls.closed.push(self.1.clone());
        }
    }

    struct Registry(Arc<Mutex<Calls>>);

    #[zbus::interface(name = "org.freedesktop.host.portal.Registry")]
    impl Registry {
        fn register(&self, _app_id: &str, _options: HashMap<String, OwnedValue>) {
            self.0.lock().unwrap().log.push("Register");
        }
    }

    struct GlobalShortcuts(Arc<Mutex<Calls>>);

    #[zbus::interface(name = "org.freedesktop.portal.GlobalShortcuts")]
    impl GlobalShortcuts {
        async fn create_session(
            &self,
            options: HashMap<String, OwnedValue>,
            #[zbus(header)] header: Header<'_>,
            #[zbus(connection)] connection: &zbus::Connection,
        ) -> OwnedObjectPath {
            self.0.lock().unwrap().log.push("CreateSession");
            let session = format!("/org/freedesktop/portal/desktop/session/{}/{}", caller(&header), text(&options, "session_handle_token"));
            let handle = OwnedObjectPath::try_from(session.as_str()).unwrap();
            connection.object_server().at(session.as_str(), Session(self.0.clone(), handle)).await.unwrap();
            respond(connection, &header, &options, HashMap::from([("session_handle", Value::from(session.as_str()))])).await
        }

        async fn bind_shortcuts(
            &self,
            session: OwnedObjectPath,
            _shortcuts: Vec<(String, HashMap<String, OwnedValue>)>,
            _parent_window: &str,
            options: HashMap<String, OwnedValue>,
            #[zbus(header)] header: Header<'_>,
            #[zbus(connection)] connection: &zbus::Connection,
        ) -> OwnedObjectPath {
            {
                let mut calls = self.0.lock().unwrap();
                calls.log.push("BindShortcuts");
                calls.bound.push(session);
            }
            respond(connection, &header, &options, HashMap::new()).await
        }
    }

    fn caller(header: &Header<'_>) -> String {
        header.sender().unwrap().trim_start_matches(':').replace('.', "_")
    }

    fn text(options: &HashMap<String, OwnedValue>, key: &str) -> String {
        String::try_from(options[key].try_clone().unwrap()).unwrap()
    }

    async fn respond(connection: &zbus::Connection, header: &Header<'_>, options: &HashMap<String, OwnedValue>, results: HashMap<&str, Value<'_>>) -> OwnedObjectPath {
        let request = format!("/org/freedesktop/portal/desktop/request/{}/{}", caller(header), text(options, "handle_token"));
        connection.emit_signal(None::<&str>, request.as_str(), "org.freedesktop.portal.Request", "Response", &(0u32, results)).await.unwrap();
        OwnedObjectPath::try_from(request).unwrap()
    }

    fn wait_until(mut done: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !done() {
            if Instant::now() > deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        true
    }

    fn next_action(events: &Channel<Event>) -> Option<Action> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match events.try_recv() {
                Ok(Event::Action(action, _)) => return Some(action),
                Ok(_) => {}
                Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
                Err(_) => return None,
            }
        }
    }

    fn activate(portal: &Connection, session: &OwnedObjectPath, id: &str) {
        portal.emit_signal(None::<&str>, PORTAL_PATH, INTERFACE, "Activated", &(session, id, 0u64, Options::new())).unwrap();
    }

    #[test]
    fn shortcuts_are_registered_and_bound_again_when_the_portal_restarts() {
        let bus = Bus::start();
        let calls = Arc::new(Mutex::new(Calls::default()));
        let first = bus.portal(&calls);
        let (sender, events) = channel::channel();
        spawn(bus.connect(), sender);
        assert!(wait_until(|| calls.lock().unwrap().bound.len() == 1));

        let probe = bus.connect();
        let names = zbus::blocking::fdo::DBusProxy::new(&probe).unwrap();
        first.close().unwrap();
        assert!(wait_until(|| !names.name_has_owner(PORTAL.try_into().unwrap()).unwrap()));
        let second = bus.portal(&calls);
        assert!(wait_until(|| calls.lock().unwrap().bound.len() == 2));
        assert_eq!(calls.lock().unwrap().log, ["CreateSession", "BindShortcuts", "Register", "CreateSession", "BindShortcuts"]);

        let (stale, current) = {
            let calls = calls.lock().unwrap();
            (calls.bound[0].clone(), calls.bound[1].clone())
        };
        activate(&second, &stale, "capture-region");
        activate(&second, &current, "pin-region");
        assert_eq!(next_action(&events), Some(Action::PinRegion));
    }

    #[test]
    fn shortcuts_are_bound_again_when_the_kde_backend_restarts_behind_the_portal() {
        let bus = Bus::start();
        let calls = Arc::new(Mutex::new(Calls::default()));
        let portal = bus.portal(&calls);
        let backend = zbus::blocking::connection::Builder::address(bus.address.as_str()).unwrap().name(BACKEND).unwrap().build().unwrap();
        let (sender, events) = channel::channel();
        spawn(bus.connect(), sender);
        assert!(wait_until(|| calls.lock().unwrap().bound.len() == 1));

        backend.close().unwrap();
        let _restarted = zbus::blocking::connection::Builder::address(bus.address.as_str()).unwrap().name(BACKEND).unwrap().build().unwrap();
        assert!(wait_until(|| calls.lock().unwrap().bound.len() == 2));
        assert_eq!(calls.lock().unwrap().log, ["CreateSession", "BindShortcuts", "Close", "CreateSession", "BindShortcuts"]);

        let (stale, current) = {
            let calls = calls.lock().unwrap();
            assert_eq!(calls.closed, [calls.bound[0].clone()]);
            (calls.bound[0].clone(), calls.bound[1].clone())
        };
        activate(&portal, &stale, "capture-region");
        activate(&portal, &current, "capture-screen");
        assert_eq!(next_action(&events), Some(Action::CaptureScreen));
    }
}
