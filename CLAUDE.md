# sharek

ShareK, a screenshot, recording and annotation tool, as two apps that share no code:

- `linux/` — Rust workspace for KDE Plasma 6 on Wayland: tray daemon, Slint editor, GIF encoder and OCR. Builds, installs and runs its tests on this machine.
- `macos/` — menu bar app in Swift and AppKit. Builds only on the Mac that runs it: Gatekeeper blocks a downloaded build that Apple has not notarized, so it has no tag or release.

Each folder's `CLAUDE.md` covers that platform's engineering, and its scripts resolve paths from their own folder. Everything shared lives once at the root:

- `docs/decisiones.md` (Spanish) — the decisions for both apps. Shared parts are written once; a difference sits in the same section as "Solo en Linux" or "Solo en macOS" with its reason. Every newly approved decision goes there.
- `docs/images/` — the README screenshots.
- `assets/icon/AppIcon.svg` — the master icon. Linux installs it; macOS renders `macos/assets/icon/` from it.
- `README.md` (Spanish) — install and build for both.
- `.github/workflows/release.yml` — runs on `linux-v<version>` tags inside `linux/`, builds and publishes the installer; tests run locally before the commit.
- `.github/workflows/macos.yml` — on every push that touches `macos/`, a GitHub Mac builds the app, runs its tests and measures the memory of the editor and of the idle menu bar process (`macos/scripts/memory-check.py`); it publishes no build. Run by hand, it takes a patch to apply first, so a change is built and measured before it is committed.

The display name is ShareK and the id is `local.sharek` on both platforms (D-Bus `local.ShareK` on Linux). Both apps behave, read and are named the same, with the editor's look taken from Linux; a difference is allowed only where the platform forces it. When the two disagree, the more correct version wins and is carried to the other platform. Compare the code, not only the docs, before calling them aligned.

The repo is public: no employer, work tool, machine identifier or personal data in files, screenshots or sample data.
