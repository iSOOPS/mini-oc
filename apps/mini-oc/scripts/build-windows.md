# Windows 构建指南（在 Windows 机器上执行）

## 前置

- Flutter SDK（stable）+ Windows 10+
- Rust（rustup，msvc toolchain）+ VS Build Tools（C++ 桌面开发负载）
- `cargo install flutter_rust_bridge_codegen --version 2.11.1`

## 步骤

```powershell
cd apps/mini-oc
flutter_rust_bridge_codegen generate
flutter build windows --release
```

产物：`build/windows/x64/runner/Release/`（整个目录分发）。

## 打包 rathole

将 `assets/rathole/bin/windows-x86_64/rathole.exe` 与 `settings/` 复制到
`build/windows/x64/runner/Release/assets/rathole/bin/windows-x86_64/`（与 exe 同级的 assets 目录，
二进制定位逻辑按 exe 同目录 → 向上五级搜索 assets）。

或直接分发整个仓库 `apps/mini-oc/` 目录结构（定位逻辑两种都支持）。

## 安装器（可选）

用 [Inno Setup 6] 制作安装包，参考 FlClash 的分发方式：
`[Files]` 段包含 Release 目录 + assets/rathole 目录。
