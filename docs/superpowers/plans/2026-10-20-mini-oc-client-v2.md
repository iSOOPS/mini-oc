# mini-oc V2 桌面客户端实施计划（计划 2/2）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 构建 Flutter + Rust FFI 桌面客户端（macOS/Windows）：登录门控、云/本地服务编排（opencode serve + rathole）、日志流、项目/会话检索与系统终端跳转，FlClash 级 Material 3 丝滑 UI。

**Architecture:** Flutter UI 层经 flutter_rust_bridge v2 调用 Rust core（进程监督/账户/日志/终端启动）。core 复用计划 1 的 `mini-oc-core`（模型、OcApi client、隧道拼装）。凭据统一 `user.key`，全程 env 注入不进命令行。

**Tech Stack:** Flutter (Dart 3, Material 3) + flutter_rust_bridge v2 / Rust (tokio, reqwest+rustls) / rathole 官方二进制 bundle。

**Spec:** `docs/superpowers/specs/2026-10-20-mini-oc-v2-design.md`（先读 §2 实测事实、§5 客户端设计、§4.1 凭据表）

**前置依赖:** 计划 1（`docs/superpowers/plans/2026-10-20-mini-oc-web-v2.md`）已完成——需要 `mini-oc-core` crate 与 web 端 `/api/user/info`、`/api/device-bind` 接口联调。

**环境要求:** Flutter SDK ≥3.22（stable）、Rust（同 workspace）、macOS 构建需 Xcode；Windows 构建仅在 Windows 环境执行（本计划 macOS 优先，Windows 出清单文档）。

## Global Constraints

- **全部功能强制登录**：未登录仅登录页；云服务另需已绑定设备（spec §4.1、§5.2）；
- **凭据不泄露**：`user.key` 只经 env 传子进程（`OPENCODE_SERVER_PASSWORD` / `OPENCODE_PASSWORD`），绝不进命令行参数、日志、错误消息；
- **云服务端口以设备清单为准**（§5.2.1）：绑定设备的 port 强制覆盖本地偏好，冲突时以清单端口重启 serve 并 UI 提示一次；
- Rust 侧 TLS 用 rustls；进程终止：Unix SIGTERM→5s→SIGKILL，Windows `taskkill /T /F`（移植 V1 `apps 路径：/Users/samuel/Documents/GitForAi/mini-oc-gui/src/serve/supervisor.rs` 的已验证逻辑）；
- Flutter 端状态管理用 Riverpod；页面文件 ≤400 行，超限拆 widget；
- 中文代码注释；commit conventional（feat/fix/test/docs/chore）；
- macOS 最低 12 / Windows 10+；不做 Linux。

## Review Focus

1. **opencode serve 意外崩溃**（被外部 kill / 端口被抢）：UI 状态必须从 running 转入异常态并给出重启按钮，不得假死显示运行中。→ Task 4 崩溃探测测试。
2. **服务运行中退出应用**：必须先优雅终止 serve + rathole（防孤儿进程占端口）。→ Task 4 退出钩子测试。
3. **未绑定设备点云服务开关**：置灰 + 引导绑定，不得启动任何进程。→ Task 9 门控 widget 测试。
4. **终端启动凭据泄露**：生成的 osascript/pwsh 脚本与命令行字符串中不得出现 key 明文（只经 env）。→ Task 7 脚本内容快照测试。
5. **端口冲突**（偏好端口/清单端口被占）：给出明确错误 + 可选「强制清理」（杀占用进程），不得 panic。→ Task 4 端口占用测试。

---

### Task 1: Flutter 项目骨架 + frb 集成

**Files:**
- Create: `apps/mini-oc/`（`flutter create --platforms=macos,windows` 产物 + `pubspec.yaml` + `rust/` crate `mini-oc-app` + `rust_builder/`）
- Modify: 根 `Cargo.toml`（members 加 `apps/mini-oc/rust`）

**Interfaces:**
- Produces: `flutter run -d macos` 显示占位页；`rust/src/api/simple.rs` 的 `pub fn ping() -> String` 经 frb 在 UI 显示 "pong"（桥接链路证明）。

