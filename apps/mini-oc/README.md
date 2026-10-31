# mini-oc 桌面客户端

Flutter + Rust FFI 桌面客户端（macOS / Windows）。在自己的电脑上拉起 `opencode serve`，并通过 rathole 隧道暴露给 Web 门户，从而在手机或另一台电脑上远程查看项目、会话并接管终端。

配套的服务端见仓库根 [README](../README.md)。

---

## 功能

| 页签 | 能力 |
|---|---|
| **概览** | 服务开关（本地服务 / 隧道 / 全部）、运行状态（进程 PID、端口、模式）、账户信息与设备绑定状态 |
| **会话** | 项目 → 会话两级浏览，标题/路径检索，点击会话在系统终端新窗口打开 opencode TUI |
| **日志** | 实时滚动日志流，按来源（opencode / rathole / system）与级别过滤，全屏与复制 |
| **设置** | 服务器地址、端口偏好、opencode 二进制路径、设备上报状态与手动上报 |

### 服务模式状态机

客户端用一个状态机编排 `opencode serve` 与 `rathole` 两个子进程：

```text
Off ──启动本地──► LocalOnly(serve)
LocalOnly ──启动全部──► Cloud(serve + rathole)
Cloud ──停止隧道──► LocalOnly
LocalOnly ──启动隧道──► TunnelOnly(rathole)
TunnelOnly ──启动全部──► Cloud
任意 ──停止/崩溃──► Off
```

进程崩溃会被后台监督循环捕获：rathole 崩溃降级为 `LocalOnly`（保住本地服务），serve 崩溃则整体落到 `Off` 并记录错误原因，UI 通过状态流实时感知。

---

## 技术栈

| 层 | 选型 |
|---|---|
| UI | Flutter 3.47 / Dart 3.13，Material 3 + 亮暗主题，宽屏（≥900px）用 NavigationRail、窄屏用 NavigationBar |
| 状态管理 | Riverpod（`flutter_riverpod` 3.x） |
| 路由 | `go_router`，三级守卫：未登录 → 登录页；登录流程内页面放行 |
| 桥接 | flutter_rust_bridge 2.11.1（类型安全绑定 + 流式推送） |
| 核心 | Rust crate `rust_lib_mini_oc`（独立 crate，被 workspace `exclude`） |
| HTTP | reqwest + rustls |
| 进程 | `std::process` + 平台终止策略（Unix SIGTERM→SIGKILL；Windows `taskkill /T /F`） |

### 目录布局

```text
apps/mini-oc/
├── lib/                    # Dart UI
│   ├── main.dart           # 入口：RustLib 初始化、路由、关窗优雅停机
│   ├── shell.dart          # 自适应导航外壳
│   ├── stores.dart         # Riverpod 状态（认证、服务、配置、上报）
│   ├── theme.dart          # Material 3 主题
│   ├── pages/              # 登录 / 概览 / 会话 / 日志 / 设置
│   └── src/rust/           # frb 生成的 Dart 绑定（自动生成，勿手改）
├── rust/                   # Rust core crate
│   ├── src/
│   │   ├── api/            # frb 入口（crate::api 是唯一暴露命名空间）
│   │   ├── account.rs      # 登录 / 绑定 / 上报注册
│   │   ├── config.rs       # 本地配置持久化
│   │   ├── supervisor.rs   # 进程监督状态机
│   │   ├── rathole.rs      # rathole 配置生成与二进制定位
│   │   ├── portkill.rs     # 端口占用查询与终止
│   │   ├── term_spawn.rs   # 系统终端启动器
│   │   ├── logbuf.rs       # 环形日志缓冲
│   │   ├── runtime.rs      # 共享 tokio runtime
│   │   └── frb_generated.rs
│   └── tests/              # Rust 单元与集成测试
├── assets/rathole/bin/     # 随包分发的 rathole 二进制
├── rust_builder/           # cargokit / frb 集成（构建桥接层）
├── scripts/                # 打包脚本
├── macos/  windows/        # 平台 runner
└── test/                   # Dart 组件测试
```

> `lib/src/rust/` 是 flutter_rust_bridge 生成的代码，`cargo expand` 后重新生成覆盖，不要手工编辑。

---

## 开发

### 环境依赖

