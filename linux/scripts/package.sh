#!/bin/sh
set -eu
repo=$(cd "$(dirname "$0")/.." && pwd)
out="$repo/dist/sharek-install.sh"
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT

cargo build --release --workspace --manifest-path "$repo/Cargo.toml"

for script in install.sh uninstall.sh fetch-ocr-models.sh; do
    install -D -m755 "$repo/scripts/$script" "$stage/scripts/$script"
done
for program in sharek sharek-editor sharek-gif sharek-ocr; do
    install -D -m755 "$repo/target/release/$program" "$stage/target/release/$program"
done
cp -r "$repo/data" "$stage/data"
install -D -m644 "$repo/../assets/icon/AppIcon.svg" "$stage/assets/icon/AppIcon.svg"

mkdir -p "$repo/dist"
{
    cat <<'SCRIPT'
#!/bin/sh
set -eu
dir=$(mktemp -d)
trap 'rm -rf "$dir"' EXIT
tail -n +"$(awk '/^__PAYLOAD__$/ { print NR + 1; exit }' "$0")" "$0" | tar xz -C "$dir"
if [ "${1:-}" = --uninstall ]; then
    "$dir/scripts/uninstall.sh"
else
    "$dir/scripts/install.sh"
fi
exit 0
__PAYLOAD__
SCRIPT
    tar cz -C "$stage" --owner=0 --group=0 --numeric-owner .
} > "$out"
chmod 755 "$out"
echo "$out"
