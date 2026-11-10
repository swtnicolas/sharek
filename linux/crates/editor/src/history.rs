use crate::app::{try_with, with};
use crate::thumbnail::{self, Thumbnail};
use crate::{HistoryCell, HistoryMenuItem, HistoryRow, HistoryWindow};
use sharek_common::history::{self, Entry, MediaKind};
use sharek_common::ipc::HISTORY_TITLE;
use sharek_common::text::Font;
use sharek_common::{portal, time};
use slint::{ComponentHandle, Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, VecModel};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, SystemTime};

const THUMBNAIL: (f64, f64) = (160.0, 108.0);
const NAME_WIDTH: f32 = 164.0;
const NAME_SIZE: f32 = 11.0;
const ROW_HEIGHT: f64 = 164.0;
const KEEP_SCREENS: usize = 2;
const KEY_RETURN: &str = "\n";
const KEY_BACKSPACE: &str = "\u{8}";
const KEY_DELETE: &str = "\u{7f}";

pub enum Command {
    Open(Vec<Entry>),
    Copy(Vec<Entry>),
    Pin(Vec<Entry>),
}

pub fn selection_after_press(selected: &[bool], anchor: Option<usize>, index: usize, ctrl: bool, shift: bool, right: bool) -> (Vec<bool>, Option<usize>) {
    let mut next = selected.to_vec();
    if right {
        if !selected[index] {
            next.iter_mut().for_each(|value| *value = false);
            next[index] = true;
            return (next, Some(index));
        }
        return (next, anchor);
    }
    if ctrl {
        next[index] = !next[index];
        return (next, Some(index));
    }
    next.iter_mut().for_each(|value| *value = false);
    match anchor.filter(|_| shift) {
        Some(anchor) => {
            let (low, high) = (anchor.min(index), anchor.max(index));
            next[low..=high].iter_mut().for_each(|value| *value = true);
            (next, Some(anchor))
        }
        None => {
            next[index] = true;
            (next, Some(index))
        }
    }
}

pub fn elide_middle(text: &str, fits: impl Fn(&str) -> bool) -> String {
    if fits(text) {
        return text.to_string();
    }
    let characters: Vec<char> = text.chars().collect();
    let candidate = |keep: usize| {
        let head = keep.div_ceil(2);
        let tail = keep / 2;
        let mut elided: String = characters[..head].iter().collect();
        elided.push('…');
        elided.extend(&characters[characters.len() - tail..]);
        elided
    };
    (0..characters.len()).rev().map(candidate).find(|elided| fits(elided)).unwrap_or_else(|| "…".into())
}

pub fn visible_rows(scroll: f64, height: f64, rows: usize) -> (usize, usize) {
    if rows == 0 {
        return (0, 0);
    }
    let first = (scroll.max(0.0) / ROW_HEIGHT).floor() as usize;
    let last = ((scroll.max(0.0) + height.max(ROW_HEIGHT)) / ROW_HEIGHT).ceil() as usize;
    (first.min(rows - 1), last.min(rows - 1))
}

struct Queue {
    wanted: VecDeque<(PathBuf, (u32, u32))>,
    closed: bool,
}

struct Loader(Arc<(Mutex<Queue>, Condvar)>);

impl Loader {
    fn start(keep: Vec<(PathBuf, (u32, u32))>) -> Loader {
        let shared = Arc::new((Mutex::new(Queue { wanted: VecDeque::new(), closed: false }), Condvar::new()));
        let worker = shared.clone();
        let _ = std::thread::Builder::new().name("thumbnails".into()).spawn(move || {
            let keys: HashSet<String> = keep.iter().filter_map(|(path, size)| thumbnail::key(path, *size)).collect();
            thumbnail::prune(&keys);
            loop {
                let next = {
                    let (lock, ready) = &*worker;
                    let Ok(mut queue) = lock.lock() else { return };
                    loop {
                        if queue.closed {
                            return;
                        }
                        if let Some(next) = queue.wanted.pop_front() {
                            break next;
                        }
                        let Ok(waited) = ready.wait(queue) else { return };
                        queue = waited;
                    }
                };
                let (path, size) = next;
                let made = thumbnail::load(&path, size);
                unsafe {
                    libc::malloc_trim(0);
                }
                if let Some(made) = made {
                    let _ = slint::invoke_from_event_loop(move || with(move |c| c.history_thumbnail(path, made)));
                }
            }
        });
        Loader(shared)
    }

