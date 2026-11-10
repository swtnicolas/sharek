#!/bin/bash
set -uo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
app="$HOME/Applications/ShareK.app"
out="/tmp/sharek-memory"
mode="${1:-none}"
steps="1-una 2-tres 3-cuatro 4-cambio 5-dibujo 6-minimizada 7-cerradas"

editor() {
    pgrep -f 'ShareK --editor$' | head -1
}

rows() {
    grep -E 'Footprint:|phys_footprint| MB | GB '
}

sampler() {
    local pid
    while :; do
        pid="$(editor)"
        if [ -n "$pid" ]; then
            echo "@@ $(date +%T) $(cat "$out/step")"
            footprint "$pid" 2>/dev/null | rows
        fi
        sleep 0.2
    done >> "$out/samples.log"
}

windows() {
    cat > "$out/windows.swift" <<'EOF'
import CoreGraphics
import Foundation

let pid = Int(CommandLine.arguments.dropFirst().first ?? "") ?? 0
let list = CGWindowListCopyWindowInfo([.optionAll], kCGNullWindowID) as? [[String: Any]] ?? []
for window in list where (window[kCGWindowOwnerPID as String] as? Int) == pid {
    let bounds = window[kCGWindowBounds as String] as? [String: Any] ?? [:]
    let width = bounds["Width"] ?? 0
    let height = bounds["Height"] ?? 0
    let onscreen = window[kCGWindowIsOnscreen as String] ?? 0
    let layer = window[kCGWindowLayer as String] ?? 0
    let memory = window[kCGWindowMemoryUsage as String] ?? 0
    print("ventana \(width)x\(height) en pantalla \(onscreen) capa \(layer) memoria \(memory)")
}
EOF
    swift "$out/windows.swift" "$1"
}

snapshot() {
    local name="$1" detail="$2" pid
    pid="$(editor)"
    if [ -z "$pid" ]; then
        echo "El editor no está abierto." | tee "$out/$name.footprint"
        return
    fi
    footprint "$pid" > "$out/$name.footprint" 2>&1
    vmmap "$pid" > "$out/$name.vmmap" 2>&1
    heap "$pid" -s > "$out/$name.heap" 2>&1
    if [ "$mode" != none ]; then
        malloc_history "$pid" -allBySize > "$out/$name.stacks" 2>&1
    fi
    if [ "$detail" = full ]; then
        windows "$pid" > "$out/$name.windows" 2>&1
        leaks --outputGraph="$out/$name.memgraph" "$pid" > "$out/$name.leaks" 2>&1
    fi
}

step() {
    local name="$1" detail="$2" text="$3" reply
    echo "$name" > "$out/step"
    printf '\n[%s] %s\n' "$name" "$text"
    read -r -p "Cuando termines, pulsa Intro. " reply
    echo "Midiendo. No toques ShareK."
    sleep 8
    snapshot "$name" "$detail"
}

timed() {
    local name="$1" detail="$2" text="$3" reply pid
    echo "$name" > "$out/step"
    printf '\n[%s] %s\n' "$name" "$text"
    read -r -p "Pulsa Intro y hazlo en los 12 segundos siguientes. " reply
    pid="$(editor)"
    if [ -n "$pid" ]; then
        sample "$pid" 12 -file "$out/$name.sample" > /dev/null 2>&1
    else
        sleep 12
    fi
    echo "Midiendo. No toques ShareK."
    sleep 8
    snapshot "$name" "$detail"
}

launch() {
    "$root/scripts/quit.sh"
    if [ "$mode" = none ]; then
        open "$app"
    elif ! open --env "MallocStackLogging=$mode" "$app" 2> /dev/null; then
        launchctl setenv MallocStackLogging "$mode"
        open "$app"
        sleep 3
        launchctl unsetenv MallocStackLogging
    fi
}

big() {
    awk '
function mb(value, unit) { return unit == "GB" ? value * 1024 : unit == "MB" ? value + 0 : 0 }
/Footprint:|phys_footprint/ { print; next }
mb($1, $2) >= 5 || mb($3, $4) >= 50 || mb($5, $6) >= 50 { print }
'
}

peak() {
    LC_ALL=C awk -v step="$1" '
function mb(value, unit) { return unit == "GB" ? value * 1024 : unit == "KB" ? value / 1024 : unit == "MB" ? value + 0 : 0 }
function flush() { if (current == step && size > top) { top = size; best = block } }
/^@@/ { flush(); current = $3; block = ""; size = 0; next }
/Footprint:/ { for (i = 1; i < NF; i++) if ($i == "Footprint:") size = mb($(i + 1), $(i + 2)) }
{ block = block $0 "\n" }
END { flush(); printf "%s", best }
' "$out/samples.log" | big
}

