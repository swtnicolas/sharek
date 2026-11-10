use crate::app::Event;
use sharek_common::image::RgbImage;
use sharek_common::ipc::{self, ToDaemon, ToEditor};
use smithay_client_toolkit::reexports::calloop::channel::Sender;
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command};
use std::sync::Arc;
use std::sync::mpsc;

pub enum Payload {
    None,
    Bytes(Arc<Vec<u8>>),
    Image(Arc<RgbImage>),
}

pub struct EditorLink {
    writer: mpsc::Sender<(ToEditor, Payload)>,
    child: Option<Child>,
    pub sent: u64,
}

impl EditorLink {
    pub fn spawn(events: Sender<Event>) -> Result<EditorLink, String> {
        let program = std::env::current_exe().map_err(|error| error.to_string())?.with_file_name("sharek-editor");
        let (daemon_end, editor_end) = UnixStream::pair().map_err(|error| error.to_string())?;
        let fd = editor_end.as_raw_fd();
        let mut command = Command::new(&program);
        command.env(ipc::FD_VARIABLE, "3");
        unsafe {
            command.pre_exec(move || {
                let result = if fd == 3 { libc::fcntl(3, libc::F_SETFD, 0) } else { libc::dup2(fd, 3) };
                if result < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command.spawn().map_err(|error| format!("{}: {error}", program.display()))?;
        drop(editor_end);
        let mut reader = daemon_end.try_clone().map_err(|error| error.to_string())?;
        std::thread::Builder::new()
            .name("editor-reader".into())
            .spawn(move || {
                while let Ok((message, payload)) = ipc::receive::<_, ToDaemon>(&mut reader) {
                    if events.send(Event::Editor(message, payload)).is_err() {
                        return;
                    }
                }
                let _ = events.send(Event::EditorGone);
            })
            .map_err(|error| error.to_string())?;
        let (writer, queue) = mpsc::channel::<(ToEditor, Payload)>();
        let mut stream = daemon_end;
        std::thread::Builder::new()
            .name("editor-writer".into())
            .spawn(move || {
                for (message, payload) in queue {
                    let bytes: &[u8] = match &payload {
                        Payload::None => &[],
                        Payload::Bytes(bytes) => bytes,
                        Payload::Image(image) => &image.data,
                    };
                    if ipc::send(&mut stream, &message, bytes).is_err() {
                        return;
                    }
                }
            })
            .map_err(|error| error.to_string())?;
        Ok(EditorLink { writer, child: Some(child), sent: 0 })
    }

    pub fn pid(&self) -> Option<u32> {
        self.child.as_ref().map(Child::id)
    }

    pub fn send(&mut self, message: ToEditor, payload: Payload) {
        self.sent += 1;
        let _ = self.writer.send((message, payload));
    }

}

impl Drop for EditorLink {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }
}