    fn want(&self, items: Vec<(PathBuf, (u32, u32))>) {
        let (lock, ready) = &*self.0;
        if let Ok(mut queue) = lock.lock() {
            queue.wanted = items.into();
            ready.notify_one();
        }
    }
}

impl Drop for Loader {
    fn drop(&mut self) {
        let (lock, ready) = &*self.0;
        if let Ok(mut queue) = lock.lock() {
            queue.closed = true;
            queue.wanted.clear();
            ready.notify_one();
        }
    }
}

pub struct HistoryView {
    window: HistoryWindow,
    entries: Vec<Entry>,
    selected: Vec<bool>,
    anchor: Option<usize>,
    columns: usize,
    rows: Vec<Rc<VecModel<HistoryCell>>>,
    thumbnails: HashMap<PathBuf, Image>,
    scroll: (f64, f64),
    loader: Loader,
    _watch: slint::Timer,
    modified: Option<SystemTime>,
    trashing: Vec<PathBuf>,
    font: Option<Font>,
}

fn kind(entry: &Entry) -> &'static str {
    match entry.media() {
        Some(MediaKind::Video) => "video",
        Some(MediaKind::Gif) => "gif",
        None => "",
    }
}

fn plural(count: usize) -> String {
    if count == 1 { "1 archivo".into() } else { format!("{count} archivos") }
}

impl HistoryView {
    pub fn new() -> Option<HistoryView> {
        let window = HistoryWindow::new().ok()?;
        bind(&window);
        let entries = history::prune_missing();
        let size = Self::pixel_size(&window);
        let loader = Loader::start(entries.iter().map(|entry| (entry.path.clone(), size)).collect());
        let watch = slint::Timer::default();
        watch.start(slint::TimerMode::Repeated, Duration::from_secs(1), || with(|c| c.history_watch()));
        let mut view = HistoryView {
            columns: window.get_columns().max(1) as usize,
            window,
            selected: vec![false; entries.len()],
            entries,
            anchor: None,
            rows: Vec::new(),
            thumbnails: HashMap::new(),
            scroll: (0.0, 0.0),
            loader,
            _watch: watch,
            modified: history::modified(),
            trashing: Vec::new(),
            font: Font::ui(),
        };
        view.rebuild();
        Some(view)
    }

    fn pixel_size(window: &HistoryWindow) -> (u32, u32) {
        let scale = f64::from(window.window().scale_factor()).max(1.0);
        ((THUMBNAIL.0 * scale).round() as u32, (THUMBNAIL.1 * scale).round() as u32)
    }

    fn size_file() -> PathBuf {
        sharek_common::paths::config_dir().join("history-window")
    }

    pub fn show(&mut self) {
        if !self.is_visible() {
            let saved = std::fs::read_to_string(Self::size_file()).ok();
            let size = saved.as_deref().and_then(|text| text.split_once(' ')).and_then(|(width, height)| Some((width.trim().parse::<f32>().ok()?, height.trim().parse::<f32>().ok()?)));
            if let Some((width, height)) = size {
                self.window.window().set_size(slint::LogicalSize::new(width, height));
            }
        }
        let _ = self.window.show();
        self.window.invoke_focus_list();
        self.request();
    }

    pub fn is_visible(&self) -> bool {
        self.window.window().is_visible()
    }

    pub fn changed_on_disk(&mut self) -> bool {
        let modified = history::modified();
        if modified == self.modified {
            return false;
        }
        self.modified = modified;
        true
    }

    pub fn reload(&mut self) {
        let chosen: HashSet<PathBuf> = self.selected_entries().into_iter().map(|entry| entry.path).collect();
        self.entries = history::load();
        self.modified = history::modified();
        self.selected = self.entries.iter().map(|entry| chosen.contains(&entry.path)).collect();
        self.anchor = None;
        let paths: HashSet<&PathBuf> = self.entries.iter().map(|entry| &entry.path).collect();
        self.thumbnails.retain(|path, _| paths.contains(path));
        self.rebuild();
        self.request();
    }

