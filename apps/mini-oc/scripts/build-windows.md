# Windows 构建指南（在 Windows 机器上执行）

## 前置

- Windows 10/11
- Flutter SDK（stable，含 Windows 桌面支持）
- Rust（rustup，`stable-x86_64-pc-windows-msvc`）+ VS Build Tools（**C++ 桌面开发**负载，msvc 工具链）
- `cargo install flutter_rust_bridge_codegen --version 2.11.1`

> **开发者模式**：Windows 桌面构建需创建插件符号链接。若未开启开发者模式
> （且不以管理员身份运行），`flutter build windows` 会报
> `Building with plugins requires symlink support`。两种解法：① 开启开发者模式
> （设置 → 隐私与安全 → 开发者选项，或管理员执行
> `reg add "HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock" /t REG_DWORD /f /v AllowDevelopmentWithoutDevLicense /d 1`）；
> ② 用 **junction 替代符号链接**：`flutter pub get` 后读取
> `.flutter-plugins-dependencies`，对每个 windows 插件执行
> `New-Item -ItemType Junction -Path windows\flutter\ephemeral\.plugin_symlinks\<name> -Target <path>`
> （junction 无需管理员，Flutter 会检测到「已存在且指向正确目标」而跳过符号链接创建）。

## 一键脚本

```powershell
cd apps/mini-oc
.\scripts\build-windows.ps1
```

脚本依次执行：`flutter_rust_bridge_codegen generate` → `flutter build windows --release` →
把 `assets/rathole/bin/windows-x86_64/rathole.exe` 复制到 Release 目录的
`assets/rathole/bin/windows-x86_64/`（与 exe 同级，二进制定位逻辑按 exe 同目录 → 向上五级搜索）。

产物：`build/windows/x64/runner/Release/`（**整个目录分发**）。

## 手动步骤（等价于脚本）

```powershell
cd apps/mini-oc
flutter_rust_bridge_codegen generate
flutter build windows --release

# 打包 rathole 到 Release 目录（与 exe 同级）
$dst = "build/windows/x64/runner/Release/assets/rathole/bin/windows-x86_64"
New-Item -ItemType Directory -Force -Path $dst | Out-Null
Copy-Item "assets/rathole/bin/windows-x86_64/rathole.exe" $dst
```

> rathole 的 `settings/global.toml` 由客户端**运行时**按设备清单自动生成到配置目录
> （`%APPDATA%\mini-oc\rathole\settings\global.toml`），无需随包预置。

## 安装器（可选）

用 [Inno Setup 6] 制作安装包（参考 FlClash 的分发方式）：`[Files]` 段包含
`Release` 目录整个内容即可（`assets/rathole/bin/windows-x86_64/rathole.exe` 已由脚本放入）。