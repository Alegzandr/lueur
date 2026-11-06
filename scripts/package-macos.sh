#!/usr/bin/env bash
# Makes the universal Mac app (Apple silicon and Intel) from two release
# builds, and its two archives.
# Usage: package-macos.sh <version> <out dir>
#
# The bundle is shaped as the Tauri one was, so an installed copy updating
# into it, and the login item pointing into it, find what they expect:
# Lueur.app, identifier com.lueur.app, executable Contents/MacOS/lueur. The
# .app.tar.gz is the update file (the .app at its top, no AppleDouble
# files); the .zip is the first download.
set -euo pipefail
version=$1
out=$2
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

for target in aarch64-apple-darwin x86_64-apple-darwin; do
  cargo build --release --locked -p lueur-slint --target "$target"
done

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
app="$work/Lueur.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
lipo -create -output "$app/Contents/MacOS/lueur" \
  target/aarch64-apple-darwin/release/lueur \
  target/x86_64-apple-darwin/release/lueur
cp packaging/macos/icon.icns "$app/Contents/Resources/icon.icns"

# The keys the Tauri bundler wrote, then those of packaging/macos/Info.plist
# (why the Music effect asks for the microphone). macOS 10.15 at least, as
# the earlier versions asked.
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key><string>en</string>
  <key>CFBundleDisplayName</key><string>Lueur</string>
  <key>CFBundleExecutable</key><string>lueur</string>
  <key>CFBundleIconFile</key><string>icon.icns</string>
  <key>CFBundleIdentifier</key><string>com.lueur.app</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>Lueur</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${version}</string>
  <key>CFBundleVersion</key><string>${version}</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.utilities</string>
  <key>LSMinimumSystemVersion</key><string>10.15</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSRequiresAquaSystemAppearance</key><false/>
</dict>
</plist>
PLIST
/usr/libexec/PlistBuddy -c "Merge packaging/macos/Info.plist" "$app/Contents/Info.plist"
plutil -lint "$app/Contents/Info.plist"

mkdir -p "$out"
base="$out/Lueur_${version}_macos_universal"
COPYFILE_DISABLE=1 tar -czf "$base.app.tar.gz" -C "$work" "Lueur.app"
ditto -c -k --keepParent "$app" "$base.zip"
