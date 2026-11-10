# ShareK for KDE Plasma

Screenshot, recording and annotation tool for KDE Plasma 6 on Wayland, written in Rust. No cloud or upload features. Budget: the daemon idles at ≤ 6 MB RSS / ≤ 3 MB PSS and 0 % CPU; every other process lives only while it has work or a window.

## Build, install, test

- `scripts/install.sh` runs `cargo build --release --workspace` when `Cargo.toml` is present, installs `sharek`, `sharek-editor`, `sharek-gif` and `sharek-ocr` into `~/.local/bin`, writes the desktop file from `data/` (`@BIN@` becomes the bin dir), installs the master icon (`assets/icon/AppIcon.svg` inside the package, or `../assets/icon/AppIcon.svg` from the repo) as the scalable `local.sharek` icon, refreshes the icon and sycoca caches, fetches the OCR models and restarts the daemon with `kioclient exec` on the desktop file, so it runs in its own `app-local.sharek` cgroup.
- `scripts/uninstall.sh` stops the daemon, drops the kglobalaccel component (`cleanUp`), and removes the binaries, desktop file, icon, autostart entry and cache; settings, history, OCR models and captures stay.
- Releases: pushing a `linux-v<version>` tag runs the repository's `.github/workflows/release.yml` inside `linux/`. It builds `dist/sharek-install.sh` with `scripts/package.sh` (a self-extracting tarball of the binaries, `data/`, the master icon and the scripts; `--uninstall` runs `uninstall.sh`), fails if any binary needs glibc above 2.35 (weak symbols ignored), and attaches it to a GitHub release that the README downloads with `curl` from the public repo. The workflow does not test: `scripts/test.sh` runs here before the commit. It writes `/etc/apt/apt-mirrors.txt` so apt downloads from archive.ubuntu.com and kernel.org: the runner's default Azure mirror sometimes cannot be resolved for hours. The version lives in `Cargo.toml` (workspace) and `Cargo.lock`.
- Only the installed binaries can capture. KWin grants `org.kde.KWin.ScreenShot2`, `zkde_screencast_unstable_v1`, `org_kde_kwin_fake_input` and plasma window management by matching the process to a desktop file `Exec` that lists them in `X-KDE-DBUS-Restricted-Interfaces` / `X-KDE-Wayland-Interfaces`; a binary run from `target/` is refused unless a desktop file names its path.
- `scripts/test.sh`: `cargo test --release --workspace` and `cargo clippy --release --workspace --all-targets -- -D warnings`, which stays clean.
- `scripts/fetch-ocr-models.sh` downloads the OCR models into `~/.local/share/sharek/ocr` from a pinned Hugging Face revision and checks their SHA-256; files already present and valid are kept.
- Runtime tools: `gst-launch-1.0` with pipewiresrc, x264enc, mp4mux, fdkaacenc and pulsesrc; `ffmpeg` / `ffprobe` (video posters, thumbnails, sizes); the system `onnxruntime` (opened with dlopen); `fc-match` and Noto Sans.
- End-to-end checks run in a nested `kwin_wayland --virtual` under `dbus-run-session` with its own `XDG_*` directories (see `migracion.md` in the parent folder). The daemon needs a StatusNotifierWatcher on that bus only to show the tray; it starts without one. Stop nested processes by PID, never with `pkill` by name: that also kills the session's own ShareK.

## Layout

| Path | Binary | Role |
|---|---|---|
| `crates/common` | — | Settings (`~/.config/sharek/settings.json`), XDG paths, history index, editor IPC framing, portal helpers, PNG/JPEG read and write with DPI, image buffers that can live in a file (`mapped`), glyph outlines (ttf-parser), time formatting, swirl geometry for the tray icon, region labels, OCR process call |
| `crates/daemon` | `sharek` | Always running: tray, global shortcuts, D-Bus service `local.ShareK` (`sharek <action>` forwards to the running instance), selection overlay, pins, clipboard, toasts, recording, scrolling capture, editor spawning |
| `crates/editor` | `sharek-editor` | Slint editor: capture tabs, recording tabs, history window |
| `crates/gif` | `sharek-gif` | Streaming GIF encoder: raw BGRx frames on stdin, per-frame median-cut palette, changed-rectangle frames with transparency, LZW |
| `crates/ocr` | `sharek-ocr` | PNG on stdin, recognized text on stdout |
| `data` | | Desktop file template |
| `scripts` | | Install, uninstall, package, test, OCR model download |

Daemon modules: `app` (state and handlers on one calloop loop fed by Wayland and an `Event` channel), `overlay`, `screenshot` (ScreenShot2 pipe reader), `windows` (plasma window list and activation on a short-lived Wayland connection), `pin`, `clipboard`, `toast`, `chrome` (recording border and panel), `record`, `scroll`, `stitch`, `shortcuts`, `service`, `tray`, `editor` (IPC link), `memory`, `wl` (bound globals), `draw`.

