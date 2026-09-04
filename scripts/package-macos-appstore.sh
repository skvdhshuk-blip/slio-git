#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

APP_NAME="slio-git"
BUNDLE_ID="com.slio.git"
TEAM_ID="${DEVELOPMENT_TEAM:-M2WM2NJP68}"
TARGET="${MACOS_TARGET:-$(rustc -vV | sed -n 's/^host: //p')}"
CARGO_TARGET_ROOT="${CARGO_TARGET_DIR:-$ROOT_DIR/target}"
APP_DIR="$ROOT_DIR/dist/${APP_NAME}.app"
PKG_PATH="$ROOT_DIR/dist/${APP_NAME}-appstore.pkg"
MACOS_DIR="$APP_DIR/Contents/MacOS"
RESOURCES_DIR="$APP_DIR/Contents/Resources"
INFO_PLIST="$APP_DIR/Contents/Info.plist"
ENTITLEMENTS="$ROOT_DIR/packaging/macos/slio-git.entitlements"
PROFILE="${MAS_PROVISIONING_PROFILE:-$ROOT_DIR/packaging/macos/slio-git-mas.provisionprofile}"
BUILD_NUMBER="$(tr -d '[:space:]' < "$ROOT_DIR/packaging/macos/BUILD_NUMBER")"

VERSION="$(
python3 - <<'PY'
from pathlib import Path
import re

text = Path("Cargo.toml").read_text()
match = re.search(
    r"\[workspace\.package\](?:.*?\n)*?version\s*=\s*\"([^\"]+)\"",
    text,
    re.S,
)
if not match:
    raise SystemExit("Failed to read workspace version from Cargo.toml")
print(match.group(1))
PY
)"

if ! rustup target list --installed | grep -qx "$TARGET"; then
  echo "Installing Rust target: $TARGET"
  rustup target add "$TARGET"
fi

echo "Building App Store binary..."
cargo build --locked --release -p src-ui --features app-store --target "$TARGET"

echo "Preparing app bundle..."
rm -rf "$APP_DIR"
mkdir -p "$MACOS_DIR" "$RESOURCES_DIR"
BIN_PATH="$CARGO_TARGET_ROOT/$TARGET/release/src-ui"
if [[ ! -f "$BIN_PATH" ]]; then
  BIN_PATH="$CARGO_TARGET_ROOT/release/src-ui"
fi
if [[ ! -f "$BIN_PATH" ]]; then
  echo "Missing release binary at $CARGO_TARGET_ROOT/$TARGET/release/src-ui" >&2
  exit 1
fi
cp "$BIN_PATH" "$MACOS_DIR/$APP_NAME"
chmod 755 "$MACOS_DIR/$APP_NAME"

ICON_SRC="$ROOT_DIR/src-ui/assets/AppIcon.icns"
if [[ -f "$ICON_SRC" ]]; then
  cp "$ICON_SRC" "$RESOURCES_DIR/AppIcon.icns"
  ICON_KEY='  <key>CFBundleIconFile</key>
  <string>AppIcon</string>'
else
  ICON_KEY=""
fi

cat > "$INFO_PLIST" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleDisplayName</key>
  <string>${APP_NAME}</string>
  <key>CFBundleExecutable</key>
  <string>${APP_NAME}</string>
  <key>CFBundleIdentifier</key>
  <string>${BUNDLE_ID}</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>${APP_NAME}</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>${VERSION}</string>
  <key>CFBundleVersion</key>
  <string>${BUILD_NUMBER}</string>
${ICON_KEY}
  <key>LSApplicationCategoryType</key>
  <string>public.app-category.developer-tools</string>
  <key>LSMinimumSystemVersion</key>
  <string>12.0</string>
  <key>ITSAppUsesNonExemptEncryption</key>
  <false/>
  <key>NSHighResolutionCapable</key>
  <true/>
  <key>NSDocumentsFolderUsageDescription</key>
  <string>slio-git opens Git repositories you select.</string>
  <key>NSDesktopFolderUsageDescription</key>
  <string>slio-git opens Git repositories you select.</string>
  <key>NSDownloadsFolderUsageDescription</key>
  <string>slio-git opens Git repositories you select.</string>
  <key>NSRemovableVolumesUsageDescription</key>
  <string>slio-git opens Git repositories you select on removable volumes.</string>
  <key>NSNetworkVolumesUsageDescription</key>
  <string>slio-git opens Git repositories you select on network volumes.</string>
</dict>
</plist>
EOF

printf 'APPL????' > "$APP_DIR/Contents/PkgInfo"
plutil -lint "$INFO_PLIST" >/dev/null

if [[ ! -f "$PROFILE" ]]; then
  echo "Missing Mac App Store provisioning profile at $PROFILE" >&2
  exit 1
fi
cp "$PROFILE" "$APP_DIR/Contents/embedded.provisionprofile"

SIGN_IDENTITY="${CODESIGN_IDENTITY:-}"
if [[ -z "$SIGN_IDENTITY" ]]; then
  if security find-identity -v -p codesigning 2>/dev/null | grep -q "Apple Distribution"; then
    SIGN_IDENTITY="$(security find-identity -v -p codesigning | awk -F'\"' '/Apple Distribution/ {print $2; exit}')"
  else
    SIGN_IDENTITY="-"
  fi
fi

echo "Signing ${APP_DIR} with ${SIGN_IDENTITY} (team ${TEAM_ID})"
codesign --force --deep --options runtime \
  --entitlements "$ENTITLEMENTS" \
  --sign "$SIGN_IDENTITY" \
  "$APP_DIR"

codesign --verify --verbose=2 "$APP_DIR"

INSTALLER_IDENTITY="${INSTALLER_IDENTITY:-}"
if [[ -z "$INSTALLER_IDENTITY" ]]; then
  if security find-identity -v | grep -q "3rd Party Mac Developer Installer"; then
    INSTALLER_IDENTITY="$(security find-identity -v | awk -F'\"' '/3rd Party Mac Developer Installer/ {print $2; exit}')"
  fi
fi

if [[ -n "$INSTALLER_IDENTITY" && "$SIGN_IDENTITY" != "-" ]]; then
  rm -f "$PKG_PATH"
  productbuild \
    --component "$APP_DIR" /Applications \
    --sign "$INSTALLER_IDENTITY" \
    "$PKG_PATH"
  echo "Done:"
  echo "  App: $APP_DIR"
  echo "  Pkg: $PKG_PATH"
else
  echo "Done (ad-hoc or missing installer identity; pkg skipped):"
  echo "  App: $APP_DIR"
fi