- [ ] **Step 1: `flutter create` + 依赖**（flutter_rust_bridge、flutter_rust_bridge_codegen dev、riverpod、window_manager）
- [ ] **Step 2: rust crate 骨架 + frb codegen 跑通**（`flutter_rust_bridge_codegen integrate` 标准布局；rust/Cargo.toml 加 `mini-oc-core = { path = "../../../crates/mini-oc-core" }`）
- [ ] **Step 3: ping/pong 端到端验证**（macos debug 运行，页面显示 pong）
- [ ] **Step 4: Commit** `feat(app): flutter skeleton with frb bridge`

---

### Task 2: core 配置持久化 + 账户模块

**Files:**
- Create: `apps/mini-oc/rust/src/{config.rs,account.rs,api.rs}`
- Test: `apps/mini-oc/rust/tests/{config_test.rs,account_test.rs}`（account 用 wiremock 伪 web 服务）

**Interfaces:**
- Consumes: `mini-oc-core`（PortalUser/UserDevice serde）
- Produces（frb 暴露，Dart 侧驼峰自动映射）:
  - `pub struct ClientConfig { server_url, user: Option<StoredUser{id,name,key}>, bound_device: Option<String>, port: u16, opencode_bin: Option<String>, theme: String }`
  - `pub fn load_config() -> ClientConfig` / `pub fn save_config(cfg: &ClientConfig) -> Result<(), String>`（应用数据目录 `mini-oc/config.json`，Unix 0600）
  - `pub async fn login(server_url: String, key: String) -> Result<AccountInfo, String>`：`POST {server}/api/user/info` → 存 config；`AccountInfo { id, name, devices: Vec<DeviceBrief{name,port,pctype,bound,device_name}> }`
  - `pub async fn bind_device(device_name: String, pctype: String, os: String) -> Result<(), String>`：`POST /api/device-bind`；成功写 `bound_device`；**403 时返回带"平台不匹配"语义的错误文案**
  - `pub fn logout() -> Result<(), String>`（清 user/bound_device，保留 server_url/port）

- [ ] **Step 1: 失败测试**——config 存取往返/权限位；login 成功存 key；login 网络失败不污染已有 config；bind 403 文案
- [ ] **Step 2: 验证 FAIL**
- [ ] **Step 3: 实现**（HTTP 出站统一走 `http_trace` 式日志截断脱敏——key 打码）
- [ ] **Step 4: PASS**
- [ ] **Step 5: Commit** `feat(app): config persistence and account module`

---

### Task 3: rathole toml 生成 + 二进制解析

**Files:**
- Create: `apps/mini-oc/rust/src/rathole.rs`
- Create: `apps/mini-oc/assets/rathole/bin/`（`macos-aarch64/`、`windows-x86_64/`，从 V1 `mini-oc-gui/rathole/bin/` 复制官方二进制）
- Test: `apps/mini-oc/rust/tests/rathole_test.rs`

**Interfaces:**
- Consumes: `mini-oc-core::tunnel::remote_addr_for_rathole`、账户模块 `AccountInfo`
- Produces:
  - `pub fn rathole_toml(device: &DeviceBrief, user_key: &str) -> Result<String, String>`——按 spec §5.3.1 契约生成（单 service，name 原样，token=user.key，local_addr=127.0.0.1:port）
  - `pub fn resolve_rathole_bin() -> Option<PathBuf>`——assets 目录（exe 同目录 `assets/rathole/bin/<os>-<arch>/rathole[.exe]`）→ `RATHOLE_BIN` env → PATH
- [ ] **Step 1: 失败测试**——toml 黄金字符串比对（含 remote_addr 两形态）；bin 解析优先级（env 覆盖）
- [ ] **Step 2: 验证 FAIL**
- [ ] **Step 3: 实现**（toml 用 `toml = "0.8"` 序列化而非字符串拼接）
- [ ] **Step 4: PASS**
- [ ] **Step 5: Commit** `feat(app): rathole config generation and binary resolution`

---

### Task 4: 进程监督（serve + rathole 编排状态机）