Default shortcuts: Print region, Shift+Print screen, Meta+Print active window, Ctrl+Shift+6 MP4, +7 GIF, +8 scrolling capture, +9 pin region, +0 OCR to clipboard, +1 color picker, +2 ruler, +H history. The key that starts a recording or scrolling capture also stops it; other captures are ignored meanwhile.

## Processes and IPC

- The daemon spawns the editor on demand with a socketpair whose fd is in `SHAREK_IPC_FD`. A message is a u32 length, a JSON header and a u64-length payload (`common/src/ipc.rs`).
- The editor offers to exit with `Exiting { received }` after any message that leaves it without a visible window. The daemon answers `ExitAccepted` only when it has sent nothing since; the editor re-offers after every later message, so a message in flight is never lost.
- Clipboard ids: the daemon numbers captures from 1; tabs the editor opens itself (from history) take ids from `1 << 48`. Text copied from the recognized-text window goes to the daemon (`CopyText`), so it outlives the editor process.
- KWin focus-stealing prevention keeps new editor windows behind. The editor sends `Activate { history }` and the daemon sets the plasma window state on the editor's windows whose title does or does not start with `ipc::HISTORY_TITLE`, retrying for up to 5 s while the editor maps its window.
- `gst-launch-1.0`, `sharek-gif`, `sharek-ocr` and `ffmpeg` are short-lived children; recording children carry `PR_SET_PDEATHSIG`.
- A capture is never dropped: with copying, saving and the editor all turned off, it opens in the editor.

## Mechanisms and footguns

