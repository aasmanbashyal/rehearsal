#!/bin/sh
# Builds dist/Rehearsal.app plus a DMG and ZIP of it.
#
#   ./scripts/build-macos.sh               # this Mac's architecture
#   ./scripts/build-macos.sh --universal   # Apple silicon + Intel in one app
#
# Set CODESIGN_IDENTITY to a "Developer ID Application" identity to sign for
# distribution; otherwise the app is signed ad hoc for use on this Mac.
set -eu

cd "$(dirname "$0")/.."
project=$PWD
universal=0
for arg in "$@"; do
  case $arg in
    --universal) universal=1 ;;
    *) echo "Unknown option: $arg" >&2; exit 2 ;;
  esac
done
[ "$(uname -s)" = Darwin ] || { echo "Build the macOS app on macOS." >&2; exit 1; }

# Use the project-local toolchain when present, like scripts/backend.js.
if [ -x "$project/.rust/cargo/bin/cargo" ]; then
  export CARGO_HOME="$project/.rust/cargo" RUSTUP_HOME="$project/.rust/rustup"
  PATH="$CARGO_HOME/bin:$PATH"
fi
command -v cargo >/dev/null || { echo "Install Rust from https://rustup.rs first." >&2; exit 1; }

version=$(sed -n 's/^version = "\(.*\)"$/\1/p' desktop-rust/Cargo.toml | head -n 1)
# WKWebView's media capture permission API needs macOS 12. The app also
# checks at launch that the loaded WebKit is Safari 16.2 or later.
export MACOSX_DEPLOYMENT_TARGET=12.0
export CARGO_TARGET_DIR="$project/desktop-rust/target/macos"
manifest=desktop-rust/Cargo.toml

if [ $universal = 1 ]; then
  arch=universal
  for target in aarch64-apple-darwin x86_64-apple-darwin; do
    command -v rustup >/dev/null && rustup target add "$target" >/dev/null
    cargo build --locked --release --manifest-path "$manifest" --target "$target"
  done
  binary="$CARGO_TARGET_DIR/rehearse-universal"
  lipo -create -output "$binary" \
    "$CARGO_TARGET_DIR/aarch64-apple-darwin/release/rehearse" \
    "$CARGO_TARGET_DIR/x86_64-apple-darwin/release/rehearse"
else
  arch=$(uname -m)
  cargo build --locked --release --manifest-path "$manifest"
  binary="$CARGO_TARGET_DIR/release/rehearse"
fi

dist="$project/dist"
app="$dist/Rehearsal.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$binary" "$app/Contents/MacOS/Rehearsal"

# Render the SVG mark into an icon set with the standard macOS margin.
work=$(mktemp -d "${TMPDIR:-/tmp}/rehearsal-icon.XXXXXX")
trap 'rm -rf "$work"' EXIT
qlmanage -t -s 824 -o "$work" public/assets/mark.svg >/dev/null 2>&1
[ -f "$work/mark.svg.png" ] || { echo "Could not render the app icon." >&2; exit 1; }
sips --padToHeightWidth 1024 1024 "$work/mark.svg.png" --out "$work/icon-1024.png" >/dev/null
iconset="$work/Rehearsal.iconset"
mkdir "$iconset"
for size in 16 32 128 256 512; do
  sips -z $size $size "$work/icon-1024.png" --out "$iconset/icon_${size}x${size}.png" >/dev/null
  double=$((size * 2))
  sips -z $double $double "$work/icon-1024.png" --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/Rehearsal.icns"

cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key><string>en</string>
  <key>CFBundleDisplayName</key><string>Rehearsal</string>
  <key>CFBundleExecutable</key><string>Rehearsal</string>
  <key>CFBundleIconFile</key><string>Rehearsal</string>
  <!-- Stable from the first macOS release: WebKit storage and privacy
       permissions are keyed to this identifier. -->
  <key>CFBundleIdentifier</key><string>io.github.aasmanbashyal.rehearsal</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>Rehearsal</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.education</string>
  <key>LSMinimumSystemVersion</key><string>$MACOSX_DEPLOYMENT_TARGET</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
  <key>NSHumanReadableCopyright</key><string>Rehearsal</string>
  <key>NSMicrophoneUsageDescription</key><string>Rehearsal records your voice for practice takes.</string>
  <key>NSCameraUsageDescription</key><string>Rehearsal records camera video for practice takes.</string>
  <key>NSAppTransportSecurity</key>
  <dict><key>NSAllowsLocalNetworking</key><true/></dict>
</dict>
</plist>
EOF

if [ -n "${CODESIGN_IDENTITY:-}" ]; then
  codesign --force --options runtime --timestamp --entitlements scripts/macos-entitlements.plist \
    --sign "$CODESIGN_IDENTITY" "$app"
else
  codesign --force --sign - "$app"
fi
codesign --verify --strict "$app"

stage="$work/dmg"
mkdir "$stage"
cp -R "$app" "$stage/"
ln -s /Applications "$stage/Applications"
dmg="$dist/Rehearsal-$version-macos-$arch.dmg"
zip="$dist/Rehearsal-$version-macos-$arch.zip"
rm -f "$dmg" "$zip"
hdiutil create -quiet -volname Rehearsal -srcfolder "$stage" -ov -format UDZO "$dmg"
ditto -c -k --keepParent "$app" "$zip"
echo "Built $app"
echo "      $dmg"
echo "      $zip"
