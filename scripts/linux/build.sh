#!/usr/bin/env bash
# Builds Linux packages (AppImage, .deb, .rpm) inside an Ubuntu 22.04 machine
# prepared with provision.sh. From Windows:
#   wsl -d RopewayBuild -u root -- bash "/mnt/c/.../scripts/linux/build.sh"
set -euo pipefail
src="$(cd "$(dirname "$0")/../.." && pwd)"
work="$HOME/ropeway"
export PATH="$HOME/.cargo/bin:$PATH"
# linuxdeploy (AppImage) runs without FUSE this way, as in WSL or containers.
export APPIMAGE_EXTRACT_AND_RUN=1

mkdir -p "$work"
rsync -a --delete \
  --exclude node_modules --exclude target --exclude dist --exclude releases \
  --exclude 'src-tauri/gen/android' --exclude 'src-tauri/target' --exclude user_revisions \
  "$src/" "$work/"
cd "$work"
npm ci --no-audit --no-fund
npx tauri build --bundles appimage,deb,rpm

version=$(node -p "require('./src-tauri/tauri.conf.json').version")
out="$src/releases/$version"
mkdir -p "$out"
b="target/release/bundle"
cp "$b"/appimage/*.AppImage "$out/Ropeway-$version-linux-x86_64.AppImage"
cp "$b"/deb/*.deb "$out/Ropeway-$version-linux-amd64.deb"
cp "$b"/rpm/*.rpm "$out/Ropeway-$version-linux-x86_64.rpm"
ls -la "$out" | grep linux
