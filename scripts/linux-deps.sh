#!/usr/bin/env sh
# Libraries needed to build Lueur on Linux: Tauri 2 (WebKitGTK, GTK, the tray),
# audio capture (cpal: ALSA) and screen capture (xcap: X11 and Wayland; the
# ScreenCast portal stream: PipeWire, and libclang to generate its bindings).
#
# Debian/Ubuntu (apt), Fedora (dnf) and Arch (pacman). Ubuntu needs 24.04 or
# later: 22.04's PipeWire (0.3.48) lacks what the pipewire 0.10 crate binds.
# docs/DEVELOPMENT.md lists the same packages; keep the two in step.
set -eu

if [ "$(id -u)" -eq 0 ]; then
  SUDO=""
else
  SUDO="sudo"
fi

if command -v apt-get >/dev/null 2>&1; then
  $SUDO apt-get update
  $SUDO apt-get install -y --no-install-recommends \
    build-essential file pkg-config clang libclang-dev \
    libwebkit2gtk-4.1-dev \
    libgtk-3-dev \
    libayatana-appindicator3-dev \
    librsvg2-dev \
    libsoup-3.0-dev \
    libssl-dev \
    libxdo-dev \
    libxcb1-dev libxrandr-dev \
    libdbus-1-dev \
    libasound2-dev \
    libpipewire-0.3-dev \
    libwayland-dev libegl-dev libgbm-dev
elif command -v dnf >/dev/null 2>&1; then
  $SUDO dnf install -y \
    gcc gcc-c++ make file pkgconf-pkg-config clang clang-devel \
    webkit2gtk4.1-devel \
    gtk3-devel \
    libappindicator-gtk3-devel \
    librsvg2-devel \
    libsoup3-devel \
    openssl-devel \
    libxdo-devel \
    libxcb-devel libXrandr-devel \
    dbus-devel \
    alsa-lib-devel \
    pipewire-devel \
    wayland-devel mesa-libEGL-devel mesa-libgbm-devel
elif command -v pacman >/dev/null 2>&1; then
  $SUDO pacman -S --needed --noconfirm \
    base-devel file pkgconf clang \
    webkit2gtk-4.1 \
    gtk3 \
    libayatana-appindicator \
    librsvg \
    libsoup3 \
    openssl \
    xdotool \
    libxcb libxrandr \
    dbus \
    alsa-lib \
    pipewire \
    wayland libglvnd mesa
else
  echo "linux-deps.sh: no apt-get, dnf or pacman here; install the packages" >&2
  echo "listed in docs/DEVELOPMENT.md with your package manager." >&2
  exit 1
fi