**Files:**
- Create: `apps/mini-oc/rust/src/supervisor.rs`、`apps/mini-oc/rust/src/ocproc.rs`
- Test: `apps/mini-oc/rust/tests/supervisor_test.rs`

**Interfaces:**
- Consumes: Task 2 config、Task 3 rathole
- Produces:
  - `pub enum ServiceMode { Off, LocalOnly, Cloud }`
  - `pub struct ServiceState { mode: ServiceMode, serve_pid: Option<u32>, rathole_pid: Option<u32>, port: u16, uptime_started: Option<i64>, last_error: Option<String> }`
  - `pub async fn start_local() -> Result<ServiceState, String>`：解析 opencode bin（`OPENCODE_BIN` env → config.opencode_bin → PATH 探测）→ 端口（绑定设备=清单 port，否则偏好 port；冲突检测 + `kill_port_listener` 可选清理）→ spawn `opencode serve --port N` env `OPENCODE_SERVER_PASSWORD=<key>` → `/api/info` 存活探测（10s）
  - `pub async fn start_cloud() -> ...`：start_local 成功后生成 toml → spawn rathole → 1.5s 存活 + 日志首条连接确认；**端口规则**：清单 port ≠ 当前运行端口时先重启 serve（spec §5.2.1）
  - `pub async fn stop_all() -> ...`；降级 `cloud → local`（只停 rathole）
  - 后台监督 task：1s 轮询子进程存活，崩溃 → 更新 ServiceState.last_error + mode 降 Off（UI 可感知）；**应用退出钩子**（Flutter `AppLifecycleListener`/window_manager close 事件 → rust `stop_all`）
  - `pub fn subscribe_state(sink: StreamSink<ServiceState>)`（状态变化推流）

- [ ] **Step 1: 失败测试**——用 `/bin/sleep` 与伪脚本替身测：启停状态机转换；kill 子进程后 2s 内 StateStream 收到 Off+error；端口占用返回 Err 含端口信息；`taskkill`/SIGKILL 路径平台各自验证
- [ ] **Step 2: 验证 FAIL**
- [ ] **Step 3: 实现**（移植 V1 supervisor：`spawn_traced` 逐行日志、Windows `CREATE_NO_WINDOW`、两轮端口扫描）
- [ ] **Step 4: PASS**
- [ ] **Step 5: Commit** `feat(app): service supervisor state machine`

---

### Task 5: 日志缓冲 + 流

**Files:**
- Create: `apps/mini-oc/rust/src/logbuf.rs`
- Modify: `apps/mini-oc/rust/src/api.rs`
- Test: `apps/mini-oc/rust/tests/logbuf_test.rs`

**Interfaces:**
- Produces:
  - `pub struct LogLine { ts_ms: i64, source: String /* opencode|rathole|system */, level: String, message: String }`
  - `pub fn subscribe_logs(sink: StreamSink<LogLine>)`（frb 流）；`pub fn recent_logs(n: usize) -> Vec<LogLine>`（环形 2000 行，`Arc<Mutex<VecDeque>>`）；`pub fn clear_logs()`
  - supervisor/账户/HTTP trace 全部汇入 logbuf；**推送节流**：100ms 窗口批量 flush（不逐行推 Dart）
- [ ] **Step 1: 失败测试**——环形上限截断；来源标记；节流批量（时间伪造或注入 clock）
- [ ] **Step 2: 验证 FAIL**
- [ ] **Step 3: 实现**
- [ ] **Step 4: PASS**
- [ ] **Step 5: Commit** `feat(app): ring log buffer with throttled stream`

---

### Task 6: 项目/会话查询 + 终端启动器

**Files:**
- Create: `apps/mini-oc/rust/src/term_spawn.rs`
- Modify: `apps/mini-oc/rust/src/api.rs`
- Test: `apps/mini-oc/rust/tests/term_spawn_test.rs`

