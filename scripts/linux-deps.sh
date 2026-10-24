#!/usr/bin/env sh
# Libraries needed by Tauri 2, audio capture (cpal: ALSA) and screen capture
# (xcap: X11, Wayland, PipeWire) on Ubuntu 24.04. 22.04 is too old: its
# PipeWire (0.3.48) lacks what the pipewire 0.10 crate binds.
set -eu
sudo apt-get update
sudo apt-get install -y --no-install-recommends \
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
