mod app;
mod chrome;
mod clipboard;
use sharek_common::portal as dbus;
mod draw;
mod editor;
mod memory;
mod overlay;
mod pin;
mod record;
mod screenshot;
mod scroll;
mod service;
mod shortcuts;
mod stitch;
mod toast;
mod tray;
mod windows;
mod wl;

use zbus::fdo::{RequestNameFlags, RequestNameReply};

fn main() {
    unsafe {
        libc::mallopt(libc::M_MMAP_THRESHOLD, 256 << 10);
    }
    let requested = std::env::args().nth(1).map(|argument| argument.trim_start_matches('-').to_string());
    let connection = match zbus::blocking::Connection::session() {
        Ok(connection) => connection,
        Err(error) => {
            eprintln!("sharek: no session bus: {error}");
            std::process::exit(1);
        }
    };
    let primary = matches!(
        connection.request_name_with_flags(service::NAME, RequestNameFlags::DoNotQueue.into()),
        Ok(RequestNameReply::PrimaryOwner)
    );
    if !primary {
        if let Some(action) = &requested
            && !service::forward(&connection, action) {
                eprintln!("sharek: unknown action {action}");
                std::process::exit(2);
            }
        return;
    }
    let registered = dbus::register_app(&connection);
    let initial = requested.as_deref().and_then(app::Action::named);
    app::run(connection, registered, initial);
}