    fn cell(&self, index: usize) -> HistoryCell {
        let entry = &self.entries[index];
        HistoryCell {
            index: index as i32,
            name: self.display_name(entry).into(),
            date: time::short_date(entry.seconds()).into(),
            kind: kind(entry).into(),
            thumbnail: self.thumbnails.get(&entry.path).cloned().unwrap_or_default(),
            selected: self.selected[index],
        }
    }

    fn display_name(&self, entry: &Entry) -> String {
        let name = entry.path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        match &self.font {
            Some(font) => elide_middle(&name, |text| font.measure(text, NAME_SIZE) <= NAME_WIDTH),
            None => name,
        }
    }

    fn rebuild(&mut self) {
        let columns = self.columns.max(1);
        self.rows = (0..self.entries.len()).step_by(columns).map(|start| Rc::new(VecModel::from((start..(start + columns).min(self.entries.len())).map(|index| self.cell(index)).collect::<Vec<_>>()))).collect();
        let rows: Vec<HistoryRow> = self.rows.iter().map(|row| HistoryRow { cells: ModelRc::from(row.clone()) }).collect();
        self.window.set_rows(ModelRc::new(VecModel::from(rows)));
        self.window.set_empty(self.entries.is_empty());
        let title = if self.entries.is_empty() { HISTORY_TITLE.to_string() } else { format!("{HISTORY_TITLE} — {}", plural(self.entries.len())) };
        self.window.set_window_title(title.into());
    }

    fn refresh(&self, index: usize) {
        let columns = self.columns.max(1);
        if let Some(row) = self.rows.get(index / columns) {
            row.set_row_data(index % columns, self.cell(index));
        }
    }

    pub fn set_columns(&mut self, columns: usize) {
        if columns.max(1) != self.columns {
            self.columns = columns.max(1);
            self.rebuild();
            self.request();
        }
    }

    pub fn scrolled(&mut self, scroll: f64, height: f64) {
        self.scroll = (scroll, height);
        self.request();
    }

    fn request(&mut self) {
        let columns = self.columns.max(1);
        let (first, last) = visible_rows(self.scroll.0, self.scroll.1, self.rows.len());
        let screen = last - first + 1;
        let keep = (first.saturating_sub(KEEP_SCREENS * screen) * columns, ((last + KEEP_SCREENS * screen + 1) * columns).min(self.entries.len()));
        let evicted: Vec<usize> = (0..self.entries.len()).filter(|index| (*index < keep.0 || *index >= keep.1) && self.thumbnails.contains_key(&self.entries[*index].path)).collect();
        for index in evicted {
            self.thumbnails.remove(&self.entries[index].path);
            self.refresh(index);
        }
        let size = Self::pixel_size(&self.window);
        let prefetch = (first.saturating_sub(screen) * columns, ((last + screen + 1) * columns).min(self.entries.len()));
        let visible = (first * columns)..((last + 1) * columns).min(self.entries.len());
        let wanted: Vec<(PathBuf, (u32, u32))> = visible
            .chain(prefetch.0..prefetch.1)
            .filter(|index| !self.thumbnails.contains_key(&self.entries[*index].path))
            .map(|index| (self.entries[index].path.clone(), size))
            .collect();
        self.loader.want(wanted);
    }

