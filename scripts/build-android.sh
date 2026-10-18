#!/usr/bin/env bash
# Builds a signed release APK.   scripts/build-android.sh [arm64|x86|x86_64|arm] ...
#
# `tauri android build` links the Rust library into jniLibs with a symbolic
# link, which Windows allows only in Developer Mode; this copies it instead
# and runs Gradle itself. Output: releases/<version>/Ropeway-<version>-android-<arch>.apk
set -euo pipefail
cd "$(dirname "$0")/.."

export JAVA_HOME="${JAVA_HOME:-C:\\Program Files\\Microsoft\\jdk-17.0.20.101-hotspot}"
export ANDROID_HOME="${ANDROID_HOME:-$LOCALAPPDATA\\Android\\Sdk}"
export NDK_HOME="${NDK_HOME:-$ANDROID_HOME\\ndk\\27.2.12479018}"
export PATH="$HOME/.cargo/bin:$PATH"

version=$(python -c "import json;print(json.load(open('src-tauri/tauri.conf.json'))['version'])")
out="releases/$version"
mkdir -p "$out"

for arch in "${@:-arm64}"; do
  case "$arch" in
    arm64) tauri=aarch64; rust=aarch64-linux-android; abi=arm64-v8a; flavor=Arm64 ;;
    arm) tauri=armv7; rust=armv7-linux-androideabi; abi=armeabi-v7a; flavor=Arm ;;
    x86) tauri=i686; rust=i686-linux-android; abi=x86; flavor=X86 ;;
    x86_64) tauri=x86_64; rust=x86_64-linux-android; abi=x86_64; flavor=X86_64 ;;
    *) echo "unknown arch: $arch"; exit 1 ;;
  esac
  echo "== $arch"
  # Builds the frontend and the library; fails only at the symlink step.
  npx tauri android build --apk --target "$tauri" || true
  lib="target/$rust/release/libropeway_lib.so"
  [ -f "$lib" ] || { echo "library not built: $lib"; exit 1; }
  jni="src-tauri/gen/android/app/src/main/jniLibs/$abi"
  mkdir -p "$jni"
  rm -f "$jni/libropeway_lib.so"
  cp "$lib" "$jni/"
  (cd src-tauri/gen/android && ./gradlew.bat "assemble${flavor}Release" -x "rustBuild${flavor}Release")
  lower=$(echo "$flavor" | tr 'A-Z' 'a-z')
  cp "src-tauri/gen/android/app/build/outputs/apk/$lower/release/app-$lower-release.apk" \
    "$out/Ropeway-$version-android-$arch.apk"
  echo "$out/Ropeway-$version-android-$arch.apk"
done
