#!/bin/bash
# mini-oc macOS release 打包（需 Xcode + Flutter SDK + Homebrew CocoaPods）
# 产物：build/macos/Build/Products/Release/mini_oc.app（universal binary + rathole bundle）
set -euo pipefail
cd "$(dirname "$0")/.."

export PUB_HOSTED_URL="${PUB_HOSTED_URL:-https://pub.flutter-io.cn}"
export FLUTTER_STORAGE_BASE_URL="${FLUTTER_STORAGE_BASE_URL:-https://storage.flutter-io.cn}"

echo "==> frb codegen"
flutter_rust_bridge_codegen generate

echo "==> pod install（清 rvm GEM 环境污染）"
(cd macos && env -u GEM_HOME -u GEM_PATH pod install)

echo "==> flutter build macos --release"
env -u GEM_HOME -u GEM_PATH flutter build macos --release

APP="build/macos/Build/Products/Release/mini_oc.app"
if [ ! -d "$APP" ]; then
  echo "!! 构建产物缺失：$APP" >&2
  exit 1
fi

echo "==> 打包 rathole bundle 到 .app Resources"
RESOURCES="$APP/Contents/Resources"
mkdir -p "$RESOURCES/assets/rathole/bin/macos-aarch64" "$RESOURCES/assets/rathole/settings"
cp assets/rathole/bin/macos-aarch64/rathole "$RESOURCES/assets/rathole/bin/macos-aarch64/"
chmod +x "$RESOURCES/assets/rathole/bin/macos-aarch64/rathole"

echo "==> 完成：$APP"
du -sh "$APP"