- Flutter SDK（stable 3.47+）
- Rust（rustup，macOS / Windows 各自的工具链）
- `cargo install flutter_rust_bridge_codegen --version 2.11.1`
- macOS 另需 Xcode 与 Homebrew CocoaPods（≥1.15）
- `opencode` 与 `rathole` 二进制在 PATH 中，或用 `OPENCODE_BIN` / `RATHOLE_BIN` 指定

### 常规流程

```bash
cd apps/mini-oc

# 1) 生成 frb 绑定（改了 rust/src/api 下的接口后必须重跑）
flutter_rust_bridge_codegen generate

# 2) 跑起来
flutter run -d macos      # 或 -d windows
```

### 测试

```bash
cd apps/mini-oc && flutter test        # Dart 组件测试（test/）
cd apps/mini-oc/rust && cargo test     # Rust core 测试
```

Dart 测试通过 `test/helpers/fake_rust.dart` 注入假的 Rust 桥接，不依赖真实网络与磁盘。

### 静态检查

```bash
cd apps/mini-oc && flutter analyze
```

---

## 打包

### macOS

```bash
cd apps/mini-oc && ./scripts/build-macos.sh
```

脚本会依次执行 frb 代码生成、`pod install`、`flutter build macos --release`，并把 rathole 二进制打包进 `.app` 的 `Contents/Resources/assets/rathole/bin/`。产物：

```text
apps/mini-oc/build/macos/Build/Products/Release/mini_oc.app
```

> 脚本刻意用净化环境（`env -i`）调 `pod`：macOS 上 rvm 自带的 pod 1.11 会抢占 PATH 并报 "unknown ISA"，必须强制走 Homebrew CocoaPods。

### Windows

见 [`scripts/build-windows.md`](scripts/build-windows.md)：需要 VS Build Tools（C++ 桌面开发负载）+ msvc 工具链，产物在 `build/windows/x64/runner/Release/`，rathole 二进制放到与 exe 同级的 `assets/rathole/bin/windows-x86_64/`。

### rathole 二进制定位顺序

客户端按以下顺序找 `rathole`（`rust/src/rathole.rs`）：

1. `RATHOLE_BIN` 环境变量；
2. macOS `.app` bundle 的 `Contents/Resources/assets/rathole/bin/<os>-<arch>/`；
3. exe 同级目录的 `assets/rathole/bin/<os>-<arch>/`（随包分发布局）；
4. 开发模式：源码树向上五级找 `apps/mini-oc/assets/rathole/bin/`；
5. PATH 上的 `rathole`。

---

## 本地数据

| 文件 | 位置 | 内容 |
|---|---|---|
| `config.json` | `MINI_OC_HOME` 或平台数据目录 | 服务器地址、用户信息、绑定设备、端口偏好、opencode 路径、device_id |
| `.env` | 同上 | `SERVER_URL` / `REMEMBER_PASSWORD` / `PASSWORD`（勾选记住密码时才有密码） |
| `rathole/settings/global.toml` | 同上 | 自动生成的 rathole 客户端配置（内含 `user.key`，权限 0600） |

平台数据目录：macOS `~/Library/Application Support/mini-oc`，Windows `%APPDATA%\mini-oc`。`MINI_OC_HOME` 可覆盖（测试隔离用）。

`config.json` 与 `.env` 均以「临时文件 + rename」原子替换写入，权限 0600。

---

## 凭据安全契约

`user.key` 是全链路唯一凭据，客户端有几条硬性约束：

- **命令行绝不含 key**。`opencode serve` 的密码经 `OPENCODE_SERVER_PASSWORD` 环境变量注入；终端启动器的脚本内容同样不含 key。
- **macOS 终端**用 `launchd` 会话不继承进程 env，凭据经 0600 权限的临时 env 文件传递，启动器脚本只 `source` 引用；过期文件会被回收。
- **日志脱敏**。Rust 侧记录 key 时只显示首尾各 4 位。
- **进程树终止**。Windows 用 `taskkill /T /F`，避免子进程成为孤儿占住端口。

---

## 已知约束

- **端口占用**：`opencode serve` 启动前会检测端口占用，报错带 `PORT_IN_USE:` 前缀。UI 识别该前缀后弹出对话框，可查看占用进程并一键终止后重试。
- **服务端地址默认**为 `https://oc.isoops.com`，可在设置页修改。
- **只支持 macOS / Windows**。Android / iOS / Linux 不在本期范围，架构上预留但未实现。
- **退出即停机**：点窗口关闭按钮或 ⌘Q 都会走优雅停机流程（最长 6 秒）后真正退出，防止留下孤儿进程占端口。