regions() {
    grep -E '^(MALLOC_LARGE|IOSurface|CoreAnimation|CG |IOAccelerator|IOKit|VM_ALLOCATE|Image IO)' \
        | grep -E '[0-9]M' \
        | sed -E 's/[0-9a-f]{8,}-[0-9a-f]{8,}//; s/ +/ /g' \
        | sort | uniq -c | sort -rn | head -14
}

objects() {
    grep -E '^ *[0-9]+ +[0-9]+ ' | sed -E 's/ +/ /g' > "$out/objects.tmp"
    {
        head -10 "$out/objects.tmp"
        grep -E 'CABackingStore|IOSurface|CALayer|NSViewBackingLayer|NSWindow|CGImage|CGDataProvider|CGBitmapContext|SwiftData|CIRender|AnnotationView|EditorWindowController|CapturePage|CaptureDocument|BaseImage' "$out/objects.tmp" | head -24
    } | awk '!seen[$0]++'
}

stacks() {
    local lines
    lines="$(grep -E ' bytes' "$1" | head -6)"
    [ -n "$lines" ] || lines="$(grep -v '^$' "$1" | head -14)"
    printf '%s\n' "$lines" | LC_ALL=C awk '
{
    count = split($0, frame, " \\| ")
    if (count < 16) { print substr($0, 1, 1800); next }
    line = frame[1] " | ..."
    for (i = count - 13; i <= count; i++) line = line " | " frame[i]
    print line
}'
}

hot() {
    awk '/^Sort by top of stack/ { on = 1 } /^Binary Images/ { on = 0 } on' "$1" | head -24
}

summary() {
    local name
    echo "ShareK, informe de memoria ($mode)"
    cat "$out/system.txt"
    for name in $steps; do
        echo
        echo "===== $name"
        if [ ! -f "$out/$name.vmmap" ]; then
            echo "sin datos"
            continue
        fi
        echo "--- en reposo"
        big < "$out/$name.footprint"
        echo "--- pico durante el paso"
        peak "$name"
        echo "--- regiones"
        regions < "$out/$name.vmmap"
        echo "--- objetos"
        objects < "$out/$name.heap"
        if [ -f "$out/$name.windows" ]; then
            echo "--- ventanas"
            cat "$out/$name.windows"
        fi
        if [ -f "$out/$name.stacks" ] && [ -f "$out/$name.windows" ]; then
            echo "--- pilas"
            stacks "$out/$name.stacks"
        fi
        if [ -f "$out/$name.sample" ]; then
            echo "--- actividad"
            hot "$out/$name.sample"
        fi
    done
    if [ -f "$out/peak.stacks" ]; then
        echo
        echo "===== pilas en el máximo de malloc"
        stacks "$out/peak.stacks"
    fi
}

rm -rf "$out"
mkdir -p "$out"
echo inicio > "$out/step"

{
    sw_vers
    system_profiler SPDisplaysDataType | grep -E 'Chipset|Resolution|UI Looks|Display Type|Main Display'
    git -C "$root" rev-parse --short HEAD
} > "$out/system.txt" 2>&1

echo "Reinicio ShareK para medir."
launch
sampler &
sampler_pid=$!
trap 'kill "$sampler_pid" 2> /dev/null' EXIT

step 1-una full "Haz UNA captura de pantalla completa y espera a que se abra en el editor."
if ps -Eww -p "$(editor)" 2> /dev/null | grep -q MallocStackLogging; then
    echo "registro de pilas: activo, suma memoria propia (Performance Tool Data)" >> "$out/system.txt"
else
    echo "registro de pilas: inactivo" >> "$out/system.txt"
fi
step 2-tres basic "Haz DOS capturas de pantalla completa más. Deben quedar 3 pestañas."
timed 3-cuatro full "Haz UNA captura de pantalla completa más, la cuarta."
step 4-cambio basic "Cambia a la primera pestaña, luego a la segunda y vuelve a la última."
timed 5-dibujo basic "Dibuja un rectángulo y una flecha, y arrastra uno de los dos."
step 6-minimizada full "Minimiza la ventana del editor."
step 7-cerradas full "Restaura la ventana y cierra tres pestañas con el botón de la papelera. Debe quedar una."

pid="$(editor)"
if [ "$mode" = full ] && [ -n "$pid" ]; then
    malloc_history "$pid" -highWaterMark -allBySize > "$out/peak.stacks" 2>&1
fi

kill "$sampler_pid" 2> /dev/null
wait "$sampler_pid" 2> /dev/null
summary > "$out/resumen.txt" 2>&1
pbcopy < "$out/resumen.txt"

"$root/scripts/quit.sh"
open "$app"

cat "$out/resumen.txt"
printf '\nListo. El resumen está en el portapapeles y en %s/resumen.txt.\n' "$out"