**Interfaces:**
- Consumes: `mini-oc-core::OcApi`（连本地 serve，password=user.key）
- Produces:
  - `pub async fn list_projects() -> Result<Vec<ProjectSummary>, String>`；`pub async fn list_sessions(directory: String) -> Result<Vec<SessionSummary>, String>`（frb 结构：ProjectSummary{id,name,path,vcs}、SessionSummary{id,title,directory,updated_ms,outcome}）
  - `pub fn open_in_terminal(directory: String, session_id: Option<String>) -> Result<(), String>`：
    - 命令：`OPENCODE_PASSWORD=<key> opencode --server http://127.0.0.1:<port> [--session <id>] <directory>`
    - macOS：osascript——iTerm2 运行中→`tell application "iTerm2"` 新 tab `write text`；否则 Terminal.app `do script`；launcher.sh `exec`（复用 V1 `attach.rs` 已验证模板）
    - Windows：临时 launcher.ps1（只读 `$env:`，首行写 `$PID` 到 pidfile）→ `cmd /c start "mini-oc" pwsh -NoExit -File`
- [ ] **Step 1: 失败测试**——**生成的脚本内容快照测试**：断言脚本文本与命令行数组中**不含 key 明文**（只经 env）；session 有/无两形态参数正确；pidfile 路径规则
- [ ] **Step 2: 验证 FAIL**
- [ ] **Step 3: 实现**（项目/会话查询直连 core OcApi；macOS 手动验收放 Task 12）
- [ ] **Step 4: PASS**
- [ ] **Step 5: Commit** `feat(app): terminal spawner without credential leakage`

---

### Task 7: Flutter 主题 + 导航骨架 + 登录门控

**Files:**
- Create: `apps/mini-oc/lib/{main.dart,theme.dart,router.dart}`、`lib/stores/{auth_store.dart,service_store.dart,config_store.dart}`、`lib/pages/login_page.dart`、`lib/shell.dart`
- Test: `apps/mini-oc/test/router_gate_test.dart`

**Interfaces:**
- Consumes: Task 2 frb API
- Produces: Material 3 主题（`ColorScheme.fromSeed` 动态色 + 亮暗跟随系统 + 连续圆角 16/24）、自适应 Shell（宽≥900 NavigationRail 否则 NavigationBar，四入口：仪表盘/项目会话/日志/设置）、GoRouter 守卫：未登录 redirect `/login`

- [ ] **Step 1: 失败 widget 测试**——未登录态进任何页 redirect 登录页；登录后（store mock）可进 Shell；窄/宽断点渲染 Rail/Bar
- [ ] **Step 2: 验证 FAIL**
- [ ] **Step 3: 实现**（登录页：服务器地址 + 密钥输入 + 登录按钮 + 错误提示卡；成功→bind_device 设备选择弹层（多设备时））
- [ ] **Step 4: PASS**
- [ ] **Step 5: Commit** `feat(ui): material3 shell with login gate`

---

### Task 8: 仪表盘页（服务控制）

**Files:**
- Create: `lib/pages/home_page.dart`、`lib/widgets/{service_switch_card.dart,status_card.dart,account_card.dart}`
- Test: `apps/mini-oc/test/home_gate_test.dart`

**Interfaces:**
- Consumes: Task 4/5 frb（ServiceState 流、start/stop）
- Produces:
  - 「启动云服务」「启动本地服务」双开关卡（FlClash 大开关交互：开关切换动画、启动中 loading 态）
  - 状态卡：进程 PID/端口/模式/已运行时长（Timer 每秒刷新）/last_error 红条 + 重试按钮
  - 账户卡：用户名/编号/绑定设备名/退出登录
  - **门控**：未绑定设备时云服务开关置灰 + 「去绑定」引导（点击弹绑定流程）

- [ ] **Step 1: 失败 widget 测试**——未绑定设备（store mock）云开关 disabled + 引导可见；StateStream 推 Off+error 后显示错误条；启动中开关 loading 不可再点
- [ ] **Step 2: 验证 FAIL**
- [ ] **Step 3: 实现**
- [ ] **Step 4: PASS**
- [ ] **Step 5: Commit** `feat(ui): dashboard with service switches`

---

### Task 9: 项目/会话页

**Files:**
- Create: `lib/pages/sessions_page.dart`、`lib/widgets/{project_tile.dart,session_tile.dart}`
- Test: `apps/mini-oc/test/sessions_page_test.dart`

