#!/bin/sh
set -u
bin="${XDG_BIN_HOME:-$HOME/.local/bin}"
data="${XDG_DATA_HOME:-$HOME/.local/share}"
config="${XDG_CONFIG_HOME:-$HOME/.config}"
cache="${XDG_CACHE_HOME:-$HOME/.cache}"
if busctl --user status local.ShareK >/dev/null 2>&1; then
    "$bin/sharek" quit
    for _ in 1 2 3 4 5 6 7 8 9 10; do
        busctl --user status local.ShareK >/dev/null 2>&1 || break
        sleep 0.5
    done
fi
busctl --user call org.kde.kglobalaccel /component/local_sharek org.kde.kglobalaccel.Component cleanUp >/dev/null 2>&1
rm -f "$bin/sharek" "$bin/sharek-editor" "$bin/sharek-gif" "$bin/sharek-ocr" \
    "$data/applications/local.sharek.desktop" \
    "$config/autostart/local.sharek.desktop" \
    "$data/icons/hicolor/scalable/apps/local.sharek.svg"
rm -rf "$cache/sharek"
gtk-update-icon-cache -q -t "$data/icons/hicolor" 2>/dev/null
update-desktop-database -q "$data/applications" 2>/dev/null
kbuildsycoca6 --noincremental >/dev/null 2>&1
echo "ShareK desinstalado. Ajustes, historial y modelos de OCR siguen en $config/sharek y $data/sharek."
