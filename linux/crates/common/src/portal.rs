use crate::paths::APP_ID;
use std::collections::HashMap;
use std::os::fd::AsFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{Fd, OwnedValue, Value};

pub const PORTAL: &str = "org.freedesktop.portal.Desktop";
pub const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";

pub type Results = HashMap<String, OwnedValue>;
pub type Options<'a> = HashMap<&'a str, Value<'a>>;

static TOKENS: AtomicU32 = AtomicU32::new(0);

pub fn token() -> String {
    format!("sharek{}_{}", std::process::id(), TOKENS.fetch_add(1, Ordering::Relaxed))
}

pub fn register_app(connection: &Connection) -> Result<(), String> {
    connection
        .call_method(Some(PORTAL), PORTAL_PATH, Some("org.freedesktop.host.portal.Registry"), "Register", &(APP_ID, Options::new()))
        .map(|_| ())
        .map_err(|error| error.to_string())
}

pub fn request<B>(connection: &Connection, interface: &str, method: &str, token: &str, body: &B) -> zbus::Result<Option<Results>>
where
    B: serde::Serialize + zbus::zvariant::DynamicType,
{
    let sender = connection.unique_name().ok_or_else(|| zbus::Error::Failure("no unique name".into()))?;
    let sender = sender.trim_start_matches(':').replace('.', "_");
    let path = format!("/org/freedesktop/portal/desktop/request/{sender}/{token}");
    let proxy = Proxy::new(connection, PORTAL, path.as_str(), "org.freedesktop.portal.Request")?;
    let mut responses = proxy.receive_signal("Response")?;
    connection.call_method(Some(PORTAL), PORTAL_PATH, Some(interface), method, body)?;
    let message = responses.next().ok_or_else(|| zbus::Error::Failure("portal closed the request".into()))?;
    let (code, results): (u32, Results) = message.body().deserialize()?;
    Ok((code == 0).then_some(results))
}

pub fn notify(connection: &Connection, summary: &str, body: &str) {
    let _ = connection.call_method(
        Some("org.freedesktop.Notifications"),
        "/org/freedesktop/Notifications",
        Some("org.freedesktop.Notifications"),
        "Notify",
        &("ShareK", 0u32, APP_ID, summary, body, Vec::<&str>::new(), Options::new(), -1i32),
    );
}

pub fn file_uri(path: &Path) -> String {
    let mut uri = String::from("file://");
    for byte in path.as_os_str().as_encoded_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => uri.push(*byte as char),
            _ => uri.push_str(&format!("%{byte:02X}")),
        }
    }
    uri
}

pub fn path_from_uri(uri: &str) -> Option<PathBuf> {
    let encoded = uri.strip_prefix("file://")?.as_bytes();
    let mut bytes = Vec::with_capacity(encoded.len());
    let mut index = 0;
    while index < encoded.len() {
        if encoded[index] == b'%' && index + 2 < encoded.len() {
            let hex = std::str::from_utf8(&encoded[index + 1..index + 3]).ok()?;
            bytes.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            bytes.push(encoded[index]);
            index += 1;
        }
    }
    Some(PathBuf::from(String::from_utf8(bytes).ok()?))
}

pub fn open_uri(connection: &Connection, uri: &str) {
    let _ = connection.call_method(
        Some(PORTAL),
        PORTAL_PATH,
        Some("org.freedesktop.portal.OpenURI"),
        "OpenURI",
        &("", uri, Options::new()),
    );
}

pub fn trash(connection: &Connection, path: &Path) -> bool {
    let Ok(file) = std::fs::File::open(path) else { return false };
    connection
        .call_method(Some(PORTAL), PORTAL_PATH, Some("org.freedesktop.portal.Trash"), "TrashFile", &(Fd::from(file.as_fd()),))
        .ok()
        .and_then(|reply| reply.body().deserialize::<u32>().ok())
        == Some(1)
}

pub fn choose_folder(connection: &Connection, title: &str, current: &Path) -> Option<PathBuf> {
    let token = token();
    let mut folder = current.as_os_str().as_encoded_bytes().to_vec();
    folder.push(0);
    let mut options = Options::new();
    options.insert("handle_token", Value::from(token.as_str()));
    options.insert("directory", Value::from(true));
    options.insert("current_folder", Value::from(folder));
    options.insert("accept_label", Value::from("Elegir"));
    let results = request(connection, "org.freedesktop.portal.FileChooser", "OpenFile", &token, &("", title, options)).ok()??;
    let uris: Vec<String> = results.get("uris")?.try_clone().ok()?.try_into().ok()?;
    path_from_uri(uris.first()?)
}

pub fn save_file(connection: &Connection, title: &str, directory: &Path, name: &str, types: &[(&str, &str)]) -> Option<PathBuf> {
    let token = token();
    let mut folder = directory.as_os_str().as_encoded_bytes().to_vec();
    folder.push(0);
    let filters: Vec<(&str, Vec<(u32, &str)>)> = types.iter().map(|(label, pattern)| (*label, vec![(0u32, *pattern)])).collect();
    let mut options = Options::new();
    options.insert("handle_token", Value::from(token.as_str()));
    options.insert("current_name", Value::from(name));
    options.insert("current_folder", Value::from(folder));
    options.insert("filters", Value::from(filters));
    let results = request(connection, "org.freedesktop.portal.FileChooser", "SaveFile", &token, &("", title, options)).ok()??;
    let uris: Vec<String> = results.get("uris")?.try_clone().ok()?.try_into().ok()?;
    path_from_uri(uris.first()?)
}

pub fn show_items(connection: &Connection, paths: &[PathBuf]) {
    let uris: Vec<String> = paths.iter().map(|path| file_uri(path)).collect();
    let _ = connection.call_method(
        Some("org.freedesktop.FileManager1"),
        "/org/freedesktop/FileManager1",
        Some("org.freedesktop.FileManager1"),
        "ShowItems",
        &(uris, ""),
    );
}