    pub fn thumbnail_ready(&mut self, path: PathBuf, made: Thumbnail) {
        let Some(index) = self.entries.iter().position(|entry| entry.path == path) else { return };
        let buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&made.rgba, made.width, made.height);
        self.thumbnails.insert(path, Image::from_rgba8(buffer));
        self.refresh(index);
    }

    fn select(&mut self, selected: Vec<bool>, anchor: Option<usize>) {
        let changed: Vec<usize> = (0..selected.len()).filter(|&index| selected[index] != self.selected[index]).collect();
        self.selected = selected;
        self.anchor = anchor;
        for index in changed {
            self.refresh(index);
        }
    }

    pub fn press(&mut self, index: usize, ctrl: bool, shift: bool, right: bool) {
        if index >= self.entries.len() {
            return;
        }
        let (selected, anchor) = selection_after_press(&self.selected, self.anchor, index, ctrl, shift, right);
        self.select(selected, anchor);
        if right {
            self.update_menu();
        }
    }

    pub fn clear_selection(&mut self) {
        self.select(vec![false; self.entries.len()], None);
    }

    fn selected_entries(&self) -> Vec<Entry> {
        self.entries.iter().zip(&self.selected).filter(|(_, selected)| **selected).map(|(entry, _)| entry.clone()).collect()
    }

    fn update_menu(&self) {
        let selection = self.selected_entries();
        let images = selection.iter().filter(|entry| entry.media().is_none()).count();
        let open = if selection.len() == 1 && images == 1 { "Abrir en el editor" } else { "Abrir" };
        let mut items = vec![("open", open), ("copy", "Copiar")];
        if images > 0 {
            items.push(("pin", "Fijar en pantalla"));
        }
        items.extend([("reveal", "Mostrar en la carpeta"), ("", ""), ("remove", "Quitar del historial"), ("trash", "Mover a la papelera")]);
        let height = 10.0 + items.iter().map(|(name, _)| if name.is_empty() { 9.0 } else { 26.0 }).sum::<f32>();
        let menu: Vec<HistoryMenuItem> = items.into_iter().map(|(name, title)| HistoryMenuItem { name: name.into(), title: title.into(), separator: name.is_empty() }).collect();
        self.window.set_menu_items(ModelRc::new(VecModel::from(menu)));
        self.window.set_menu_height(height);
    }

    pub fn activated(&mut self, index: usize) -> Option<Command> {
        let entry = self.entries.get(index)?.clone();
        self.select(self.entries.iter().enumerate().map(|(position, _)| position == index).collect(), Some(index));
        Some(Command::Open(vec![entry]))
    }

    pub fn menu_action(&mut self, name: &str) -> Option<Command> {
        let selection = self.selected_entries();
        if selection.is_empty() {
            return None;
        }
        match name {
            "open" => Some(Command::Open(selection)),
            "copy" => Some(Command::Copy(selection)),
            "pin" => Some(Command::Pin(selection.into_iter().filter(|entry| entry.media().is_none()).collect())),
            "reveal" => {
                let paths: Vec<PathBuf> = selection.into_iter().map(|entry| entry.path).collect();
                std::thread::spawn(move || {
                    if let Ok(connection) = zbus::blocking::Connection::session() {
                        portal::show_items(&connection, &paths);
                    }
                });
                None
            }
            "remove" => {
                history::remove(&selection.into_iter().map(|entry| entry.path).collect::<Vec<_>>());
                self.reload();
                None
            }
            "trash" => {
                self.ask_trash(selection);
                None
            }
            _ => None,
        }
    }

    fn ask_trash(&mut self, selection: Vec<Entry>) {
        let title = match selection.as_slice() {
            [single] => format!("¿Mover {} a la papelera?", single.path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()),
            many => format!("¿Mover {} archivos a la papelera?", many.len()),
        };
        self.trashing = selection.into_iter().map(|entry| entry.path).collect();
        self.window.set_confirm_title(title.into());
        self.window.set_confirming(true);
    }

    pub fn confirmed(&mut self, accepted: bool) {
        self.window.set_confirming(false);
        let paths = std::mem::take(&mut self.trashing);
        self.window.invoke_focus_list();
        if !accepted || paths.is_empty() {
            return;
        }
        std::thread::spawn(move || {
            if let Ok(connection) = zbus::blocking::Connection::session() {
                let trashed: Vec<PathBuf> = paths.into_iter().filter(|path| portal::trash(&connection, path)).collect();
                history::remove(&trashed);
            }
            let _ = slint::invoke_from_event_loop(|| with(|c| c.history_reload()));
        });
    }

    pub fn key(&mut self, text: &str, ctrl: bool) -> (bool, Option<Command>) {
        match text {
            KEY_RETURN => (true, self.menu_action("open")),
            KEY_DELETE | KEY_BACKSPACE => (true, self.menu_action(if ctrl { "trash" } else { "remove" })),
            "a" | "A" if ctrl => {
                self.select(vec![true; self.entries.len()], self.anchor);
                (true, None)
            }
            "c" | "C" if ctrl => (true, self.menu_action("copy")),
            _ => (false, None),
        }
    }
}

