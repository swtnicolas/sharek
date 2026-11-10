use crate::app::Event;
use crate::chrome::Chrome;
use crate::screenshot::{self, Shot, Target};
use crate::stitch::{Outcome, Stitcher};
use crate::windows::Rect;
use sharek_common::image::RgbImage;
use smithay_client_toolkit::reexports::calloop::channel::Sender;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use wayland_client::Connection;
use wayland_protocols_plasma::fake_input::client::org_kde_kwin_fake_input::OrgKdeKwinFakeInput;

const MAXIMUM_AUTOMATIC_STEPS: usize = 400;
const MAXIMUM_HEIGHT: usize = 30_000;
const VERTICAL: u32 = 0;
const FIRST_REQUEST: f64 = 6.0;
const TARGET_SHARE: f64 = 0.4;
const MAXIMUM_MISSES: usize = 4;
const SETTLE_POLL: Duration = Duration::from_millis(60);
const SETTLE_LIMIT: Duration = Duration::from_millis(1500);
const MANUAL_INTERVAL: Duration = Duration::from_millis(80);

pub type Finished = Result<Option<(RgbImage, f64)>, String>;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Automatic,
    Manual,
    TooFast,
}

impl Status {
    pub fn title(self) -> &'static str {
        match self {
            Status::Automatic => "Desplazamiento",
            Status::Manual => "Desplázate",
            Status::TooFast => "Más despacio",
        }
    }

    pub fn offers_manual(self) -> bool {
        self == Status::Automatic
    }
}

pub struct Scroller {
    pub input: OrgKdeKwinFakeInput,
    pub connection: Connection,
}

impl Scroller {
    pub fn park(&self, region: Rect) {
        self.input.pointer_motion_absolute(region.x + region.width / 2.0, region.y + region.height / 2.0);
        let _ = self.connection.flush();
    }

    fn scroll(&self, amount: f64) {
        self.input.axis(VERTICAL, amount);
        let _ = self.connection.flush();
    }
}

#[derive(Default)]
struct Control {
    stop: AtomicBool,
    cancel: AtomicBool,
    manual: AtomicBool,
}

impl Control {
    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
}

pub struct Scrolling {
    pub generation: u64,
    pub chrome: Chrome,
    control: Arc<Control>,
}

impl Scrolling {
    pub fn start(generation: u64, region: Rect, chrome: Chrome, scroller: Option<Scroller>, dbus: zbus::blocking::Connection, sender: Sender<Event>) -> Scrolling {
        let control = Arc::new(Control::default());
        let worker = control.clone();
        std::thread::spawn(move || {
            let notify = |status: Status| {
                let _ = sender.send(Event::ScrollStatus(generation, status));
            };
            let result = run(region, scroller.as_ref(), &dbus, &worker, &notify);
            let _ = sender.send(Event::ScrollFinished(generation, result));
        });
        Scrolling { generation, chrome, control }
    }

    pub fn go_manual(&mut self) {
        self.control.manual.store(true, Ordering::Relaxed);
        self.chrome.release_guards();
        self.chrome.set_title(Status::Manual.title(), false);
    }

    pub fn show(&mut self, status: Status) {
        if status != Status::Automatic {
            self.chrome.release_guards();
        }
        self.chrome.set_title(status.title(), status.offers_manual());
    }

    pub fn stop(&mut self) {
        self.control.stop.store(true, Ordering::Relaxed);
        self.chrome.show_processing();
    }

    pub fn cancel(&mut self) {
        self.control.cancel.store(true, Ordering::Relaxed);
        self.control.stop.store(true, Ordering::Relaxed);
        self.chrome.show_processing();
    }
}

fn settled(dbus: &zbus::blocking::Connection, target: Target, control: &Control) -> Result<Option<Shot>, String> {
    let started = Instant::now();
    let mut last = screenshot::capture(dbus, target, false)?;
    while started.elapsed() < SETTLE_LIMIT && !control.cancelled() {
        std::thread::sleep(SETTLE_POLL);
        let shot = screenshot::capture(dbus, target, false)?;
        if shot.image.data == last.image.data {
            return Ok(Some(shot));
        }
        last = shot;
    }
    Ok((!control.cancelled()).then_some(last))
}

fn run(region: Rect, scroller: Option<&Scroller>, dbus: &zbus::blocking::Connection, control: &Control, notify: &dyn Fn(Status)) -> Finished {
    let target = Target::Area { x: region.x as i32, y: region.y as i32, width: region.width as u32, height: region.height as u32 };
    let Some(first) = settled(dbus, target, control)? else { return Ok(None) };
    let scale = first.scale;
    let mut stitcher = Stitcher::new((16.0 * scale).round() as usize);
    stitcher.add(first.image);
    let mut status = if scroller.is_some() { Status::Automatic } else { Status::Manual };
    let mut show = |next: Status, only_from: Option<Status>| {
        if status != next && only_from.is_none_or(|from| status == from) {
            status = next;
            notify(next);
        }
    };
    let mut request = FIRST_REQUEST;
    let mut direction = 1.0;
    let mut direction_checked = false;
    let mut progressed = false;
    let (mut unchanged, mut misses, mut steps) = (0, 0, 0);
    loop {
        if control.cancelled() {
            return Ok(None);
        }
        if control.stopped() || stitcher.height() >= MAXIMUM_HEIGHT {
            break;
        }
        let automatic = scroller.filter(|_| !control.manual.load(Ordering::Relaxed) && misses <= MAXIMUM_MISSES);
        let Some(scroller) = automatic else {
            show(Status::Manual, Some(Status::Automatic));
            std::thread::sleep(MANUAL_INTERVAL);
            let shot = screenshot::capture(dbus, target, false)?;
            match stitcher.add(shot.image) {
                Outcome::NoMatch => show(Status::TooFast, None),
                Outcome::Reversed => {}
                _ => show(Status::Manual, None),
            }
            continue;
        };
        steps += 1;
        if steps > MAXIMUM_AUTOMATIC_STEPS {
            break;
        }
        scroller.scroll(direction * request);
        let Some(shot) = settled(dbus, target, control)? else { return Ok(None) };
        match stitcher.add(shot.image) {
            Outcome::Appended(moved) => {
                progressed = true;
                direction_checked = true;
                unchanged = 0;
                misses = 0;
                let moved = moved as f64 / scale;
                request = (request * region.height * TARGET_SHARE / moved.max(1.0)).clamp(1.0, 4000.0);
            }
            Outcome::Unchanged if progressed => {
                unchanged += 1;
                if unchanged >= 2 {
                    break;
                }
            }
            Outcome::Unchanged => {
                request *= 4.0;
                misses += 1;
            }
            Outcome::Reversed if !direction_checked => {
                direction = -direction;
                direction_checked = true;
                scroller.scroll(direction * request);
                let Some(back) = settled(dbus, target, control)? else { return Ok(None) };
                stitcher.add(back.image);
            }
            Outcome::Reversed | Outcome::NoMatch => {
                scroller.scroll(-direction * request);
                let Some(back) = settled(dbus, target, control)? else { return Ok(None) };
                stitcher.add(back.image);
                request = (request / 4.0).max(1.0);
                misses += 1;
            }
            Outcome::Started => {}
        }
    }
    if control.cancelled() {
        return Ok(None);
    }
    Ok(stitcher.compose().map(|image| (image, scale)))
}
