#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_NAME="VTerm"
BUNDLE_DIR="$ROOT/dist/$APP_NAME.app"
PORTABLE_DIR="$ROOT/dist/$APP_NAME-alpha"
CONTENTS_DIR="$BUNDLE_DIR/Contents"
MACOS_DIR="$CONTENTS_DIR/MacOS"
RESOURCES_DIR="$CONTENTS_DIR/Resources"
BIN_PATH="$ROOT/target/release/vterm-app"

mkdir -p "$MACOS_DIR" "$RESOURCES_DIR" "$PORTABLE_DIR"

cargo build --release --manifest-path "$ROOT/app/Cargo.toml"

cp "$BIN_PATH" "$MACOS_DIR/$APP_NAME"
cp "$ROOT/lua/bootstrap.lua" "$RESOURCES_DIR/bootstrap.lua"
cp "$BIN_PATH" "$PORTABLE_DIR/vterm-app"
cp "$ROOT/lua/bootstrap.lua" "$PORTABLE_DIR/bootstrap.lua"

cat > "$CONTENTS_DIR/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleDisplayName</key>
  <string>VTerm</string>
  <key>CFBundleExecutable</key>
  <string>VTerm</string>
  <key>CFBundleIdentifier</key>
  <string>com.venusdafur.vterm</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>VTerm</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>0.1.0-alpha</string>
  <key>CFBundleVersion</key>
  <string>1</string>
  <key>LSMinimumSystemVersion</key>
  <string>13.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
PLIST

printf 'APPL????' > "$CONTENTS_DIR/PkgInfo"

chmod +x "$MACOS_DIR/$APP_NAME"
cat > "$PORTABLE_DIR/run-vterm" <<'RUNNER'
#!/usr/bin/env bash
set -euo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$DIR"
exec "$DIR/vterm-app"
RUNNER
chmod +x "$PORTABLE_DIR/run-vterm"

echo "Built app bundle at: $BUNDLE_DIR"
echo "Built portable alpha at: $PORTABLE_DIR"
