# mini-oc Windows release 打包
# 前置：Flutter SDK（stable）+ Rust（rustup，msvc toolchain，需 VS Build Tools「C++ 桌面开发」负载）
#       + `cargo install flutter_rust_bridge_codegen --version 2.11.1`
# 产物：build/windows/x64/runner/Release/（整个目录分发；含 rathole bundle）
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

# 可选：国内镜像加速
if (-not $env:PUB_HOSTED_URL) { $env:PUB_HOSTED_URL = "https://pub.flutter-io.cn" }
if (-not $env:FLUTTER_STORAGE_BASE_URL) { $env:FLUTTER_STORAGE_BASE_URL = "https://storage.flutter-io.cn" }

Write-Host "==> frb codegen"
flutter_rust_bridge_codegen generate
if ($LASTEXITCODE -ne 0) { throw "frb codegen 失败" }

Write-Host "==> flutter build windows --release"
flutter build windows --release
if ($LASTEXITCODE -ne 0) { throw "flutter build windows 失败" }

$releaseDir = "build/windows/x64/runner/Release"
if (-not (Test-Path $releaseDir)) {
    throw "构建产物缺失：$releaseDir"
}

Write-Host "==> 打包 rathole bundle 到 Release 目录"
$src = "assets/rathole/bin/windows-x86_64/rathole.exe"
if (-not (Test-Path $src)) {
    throw "缺少 rathole Windows 二进制：$src（请先放入 assets/rathole/bin/windows-x86_64/）"
}
$dest = Join-Path $releaseDir "assets/rathole/bin/windows-x86_64"
New-Item -ItemType Directory -Force -Path $dest | Out-Null
Copy-Item $src $dest -Force

Write-Host "==> 完成：$releaseDir"
$size = (Get-ChildItem $releaseDir -Recurse -File | Measure-Object -Property Length -Sum).Sum / 1MB
Write-Host ("    发行目录大小约 {0:N1} MB" -f $size)