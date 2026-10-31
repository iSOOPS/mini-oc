#!/bin/bash
# mini-oc macOS release 打包（需 Xcode + Flutter SDK + Homebrew CocoaPods）
# 产物：build/macos/Build/Products/Release/mini_oc.app（universal binary + rathole bundle）
set -euo pipefail
cd "$(dirname "$0")/.."

export PUB_HOSTED_URL="${PUB_HOSTED_URL:-https://pub.flutter-io.cn}"
export FLUTTER_STORAGE_BASE_URL="${FLUTTER_STORAGE_BASE_URL:-https://storage.flutter-io.cn}"

# CocoaPods 需 ≥1.15 才能解析 Flutter 3.47 生成的 SPM 引用（XCLocalSwiftPackageReference）。
# rvm 自带的 pod 1.11.3 会抢占 PATH 并报 "unknown ISA"，故 pod 步骤改用净化环境
# 强制走 Homebrew CocoaPods（绕开 rvm 的 ruby/GEM 环境）。
# GEM_HOME/GEM_PATH 同步指向 Homebrew cocoapods libexec（否则 rvm gem env 污染导致
# "Could not find 'rexml' 等原生 gem" 激活失败）。
POD_ENV=(env -i HOME="$HOME" LANG="en_US.UTF-8" LC_ALL="en_US.UTF-8" \
  GEM_HOME="/opt/homebrew/Cellar/cocoapods/1.17.0_1/libexec" \
  GEM_PATH="/opt/homebrew/Cellar/cocoapods/1.17.0_1/libexec" \
  PATH="/opt/homebrew/bin:/opt/homebrew/opt/ruby/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin")

echo "==> frb codegen"
flutter_rust_bridge_codegen generate

echo "==> pod install（净化环境，强制 Homebrew pod ≥1.15）"
(cd macos && "${POD_ENV[@]}" pod install)

echo "==> flutter build macos --release"
# flutter build 内部也会调 pod（CocoaPods 集成），同样需要避开 rvm pod 1.11.3；
# flutter 本体路径需保留（FLUTTER_ROOT 由调用环境继承）
FLUTTER_BIN="$(command -v flutter)"
PATH="/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin" \
GEM_HOME="/opt/homebrew/Cellar/cocoapods/1.17.0_1/libexec" \
GEM_PATH="/opt/homebrew/Cellar/cocoapods/1.17.0_1/libexec" \
  "$FLUTTER_BIN" build macos --release

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
