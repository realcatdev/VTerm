#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_NAME="VTerm"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -n 1)"
DIST_DIR="$ROOT/dist"
BUNDLE_DIR="$DIST_DIR/$APP_NAME.app"
DMG_STAGE="$DIST_DIR/dmg-root"
DMG_NAME="$APP_NAME-$VERSION-macos.dmg"
DMG_PATH="$DIST_DIR/$DMG_NAME"
VOL_NAME="$APP_NAME"

bash "$ROOT/scripts/package_macos_app.sh"

rm -rf "$DMG_STAGE" "$DMG_PATH"
mkdir -p "$DMG_STAGE"

cp -R "$BUNDLE_DIR" "$DMG_STAGE/$APP_NAME.app"
ln -s /Applications "$DMG_STAGE/Applications"

hdiutil create \
  -volname "$VOL_NAME" \
  -srcfolder "$DMG_STAGE" \
  -ov \
  -format UDZO \
  "$DMG_PATH" >/dev/null

rm -rf "$DMG_STAGE"

echo "Built release DMG at: $DMG_PATH"
