#!/bin/sh
set -eu
data="${XDG_DATA_HOME:-$HOME/.local/share}/sharek/ocr"
base="https://huggingface.co/nakamura196/ppocrv5-mobile-web-onnx/resolve/dc7e2271c00d961a4c0a38dcebe23beb735ba33e"
install -d "$data"

verify() {
    echo "$2  $1" | sha256sum -c --status
}

fetch() {
    if [ -f "$data/$1" ] && verify "$data/$1" "$2"; then
        return
    fi
    curl -fL --retry 3 -o "$data/$1.part" "$base/$1"
    if ! verify "$data/$1.part" "$2"; then
        rm -f "$data/$1.part"
        echo "$1: la suma de verificación no coincide" >&2
        exit 1
    fi
    mv -f "$data/$1.part" "$data/$1"
}

fetch PP-OCRv5_mobile_det.onnx 6bdf743af375b13fbbeeb47a91a40063af5bfb7a7f197f6df8cb990dc827e5d6
fetch latin_PP-OCRv5_mobile_rec.onnx 46449fd0606cd07d856744703211ea6826e4642d4c803d91167c53cde01d3b94
fetch latin_PP-OCRv5_mobile_rec.dict.json 705592cc40b955ac82cdfdb049ec75f0a9041824d524eeb946a913a34d285409
