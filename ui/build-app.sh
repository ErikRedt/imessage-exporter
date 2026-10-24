#!/bin/sh
# Build a self-contained, ad-hoc-signed macOS application bundle.
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
APP_NAME="iMessage Exporter.app"
OUTPUT=${1:-"$ROOT/output/$APP_NAME"}
APP="$OUTPUT"
case "$APP" in
    *.app) ;;
    *) echo "Output path must end in .app" >&2; exit 2 ;;
esac

mkdir -p "$(dirname "$APP")"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

echo "Building exporter CLI..."
cargo build --release --manifest-path "$ROOT/Cargo.toml" -p imessage-exporter

echo "Building native UI..."
swiftc -O -o "$APP/Contents/MacOS/imessage-exporter-ui" "$ROOT/ui/main.swift"
cp "$ROOT/target/release/imessage-exporter" "$APP/Contents/MacOS/imessage-exporter"
cp "$ROOT/ui/Info.plist" "$APP/Contents/Info.plist"

# Keep the supported raster assets in the repository. `sips` cannot rasterize
# SVG consistently on the macOS releases this bundle supports.
/usr/bin/iconutil -c icns "$ROOT/ui/Resources/AppIcon.iconset" \
    -o "$APP/Contents/Resources/AppIcon.icns"

# An ad-hoc signature makes the bundle internally consistent while leaving
# distribution signing and notarization to a release workflow.
/usr/bin/codesign --force --sign - --timestamp=none "$APP/Contents/MacOS/imessage-exporter"
/usr/bin/codesign --force --sign - --timestamp=none "$APP/Contents/MacOS/imessage-exporter-ui"
/usr/bin/codesign --force --sign - --timestamp=none "$APP"
/usr/bin/codesign --verify --deep --strict "$APP"

echo "Built $APP"