**Interfaces:**
- Consumes: Task 6 frb
- Produces: 项目列表（名称/路径/vcs 徽标/活跃时间）→ 展开加载会话（标题/模型/outcome 徽标/token 数/更新时间）；顶部检索框本地模糊过滤（标题+路径）；点击会话/项目 → `open_in_terminal`；serve 未运行时整页引导「先启动服务」+ 刷新按钮；加载失败显示错误卡

- [ ] **Step 1: 失败 widget 测试**——serve Off 态引导渲染；检索过滤命中；点击 tile 调用 bridge mock 的 open_in_terminal（参数断言）
- [ ] **Step 2: 验证 FAIL**
- [ ] **Step 3: 实现**
- [ ] **Step 4: PASS**
- [ ] **Step 5: Commit** `feat(ui): projects and sessions browser`

---

### Task 10: 日志页 + 设置页

**Files:**
- Create: `lib/pages/{logs_page.dart,settings_page.dart}`、`lib/widgets/log_viewer.dart`
- Test: `apps/mini-oc/test/logs_page_test.dart`

**Interfaces:**
- Consumes: Task 5 frb 日志流
- Produces:
  - 日志页：实时滚动（用户上滚时暂停自动跟随，回底部恢复）、来源过滤 chips（全部/opencode/rathole/system）、全屏切换（含系统状态栏沉浸）、`SelectionArea` 选择复制 + 「复制全部」按钮、清空
  - 设置页：服务器地址（改后需重新登录提示）、opencode 二进制路径覆盖、本地偏好端口（提示绑定设备后以清单为准）、主题覆盖（系统/亮/暗）；「关于」版本卡

- [ ] **Step 1: 失败 widget 测试**——来源过滤后列表仅含选中 source；全屏切换后 AppBar 隐藏；1000 行日志构建 < 500ms（虚拟列表性能）
- [ ] **Step 2: 验证 FAIL**
- [ ] **Step 3: 实现**
- [ ] **Step 4: PASS**
- [ ] **Step 5: Commit** `feat(ui): logs viewer and settings`

---

### Task 11: 应用退出清理 + 构建打包

**Files:**
- Modify: `apps/mini-oc/lib/main.dart`（生命周期钩子）
- Create: `apps/mini-oc/scripts/{build-macos.sh,build-windows.md}`、`apps/mini-oc/macos/Runner` 图标资源（从 V1 `mini-oc-gui/assets/icon.svg` 复用生成）

**Interfaces:**
- Produces: 退出应用（窗口关闭）自动 `stop_all`；`flutter build macos --release` 产物内嵌 rathole bundle；Windows 构建文档（CI/跨机构建说明 + 本机 MSYS2 MinGW 说明）

- [ ] **Step 1: 实现退出钩子**（windowManager onWindowClose → rust stop_all → destroy）
- [ ] **Step 2: macOS release 构建验证**（产物包含 rust dylib + rathole；`flutter build macos --release` 成功；应用可启动）
- [ ] **Step 3: rathole bundle 打包脚本**（构建后拷贝 assets/rathole 到 .app Resources；Windows 为文档指引）
- [ ] **Step 4: Commit** `feat(app): exit cleanup and packaging`

---

### Task 12: 端到端验收（macOS 实测）

**Files:**
- Create: `docs/superpowers/plans/2026-10-20-e2e-checklist.md`

- [ ] **Step 1: 本地链路**：登录（对运行中的 mini-oc-web）→ 启动本地服务 → 项目/会话列表出数据（需本机有 opencode 会话历史）→ 点击会话开出终端窗口且直达会话
- [ ] **Step 2: 云链路**：绑定设备 → 启动云服务 → rathole 连接成功（服务器侧确认）→ 手机浏览器经 mini-oc-web 看到设备 online → 移动端跳转进 opencode Web UI
- [ ] **Step 3: 异常链路**：serve 被 kill 的 UI 恢复；端口占用清理；退出应用无孤儿进程（`pgrep opencode` 验证）
- [ ] **Step 4: Windows 手动清单**（同 1-3，记录到 checklist 文档，标注「待 Windows 环境」项）
- [ ] **Step 5: Commit** `docs: e2e checklist`
