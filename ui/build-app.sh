#!/bin/sh
# Build a self-contained, ad-hoc-signed macOS application bundle.
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
APP_NAME="iMessage Exporter.app"
OUTPUT=${1:-"$ROOT/output/$APP_NAME"}
APP="$OUTPUT"
TEMP=$(mktemp -d "${TMPDIR:-/tmp}/imessage-exporter-app.XXXXXX")
trap 'rm -rf "$TEMP"' EXIT HUP INT TERM

case "$APP" in
    *.app) ;;
    *) echo "Output path must end in .app" >&2; exit 2 ;;
esac

mkdir -p "$(dirname "$APP")"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$TEMP/AppIcon.iconset"

echo "Building exporter CLI..."
cargo build --release --manifest-path "$ROOT/Cargo.toml" -p imessage-exporter

echo "Building native UI..."
swiftc -O -o "$APP/Contents/MacOS/imessage-exporter-ui" "$ROOT/ui/main.swift"
cp "$ROOT/target/release/imessage-exporter" "$APP/Contents/MacOS/imessage-exporter"
cp "$ROOT/ui/Info.plist" "$APP/Contents/Info.plist"

for size in 16 32 128 256 512; do
    /usr/bin/sips -s format png -z "$size" "$size" "$ROOT/ui/Resources/AppIcon.svg" \
        --out "$TEMP/AppIcon.iconset/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    /usr/bin/sips -s format png -z "$double" "$double" "$ROOT/ui/Resources/AppIcon.svg" \
        --out "$TEMP/AppIcon.iconset/icon_${size}x${size}@2x.png" >/dev/null
done
/usr/bin/iconutil -c icns "$TEMP/AppIcon.iconset" -o "$APP/Contents/Resources/AppIcon.icns"

# An ad-hoc signature makes the bundle internally consistent while leaving
# distribution signing and notarization to a release workflow.
/usr/bin/codesign --force --sign - --timestamp=none "$APP/Contents/MacOS/imessage-exporter"
/usr/bin/codesign --force --sign - --timestamp=none "$APP/Contents/MacOS/imessage-exporter-ui"
/usr/bin/codesign --force --sign - --timestamp=none "$APP"
/usr/bin/codesign --verify --deep --strict "$APP"

echo "Built $APP"
