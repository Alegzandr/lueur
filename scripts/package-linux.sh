#!/usr/bin/env bash
# Makes the Linux AppImage from a release build (target/release/lueur).
# Usage: package-linux.sh <version> <out dir>
#
# The AppImage is also the update file: an installed copy swaps itself for
# it (crates/lueur-shell/src/update.rs, as the Tauri updater did), so its
# name and shape stay those of the Tauri releases.
#
# linuxdeploy gathers the libraries the binary needs, and leaves out those
# every desktop has (the GPU drivers and Vulkan among them, which the binary
# opens at run time). It is pinned to one release and checked against its
# checksum before it runs.
set -euo pipefail
version=$1
out=$2
root=$(cd "$(dirname "$0")/.." && pwd)

LINUXDEPLOY_URL=https://github.com/linuxdeploy/linuxdeploy/releases/download/1-alpha-20251107-1/linuxdeploy-x86_64.AppImage
LINUXDEPLOY_SHA256=c20cd71e3a4e3b80c3483cef793cda3f4e990aca14014d23c544ca3ce1270b4d

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
curl -fsSL -o "$work/linuxdeploy" "$LINUXDEPLOY_URL"
echo "$LINUXDEPLOY_SHA256  $work/linuxdeploy" | sha256sum -c -
chmod +x "$work/linuxdeploy"

# Without FUSE on the build machine, the tool unpacks itself and runs.
export APPIMAGE_EXTRACT_AND_RUN=1
export LINUXDEPLOY_OUTPUT_VERSION=$version
(
  cd "$work"
  ./linuxdeploy \
    --appdir "$work/Lueur.AppDir" \
    --executable "$root/target/release/lueur" \
    --desktop-file "$root/packaging/linux/lueur.desktop" \
    --icon-file "$root/packaging/linux/lueur.png" \
    --output appimage
)

mkdir -p "$out"
images=("$work"/Lueur*.AppImage)
if [ "${#images[@]}" -ne 1 ] || [ ! -f "${images[0]}" ]; then
  echo "package-linux.sh: expected one AppImage, found: ${images[*]}" >&2
  exit 1
fi
mv "${images[0]}" "$out/Lueur_${version}_linux_x86_64.AppImage"
