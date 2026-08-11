#!/usr/bin/env bash
# INPUT:  alva-hub-gpui release binary
# OUTPUT: ALVA Hub.app — macOS bundle for the Rust-native GPUI build
# POS:    P4 packaging. After migration parity, the Tauri .app is deleted and
#         this is the product artifact (Web later via GPUI wasm).
#
# Usage:  scripts/package-gpui-app.sh

set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP_NAME="ALVA Hub"
APP_DIR="$ROOT/target/release/bundle/gpui/$APP_NAME.app"
BIN_NAME="alva-hub-gpui"

echo "# packaging $APP_NAME (GPUI)"
cargo build --release -p alva-hub-gpui --manifest-path "$ROOT/Cargo.toml"

rm -rf "$APP_DIR"
mkdir -p "$APP_DIR/Contents/MacOS" "$APP_DIR/Contents/Resources"

cat > "$APP_DIR/Contents/Info.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>ALVA Hub</string>
  <key>CFBundleDisplayName</key><string>ALVA Hub</string>
  <key>CFBundleIdentifier</key><string>com.alva.hub.gpui</string>
  <key>CFBundleExecutable</key><string>alva-hub-gpui</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSApplicationCategoryType</key><string>public.app-category.developer-tools</string>
</dict>
</plist>
EOF

cp "$ROOT/target/release/$BIN_NAME" "$APP_DIR/Contents/MacOS/$BIN_NAME"
chmod +x "$APP_DIR/Contents/MacOS/$BIN_NAME"
codesign --force --sign - "$APP_DIR" 2>/dev/null || true

echo "✅ $APP_DIR"
du -sh "$APP_DIR"
