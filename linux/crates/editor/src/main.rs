mod app;
mod canvas;
mod document;
mod drag;
mod geometry;
mod history;
mod icons;
mod media;
mod model;
mod ocr;
mod render;
mod thumbnail;
mod view;

slint::include_modules!();

use sharek_common::ipc::{self, ToEditor};
use std::os::fd::FromRawFd;
use std::os::unix::net::UnixStream;

fn main() {
    unsafe {
        libc::mallopt(libc::M_MMAP_THRESHOLD, 256 << 10);
    }
    let Some(fd) = std::env::var(ipc::FD_VARIABLE).ok().and_then(|value| value.parse::<i32>().ok()) else {
        eprintln!("sharek-editor: started by sharek only");
        std::process::exit(2);
    };
    let stream = unsafe { UnixStream::from_raw_fd(fd) };
    let mut reader = stream.try_clone().expect("duplicate IPC socket");
    app::start(app::Link::new(stream));
    std::thread::Builder::new()
        .name("ipc".into())
        .spawn(move || {
            loop {
                match ipc::receive::<_, ToEditor>(&mut reader) {
                    Ok((message, payload)) => {
                        let _ = slint::invoke_from_event_loop(move || app::with(move |c| c.handle(message, payload)));
                    }
                    Err(_) => {
                        let _ = slint::invoke_from_event_loop(|| {
                            let _ = slint::quit_event_loop();
                        });
                        return;
                    }
                }
            }
        })
        .expect("spawn IPC reader");
    let _ = slint::run_event_loop_until_quit();
}