Memory
- Both long-lived processes set `M_MMAP_THRESHOLD` to 256 KiB so large buffers are mmapped and handed back on free.
- `memory::release()` (malloc_trim plus `MADV_PAGEOUT` on the executable's read-only segments) runs 5 s after the last handled event and after large frees. Readahead otherwise leaves the whole binary resident.
- The images the editor keeps live in files, not in anonymous memory: the base of every capture and of every undo step (`EditorState::new`), its mip levels (`Mips`) and the cached export (`Document::rendered`). `mapped::Bytes`, the buffer inside `RgbImage`, is a `Vec` until `on_disk()` copies 1 MiB or more into a `MAP_SHARED` mapping of an `O_TMPFILE` file in `~/.cache/sharek`; the file has no name and goes away with the mapping or the process. Images the daemon holds for an instant stay on the heap. The buffer also stays on the heap when that folder is on tmpfs, when its volume lacks the size plus 1 GiB of free space, or when the filesystem has no `O_TMPFILE`.
- Resident pages of those mappings are file pages (`RssFile`, outside `RssAnon`). `mapped::page_out()` runs `MADV_PAGEOUT` over every live mapping whenever the shown tab changes and 2 s after the last redraw, which takes them out of the RSS; the next read faults them back from the page cache. The scene and the frame handed to Slint are window-sized and stay anonymous.
- zbus applies backpressure: a live MessageStream or SignalIterator that nobody polls stops all reception on its connection.

Tray and shortcuts
- The tray spawns with `assume_sni_available`, so the daemon starts before plasmashell's StatusNotifierWatcher and the item registers when it appears. Its tooltip carries the version.
- Shortcuts go through the GlobalShortcuts portal. dbus-broker can disconnect xdg-desktop-portal (UID quota); its restart makes xdg-desktop-portal-kde close every session. xdg-desktop-portal-kde can also crash and be restarted by systemd while xdg-desktop-portal stays up: the frontend keeps the session, but the new backend has none, so no shortcut fires. The shortcuts thread watches both names: when the portal comes back it registers the app id and binds again; when the KDE backend does, it closes the stale session and binds again. `shortcuts::tests` reproduces both on a private dbus-daemon.
- BindShortcuts never changes keys KDE already stores; preferred triggers only seed the first registration. Stored keys change with kglobalaccel `setForeignShortcutKeys`, defaults with `setShortcutKeys` flag 8.
- The tray menu shows the keys KDE stores, read from kglobalaccel `/component/local_sharek` `allShortcutInfos` after binding and on every `yourShortcutsChanged`; `shortcuts::keys` picks the readable alternative (Ctrl+Shift+9 over Ctrl+( and the keypad variant).
- KWin drops Shift when it produced a non-letter keysym, so Ctrl+Shift+6 is matched as Ctrl+^ (layout us(altgr-intl)); digit triggers use keysym names (`CTRL+asciicircum`, …). With NumLock on, Shift does not change keypad keys, so Ctrl+Shift+keypad digits arrive as Ctrl+Shift+6 or Ctrl+Shift+Num+6; those alternatives exist only in the KDE config.
- A process started inside another app's cgroup gets that app's id from the portal. Restarting the daemon while KDE's "register shortcuts" dialog is open registers the actions without keys.

Overlay and pins
- The overlay freezes each output through ScreenShot2 into a shm RawPool (BGR888); dim, outline and crosshair are single-pixel subsurfaces scaled with wp_viewporter, magnifier and labels are tiny-skia painted. The dashed outline strips share one buffer per orientation, attached as a plain `wl_buffer`: SCTK's `Buffer::attach_to` refuses a buffer that is already attached elsewhere. The ruler label sits on the side away from the magnifier.
- A pin is one layer surface whose image lives once in a memfd shared with KWin; zoom is the viewport destination (wheel or pinch through `zwp_pointer_gestures_v1`), opacity wp_alpha_modifier. KWin reports pointer motion relative to the geometry it had when the event was generated, so every move commit is followed by `wl_display.sync` and the drag basis switches only on its `done`; at most one move is in flight. The double-click interval comes from `kdeglobals` `[KDE] DoubleClickInterval`.

Recording
- `zkde_screencast` v3 `stream_region` on the logical rect (video at the output's pixel ratio, GIF at 1×, cursor embedded) gives a PipeWire node for `gst-launch-1.0`. Stop sends SIGINT (`-e` writes EOS); cancel sends SIGKILL and deletes the file; a 15 s SIGKILL fallback follows a stop.
- Nothing may queue between `videorate` and `x264enc`: after a static gap videorate emits its duplicates in one burst, and a leaky queue drops them, which lowers the frame rate and delays the first video frame.
- Video is encoded with x264 on the CPU; nvh264enc loads CUDA into the gst process, hundreds of MB.
- The border is four dashed strips just outside the region (a strip that would fall inside the region at a screen edge is omitted); the panel sits outside the region unless the region fills the output. The panel is sized to its contents and dragged by any spot that is not a button, with the same one-move-in-flight sync as pins.
- GIF frames come from videorate at a constant 15 fps; an unchanged frame lengthens the delay of the frame before it, and delays are rounded on cumulative time so the total length stays exact.

Scrolling capture
- ScreenShot2 `CaptureArea` with `hide-caller-windows`, so the daemon's own panel and pins never appear in the frames. The first frame and every frame after an automatic scroll wait until two captures 60 ms apart are identical (up to 1.5 s), so animations and the window activation after the overlay closes never land in a frame.
- Automatic scrolling uses `org_kde_kwin_fake_input` (authenticated when bound). The pointer moves to the region center once, before the session's guards map (`Scroller::park`); every step after that is one axis event wherever the pointer is. The guards are transparent `Top`-layer surfaces on every output whose input region is the output minus the scroll region (`Chrome::guard`): entering one means the user took the pointer out, so the session goes manual and drops them, and an axis event sent in that instant lands on a guard instead of another window. Moving the pointer on every step would lock the user out of the mouse. Apps turn axis values into very different distances, so the first request is small and each later one is scaled from the distance the last one moved, aiming at 40 % of the region. A step with no overlap is scrolled back and retried four times smaller; after four misses, when the pointer leaves the region, when the panel's Manual button is pressed, or without fake input, the session is manual: frames every 80 ms, the title reads "Desplázate", or "Más despacio" while frames do not overlap. Manual sessions end only with Stop, the shortcut or 30 000 px.
- The stitcher hashes only the columns that changed between the first two different frames (fixed sidebars stay out), ignores the trailing 16 px × scale (scrollbars), votes with 16-row windows, and accepts an offset only when 90 % of the overlapping moving rows agree. Frames whose rows are 97 % identical count as unchanged (a blinking cursor). The fixed footer starts below the last row that moved, and a move that would leave less than one window of overlap above it is rejected, so fixed rows never land in a seam.

Clipboard
- ext_data_control_v1. History of up to 5 captures plus a snapshot of the clipboard taken before the first one, restored by "discard". Recordings are offered as file URIs (plus image/gif for GIFs); several history files as a URI list.

Editor
- Slint 1.18, software renderer on winit Wayland. Set the window size before the first `show()` (it otherwise opens at its minimum size); `slint::set_xdg_app_id` only works once a component exists. The window opens at the capture size plus chrome, at least as wide as the unfolded toolbar and at most 85 % of the smallest monitor.
- softbuffer 0.4 keeps the buffer age across a resize on Wayland, so after a scale change Slint repaints only dirty regions into a buffer with a new stride. A winit `ScaleFactorChanged` filter toggles the `repaint` property, which dirties the whole window.
- The canvas `TouchArea` takes every press, so `ContextMenuArea` never sees a right click: the canvas opens the menu itself after the controller has selected the shape under the pointer.
- Canvas frames: `Scene` holds the base plus the settled shapes (everything below the first shape being dragged, edited or selected, `CaptureTab::live_from`) rendered at the current view. Each frame copies it and draws only the live shapes and the overlay. Shapes are compared by `render::fingerprints` (geometry, style, text, image pointer, step number): new settled shapes are drawn on top of the cache, a change below them rebuilds it from the base, and so does any zoom, scroll or resize.
- The macOS editor has this shadow ported line by line (`SoftShadow.swift`); `the_shadow_gives_the_values_the_macos_editor_is_tested_against` and its Swift twin assert the same numbers, so change both together.
- Shadows are drawn from a half-resolution coverage layer blurred on one channel and composited by hand (`composite_shadow`): a full-resolution RGBA blur costs tens of milliseconds per shape. The highlight fills with `BlendMode::Multiply`, so it tints the shapes beneath it.
- Text editing: the edited shape stays on the canvas; the overlay `TextInput` is invisible (transparent text and selection) and only takes keys and mouse placement. It gets its text only through `begin-text`, called when the edited shape id changes (`edit_shown`); a binding would break after the first edit. Its cursor and anchor byte offsets (Slint's internal `cursor-position-byte-offset` and `anchor-position-byte-offset`) arrive through `caret`, and the canvas draws the caret and selection with the renderer's font. Starting an edit redraws synchronously so the `TextInput` has focus before the next key. Ctrl+Z and Ctrl+Shift+Z stay in the `TextInput`'s own history until `text_history_belongs_to_document` hands them to the document. The canvas takes focus on press only while nothing is being edited.
- Choosing a value rebuilds the options model under the open ComboBox popup, and when the popup closes Slint returns focus to the destroyed item. `after_control` gives focus back to the canvas or the `TextInput` on the next event-loop turn.
- The options pill has a full and a compact `OptionsRow` (no title, no color labels; the color popup shows the label as its title); the compact one shows when the full one does not fit. The pill moves left of the zoom control when centered would touch it, and the zoom control moves above the pill when they do not fit side by side.
- The history window lives in the editor process. Thumbnails are made on one worker thread, cached as PNG in `~/.cache/sharek/thumbnails` keyed by path, mtime, size and pixel size, and kept in memory only for rows near the viewport; `history.json` mtime is polled every second while the window is open. Its size is saved to `~/.config/sharek/history-window` on close.
- One toolbar row. Tool groups fold into a split button (last tool used plus a popup) by rank as the window narrows; `app::thresholds()` computes the fold widths from the same button widths `editor.slint` uses (34 px buttons, 48 px group buttons), so change both together. The toolbar layout has explicit geometry: letting its minimum width reach the window creates a binding loop through `level`.
- The icon paths in `icons.rs` are copied in `macos/Sources/ShareK/Editor/Icons.swift`, and `icons::tests` fails when the two differ: change both together.
- Drawing tool icons are tinted with each tool's saved style (`app::icon_colors`); colors too dark for the toolbar fall back to gray, except outlined text, which shows its outline color. Every other icon takes a fixed color by purpose (`app::action_color` and the tool cases in `icon_colors`).
- `styles.json` stores the sizes as drawn; nothing scales them by image size.
- Rotate, flip and frame rewrite the base image and move the shapes, like crop, so they undo as one step; text shapes keep their size and stay upright around their new center. The frame (`frame.json`) is baked into the RGB base, so its background is opaque.
- The view refits on every resize until the user zooms or scrolls (`CaptureTab::auto_fit`).
- Dragging the Copy button out (`drag.rs`) opens a second wayland-client connection on winit's `wl_display` (foreign display), keeps its own `wl_pointer` to learn the button-press serial, and starts a `wl_data_device` drag from the editor surface offering `image/png` and a `text/uri-list` file in `~/.cache/sharek/drag` (replaced on each drag). Its queue is pumped every 100 ms while the window is visible and dropped on hide, when winit destroys the surface.
- The eyedropper samples the rendered export at the clicked point, shapes included. Saving reads the save folder from `settings.json` at that moment.

OCR
- Models: PP-OCRv5 mobile detection and latin PP-OCRv5 mobile recognition (838 classes: 0 is the CTC blank, k + 1 is `dict[k]`, the last is a space). The detection input is capped at 1280 px on its long side because activations dominate memory; recognition crops come from the full-resolution image.
- `ort` uses `load-dynamic` with the `api-24` feature over the system library; no runtime is downloaded.

Tests
- Tests that need D-Bus start a private `dbus-daemon` with a config that has no `<servicedir>`: the stock session config would activate real services, such as the portals, on the private bus and leave them running.
