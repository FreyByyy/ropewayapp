#!/usr/bin/env bash
# Prepares an Ubuntu 22.04 machine (or WSL distro) to build Ropeway for Linux.
# Building on 22.04 keeps the packages working on newer distributions too.
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get install -y --no-install-recommends \
  build-essential curl wget file git ca-certificates pkg-config clang nasm patchelf rpm xz-utils rsync \
  libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libssl-dev \
  libxdo-dev libasound2-dev libpipewire-0.3-dev libspa-0.2-dev libdbus-1-dev squashfs-tools
# Rust
if ! command -v cargo >/dev/null && [ ! -x "$HOME/.cargo/bin/cargo" ]; then
  curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
fi
# Node.js 22
if ! command -v node >/dev/null; then
  ver=v22.20.0
  curl -sSL "https://nodejs.org/dist/$ver/node-$ver-linux-x64.tar.xz" | tar -xJ -C /usr/local --strip-components=1
fi
"$HOME/.cargo/bin/cargo" --version
node --version

# Ubuntu 22.04 ships PipeWire 0.3.48 headers, older than the Rust bindings
# need. Newer headers from the PipeWire upstream PPA; the program still links
# against the stable libpipewire-0.3.so.0 ABI and runs with the system's.
if ! pkg-config --atleast-version=1.0 libpipewire-0.3; then
  apt-get install -y --no-install-recommends software-properties-common gpg-agent
  add-apt-repository -y ppa:pipewire-debian/pipewire-upstream
  apt-get update
  apt-get install -y --no-install-recommends libpipewire-0.3-dev libspa-0.2-dev
fi
pkg-config --modversion libpipewire-0.3
