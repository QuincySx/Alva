#!/usr/bin/env bash
# Build the GPUI Hub Desktop app and bundle it as a macOS .app.
# POS: replaces the Tauri `tauri build` path for alva-hub-gpui.
set -euo pipefail
cd "$(dirname "$0")/.."

APP_NAME="ALVA Hub"
BUNDLE="target/release/bundle/macos/${APP_NAME}.app"

echo "==> cargo build --release -p alva-hub-gpui"
cargo build --release -p alva-hub-gpui

echo "==> assembling ${BUNDLE}"
rm -rf "${BUNDLE}"
mkdir -p "${BUNDLE}/Contents/MacOS"

cp target/release/alva-hub-gpui "${BUNDLE}/Contents/MacOS/alva-hub-gpui"

cat > "${BUNDLE}/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key>
  <string>ALVA Hub</string>
  <key>CFBundleDisplayName</key>
  <string>ALVA Hub</string>
  <key>CFBundleIdentifier</key>
  <string>com.alva.hub-gpui</string>
  <key>CFBundleExecutable</key>
  <string>alva-hub-gpui</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>0.1.0</string>
  <key>CFBundleVersion</key>
  <string>1</string>
  <key>LSMinimumSystemVersion</key>
  <string>12.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
  <key>NSPrincipalClass</key>
  <string>NSApplication</string>
</dict>
</plist>
PLIST

echo "==> done: ${BUNDLE}"
