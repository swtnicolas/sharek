#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
if [ -f Cargo.toml ]; then
    cargo build --release --workspace
fi
bin="${XDG_BIN_HOME:-$HOME/.local/bin}"
data="${XDG_DATA_HOME:-$HOME/.local/share}"
desktop="$data/applications/local.sharek.desktop"
for program in sharek sharek-editor sharek-gif sharek-ocr; do
    install -D -m755 "target/release/$program" "$bin/$program.new" && mv -f "$bin/$program.new" "$bin/$program"
done
install -d "$data/applications"
sed "s|@BIN@|$bin|g" data/local.sharek.desktop > "$desktop"
icon=assets/icon/AppIcon.svg
[ -f "$icon" ] || icon=../assets/icon/AppIcon.svg
install -D -m644 "$icon" "$data/icons/hicolor/scalable/apps/local.sharek.svg"
touch "$data/icons/hicolor"
gtk-update-icon-cache -q -t "$data/icons/hicolor" 2>/dev/null || true
update-desktop-database -q "$data/applications" 2>/dev/null || true
kbuildsycoca6 --noincremental >/dev/null 2>&1 || true
scripts/fetch-ocr-models.sh || echo "No se pudieron descargar los modelos de OCR; vuelve a instalar para reintentar." >&2
if busctl --user status local.ShareK >/dev/null 2>&1; then
    "$bin/sharek" quit
    for _ in 1 2 3 4 5 6 7 8 9 10; do
        busctl --user status local.ShareK >/dev/null 2>&1 || break
        sleep 0.5
    done
fi
kioclient exec "$desktop" >/dev/null 2>&1 || gtk-launch local.sharek