fn bind(window: &HistoryWindow) {
    window.on_columns_changed(|columns| with(move |c| c.history_input(move |history| {
        history.set_columns(columns.max(1) as usize);
        None
    })));
    window.on_scrolled(|scroll, height| with(move |c| c.history_input(move |history| {
        history.scrolled(f64::from(scroll), f64::from(height));
        None
    })));
    window.on_pressed(|index, ctrl, shift, right| {
        let _ = try_with(move |c| c.history_input(move |history| {
            history.press(index.max(0) as usize, ctrl, shift, right);
            None
        }));
    });
    window.on_background_pressed(|| with(|c| c.history_input(|history| {
        history.clear_selection();
        None
    })));
    window.on_activated(|index| with(move |c| c.history_input(move |history| history.activated(index.max(0) as usize))));
    window.on_menu_action(|name| with(move |c| c.history_input(move |history| history.menu_action(&name))));
    window.on_confirmed(|accepted| with(move |c| c.history_input(move |history| {
        history.confirmed(accepted);
        None
    })));
    window.on_key(|text, ctrl, _shift| try_with(move |c| c.history_key(&text, ctrl)).unwrap_or(false));
    let weak = window.as_weak();
    window.window().on_close_requested(move || {
        if let Some(window) = weak.upgrade() {
            let size = window.window().size().to_logical(window.window().scale_factor());
            let _ = std::fs::create_dir_all(sharek_common::paths::config_dir());
            let _ = std::fs::write(HistoryView::size_file(), format!("{} {}", size.width.round(), size.height.round()));
        }
        slint::Timer::single_shot(Duration::ZERO, || with(|c| c.history_closed()));
        slint::CloseRequestResponse::HideWindow
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: [bool; 5] = [false; 5];

    #[test]
    fn a_plain_press_selects_only_that_item() {
        let (selected, anchor) = selection_after_press(&[true, false, true, false, false], Some(0), 3, false, false, false);
        assert_eq!(selected, vec![false, false, false, true, false]);
        assert_eq!(anchor, Some(3));
    }

    #[test]
    fn ctrl_toggles_an_item_and_keeps_the_rest() {
        let (selected, _) = selection_after_press(&[true, false, false, false, false], Some(0), 2, true, false, false);
        assert_eq!(selected, vec![true, false, true, false, false]);
        let (selected, _) = selection_after_press(&selected, Some(2), 0, true, false, false);
        assert_eq!(selected, vec![false, false, true, false, false]);
    }

    #[test]
    fn shift_selects_the_range_from_the_anchor_in_either_direction() {
        assert_eq!(selection_after_press(&NONE, Some(1), 3, false, true, false).0, vec![false, true, true, true, false]);
        assert_eq!(selection_after_press(&NONE, Some(3), 0, false, true, false).0, vec![true, true, true, true, false]);
    }

    #[test]
    fn a_right_press_keeps_a_selection_that_contains_the_item() {
        let current = [true, true, false, false, false];
        assert_eq!(selection_after_press(&current, Some(0), 1, false, false, true).0, current.to_vec());
        assert_eq!(selection_after_press(&current, Some(0), 4, false, false, true).0, vec![false, false, false, false, true]);
    }

    #[test]
    fn long_names_keep_their_start_and_their_extension() {
        let fits = |text: &str| text.chars().count() <= 12;
        assert_eq!(elide_middle("ShareK_2026-11-06.png", fits), "ShareK…6.png");
        assert_eq!(elide_middle("short.png", fits), "short.png");
    }

    #[test]
    fn visible_rows_cover_the_viewport() {
        assert_eq!(visible_rows(0.0, 400.0, 10), (0, 3));
        assert_eq!(visible_rows(400.0, 400.0, 10), (2, 5));
        assert_eq!(visible_rows(0.0, 400.0, 2), (0, 1));
    }
}
