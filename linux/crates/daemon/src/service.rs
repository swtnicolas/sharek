use crate::app::{Action, Event};
use smithay_client_toolkit::reexports::calloop::channel::Sender;

pub const NAME: &str = "local.ShareK";
pub const PATH: &str = "/local/ShareK";

struct Service {
    sender: Sender<Event>,
}

#[zbus::interface(name = "local.ShareK")]
impl Service {
    fn run(&self, action: &str) -> bool {
        match Action::named(action) {
            Some(action) => self.sender.send(Event::Action(action, false)).is_ok(),
            None => false,
        }
    }
}

pub fn serve(connection: &zbus::blocking::Connection, sender: Sender<Event>) {
    let _ = connection.object_server().at(PATH, Service { sender });
}

pub fn forward(connection: &zbus::blocking::Connection, action: &str) -> bool {
    connection
        .call_method(Some(NAME), PATH, Some(NAME), "Run", &(action,))
        .ok()
        .and_then(|reply| reply.body().deserialize::<bool>().ok())
        .unwrap_or(false)
}
