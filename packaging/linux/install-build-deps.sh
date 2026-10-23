#!/usr/bin/env bash
# System libraries GPUI needs to build on Debian and Ubuntu (X11, Wayland, Vulkan, fonts).
set -euo pipefail

sudo apt-get update
sudo apt-get install -y --no-install-recommends \
    pkg-config \
    libfontconfig-dev \
    libfreetype-dev \
    libvulkan-dev \
    libwayland-dev \
    libx11-xcb-dev \
    libxcb1-dev \
    libxkbcommon-dev \
    libxkbcommon-x11-dev
