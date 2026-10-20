# mini-oc V2 重构设计文档

日期：2026-10-20
状态：待用户确认
上游需求：`重构指南.txt`（仓库根目录）

---

## 1. 意图与背景（Why）

### 1.1 为什么重构

现有两个项目构成了一套「远程访问本机 opencode 会话」的体系：

| 项目 | 形态 | 现状 |
|---|---|---|
| mini-oc-gui | ratatui 终端 TUI + 内嵌 axum（端口 9465）| **与 opencode v2 全面不兼容**（`opencode attach` 已移除、API 路径全变、用户名硬编码导致 401） |
| mini-oc-web | axum BFF + Vue3 SPA | 探测/代理逻辑依赖 V1 设备端 axum 服务与双端口模型，需随 V2 简化 |

opencode v2 的三个根本变化驱动本次重构：

1. **serve 内嵌官方 Web UI + 完整 v2 HTTP API**（`/api/project`、`/api/session`、`/api/pair` 等，OpenAPI 规范 143 操作）；
2. **鉴权模型变化**：用户名硬编码 `opencode`，密码由 `OPENCODE_SERVER_PASSWORD` 指定（已实测验证）；客户端连接凭据 env 为 `OPENCODE_PASSWORD`；
3. **`opencode attach` 移除**：替代形态是 `opencode --server <url> --session <id> <directory>`（TUI）或 serve 内嵌 Web UI。

### 1.2 重构后的形态

- **mini-oc**（本仓库 `apps/mini-oc`）：Flutter + Rust FFI 桌面客户端（macOS / Windows），完全参考 FlClash 的「Material You 设计 + Apple 式交互」架构与观感。版本号 V2。
- **mini-oc-web**（本仓库 `apps/mini-oc-web`）：Rust (axum) + Vue3 Web 服务，部署在服务器，**移动端优先、兼容 PC**。

两者共用一个 Cargo workspace 中的共享模型 crate。

---

## 2. 已实测验证的技术事实（本设计的基石）

以下全部在 macOS 本机对 `opencode 2.0.32` 实测验证通过：

| # | 事实 | 验证方式 |
|---|---|---|
| V1 | `opencode serve --port N` 启动 v2 API + Web UI；`/` 返回 Web UI HTML | curl |
| V2 | `OPENCODE_SERVER_PASSWORD=<pwd> opencode serve` → 密码生效，日志不再打印随机密码 | curl + basic auth 验证 |
| V3 | API 鉴权：Basic Auth，用户名必须 `opencode`；`OPENCODE_SERVER_USERNAME` 被 v2.0.32 忽略 | curl 各组合实测 + GitHub issue #45856 |
| V4 | `POST /api/pair` → `{code, expires_in:300}`；`GET /auth/connect/{code}` → `{token}`；**token 为 `exp.hmac` 格式，可直接作 Basic Auth 密码** | curl 链路实测 |
| V5 | Web UI 支持三种鉴权传递：Basic Auth header / `?auth_token=base64(opencode:pwd)` query / `opencode_session_<port>` cookie（即 pair token） | 二进制源码 strings 确认 |
| V6 | `/api/project` 返回 `Project[]`（id/canonical/vcs/name/time）；`/api/session?directory=` 返回 `{data: Session.Info[], cursor}`，结构与附A一致 | curl 实测 |
| V7 | 客户端（TUI/run）连接远程 server：`OPENCODE_PASSWORD=<pwd> opencode --server <url> [--session <id>] [dir]` | 二进制源码 + 文档 |
| V8 | serve 的静态 Web UI 资源无鉴权即可加载，`/api/*` 与 SSE 需鉴权 | curl |

---

## 3. 目标 / 非目标 / 反目标

### 3.1 目标

**mini-oc 客户端：**
1. FlClash 级别的桌面观感：Material 3 动态色、亮暗主题、连续圆角、自适应布局（宽屏 NavigationRail / 窄屏 NavigationBar）、丝滑过渡动画；
2. 一键「启动云服务」：opencode serve + rathole 双进程编排、监督、日志；
3. 一键「启动本地服务」：仅 opencode serve；
4. 日志页：可全屏、可复制、按进程来源过滤、实时滚动；
5. 项目/会话页：数据来自本地 opencode server 的 `/api/project` + `/api/session`（附A结构），支持检索，点击 → **系统终端新窗口**运行 `opencode <dir> --server <url> --session <id>`；
6. 登录体系：使用 mini-oc-web 创建的账户（用户密钥）登录，**全部功能强制登录**后方可使用（云服务另需绑定设备）；
7. macOS + Windows 双平台。

**mini-oc-web 服务端：**
1. 移动端优先的 Vue3 SPA（兼容 PC），保留超管/普通用户双层体系；
2. 设备清单按 V2 精简（见 §6.2），核心 rathole 业务交互保留；
3. 状态刷新逻辑重构：直接探测设备 opencode v2 API（经 rathole 隧道，Basic Auth `opencode:<user.key>`）；
4. 项目/会话实时来自设备 v2 API；离线时读 SB 快照（BFF 在设备在线时写入）；
5. 移动端会话跳转保留 pair-link 一次性链接机制；
6. 存储延续「SB 存储 md 为核心」。

### 3.2 非目标（本期不做）

- 不做 Flutter Web / Android / Linux 客户端（架构预留，不实现）；
- 不做 rathole **服务端**配置管理（服务端 toml / nginx 仍由管理员在系统外维护，与 V1 相同）；
- 不做 opencode 会话内容级操作（发消息、diff 查看等）——那是 opencode Web UI / TUI 自己的事，mini-oc-web 只做列表与跳转；
- 不做用户自注册（用户一律超管创建，与 V1 一致）；
- 不做数据自动迁移工具（V1 path-list 登记簿废弃，见 §6.4）。

### 3.3 反目标（做了即失败）

- **凭据泄露**：user.key 出现在命令行参数、URL referrer、日志明文中；
- **假数据混发**：离线陈旧快照不标注来源直接当实时数据展示（V1 的 source 标注机制必须保留）；
- **UI 卡顿掉帧**：日志高频刷新阻塞 UI 线程、页面切换无动画或闪烁；
- **双击即崩**：opencode/rathole 二进制缺失、端口被占、未登录就点云服务按钮——都要有明确的门控与引导，而非 panic。

---

## 4. 总体架构

```
┌──────────────────────────── 服务器 ────────────────────────────┐
│  mini-oc-web (axum :8100)          SilverBullet (:3000)        │
│  ┌──────────────────────┐          ┌──────────────────────┐    │
│  │ Vue3 SPA (移动优先)   │          │ web/opencode/        │    │
│  │ 超管管理 / 用户门户    │◄──HTTP──►│   config.md (用户表) │    │
│  │ BFF 代理 + 探测 + 跳转 │  sb.rs   │   snapshot/*.md      │    │
│  └─────────┬────────────┘          └──────────────────────┘    │
│            │  经 rathole 隧道 (Basic opencode:<user.key>)       │
│            │  {public_url}/{port}/api/*                         │
└────────────┼───────────────────────────────────────────────────┘
             │ rathole server (系统外人工维护, :8999)
             │
═════════════╪══════════ 隧道边界 ════════════════════════════════
             │
┌────────────┼───── 用户电脑 (macOS / Windows) ───────────────────┐
│  ┌─────────┴────────┐    ┌──────────────────────────────────┐  │
│  │ mini-oc V2       │    │ rathole client (自动生成 toml)    │  │
│  │ Flutter UI       │    │  token = user.key                │  │
│  │  ├ 登录/绑定      │    │  local 127.0.0.1:<port>          │  │
│  │  ├ 服务控制      │    └──────────────┬───────────────────┘  │
│  │  ├ 项目/会话      │                   │ 转发                  │
│  │  ├ 日志          │    ┌──────────────┴───────────────────┐  │
│  │  └ 设置          │    │ opencode serve --port <port>     │  │
│  │ Rust core (FFI)  │    │ OPENCODE_SERVER_PASSWORD=user.key│  │
│  │  ├ 进程监督       │    │ 内嵌 Web UI + v2 API             │  │
│  │  ├ 账户客户端     │    └──────────────────────────────────┘  │
│  │  ├ oc API client │                                        │
│  │  ├ 终端启动器     │──► 系统终端新窗口:                       │
│  │  └ 日志缓冲       │    OPENCODE_PASSWORD=<key> \            │
│  └──────────────────┘     opencode <dir> --server <url> ...    │
└────────────────────────────────────────────────────────────────┘
```

**关键简化（对比 V1）**：设备端从 3 个进程（opencode serve + axum TUI 服务 + rathole）减为 **2 个**（opencode serve + rathole）。探测/列表/跳转全部直连 opencode v2 API，设备端不再有自有 HTTP 服务。

### 4.1 凭据体系（重新设计后）

**user.key（32 位随机字符串）单凭据贯穿全链路：**

| 场景 | 用法 |
|---|---|
| 客户端登录 web | `POST /api/user/info {key}` |
| 设备 opencode serve 启动（本地/云服务皆同） | `OPENCODE_SERVER_PASSWORD=<user.key>` env 注入 |
| web BFF 经隧道调设备 API | Basic Auth `opencode:<user.key>` |
| rathole 客户端 token | `[client.services.<name>] token = "<user.key>"` |
| 终端 TUI 连接本地 serve | `OPENCODE_PASSWORD=<user.key>` env 注入（不进命令行） |
| 移动端会话跳转 | pair-link 一次性 token（由设备 `POST /api/pair` 签发） |

**全部功能强制登录**（用户确认）：客户端启动即要求登录，登录前仅有登录页；登录后本地服务、项目/会话、终端跳转、云服务全部可用（云服务另需已绑定设备）。凭据全程统一为 user.key，无本地自建密码。

超管：`SB_PASSWORD` env 登录（与 V1 相同）。V1 的用户级 SB 配置（UserSbConfig）**整体删除**——它的两个用途（rathole token 来源、设备端写 path-list）在 V2 均已消失。

### 4.2 仓库布局（monorepo，当前仓库）

```
mini-oc/
├── apps/
│   ├── mini-oc/               # Flutter + Rust 客户端
│   │   ├── lib/               # Flutter (Dart) UI
│   │   ├── rust/              # Rust core crate（flutter_rust_bridge）
│   │   ├── rust_builder/      # cargokit/flutter_rust_bridge 集成
│   │   ├── assets/            # 图标、rathole 二进制 bundle
│   │   │   └── rathole/bin/<os>-<arch>/rathole[.exe]
│   │   └── pubspec.yaml
│   └── mini-oc-web/           # axum + Vue3 服务端
│       ├── src/               # Rust BFF
│       ├── web/               # Vue3 SPA
│       └── tests/             # wiremock 集成测试
├── crates/
│   └── mini-oc-core/          # 共享 Rust 库
│       ├── 用户/设备数据模型（serde，带 V2 字段校验）
│       ├── opencode v2 API client（reqwest，Project/Session/info/pair）
│       └── 隧道地址拼装纯函数（含单测）
├── docs/superpowers/          # 本文档与后续 plan
├── Cargo.toml                 # workspace 根
└── 重构指南.txt
```

---

## 5. mini-oc 客户端详细设计

### 5.1 技术栈

| 层 | 选型 | 理由 |
|---|---|---|
| UI | Flutter (Dart)，Material 3 | 用户指定复刻 FlClash（Flutter 原生技术栈）；Material You + Apple 交互的成熟实现 |
| 桥接 | flutter_rust_bridge v2 | Rust↔Dart 类型安全绑定，流式日志/事件推送天然支持 |
| core | Rust（workspace 成员） | 进程管理、HTTP、配置——与 web 端共享模型 crate |
| HTTP | reqwest + rustls | 纯 Rust TLS（延续 V1 「无 C 编译器」约束；Windows 不引 schannel 版本坑） |
| 进程 | std::process + 平台特定终止（nix / taskkill） | 复用 V1 supervisor 的成熟逻辑（SIGTERM→SIGKILL、进程树击杀、两轮端口扫描） |

### 5.2 页面结构（FlClash 式导航）

```
Shell（NavigationRail 宽屏 / NavigationBar 窄屏，自适应断点 900px）
├── 首页（仪表盘）
│   ├── 账户卡片：用户名/编号、设备绑定状态、登录/退出
│   ├── 服务开关卡片：「启动云服务」「启动本地服务」双开关（FlClash 的大开关交互）
│   └── 状态卡：opencode/rathole 进程状态、端口、隧道地址、已连时长
├── 项目 / 会话页
│   ├── 顶部检索框（标题/路径模糊过滤，本地过滤）
│   ├── 项目列表（名称、路径、vcs 徽标、最近活跃时间）
│   └── 展开项目 → 会话列表（标题、模型、outcome 徽标、token 用量、更新时间）
│       └── 点击会话 → 系统终端新窗口（§5.5）
├── 日志页
│   ├── 实时滚动流（按来源过滤：opencode / rathole / system）
│   ├── 全屏模式（含系统状态栏沉浸）
│   └── 长按/拖选复制（Flutter SelectableRegion），一键复制全部
└── 设置页
    ├── 服务器地址（mini-oc-web URL）
    ├── opencode 二进制路径（自动探测 + 手动覆盖）
    ├── 端口偏好、开机自启（可选后置）
    └── 主题：动态色 / 跟随系统亮暗
```

未登录时的门控：**全部功能强制登录**——应用启动即进入登录页，登录成功后方可进入主界面。「启动云服务」开关与隧道信息另需**已绑定设备**（置灰 + 引导提示）；「启动本地服务」与项目/会话/终端跳转登录即可用。

### 5.2.1 端口规则（用户确认：云服务以清单为准）

| 场景 | 端口 |
|---|---|
| 纯本地服务（未绑定设备） | 本地偏好端口（默认 9464，可改） |
| 云服务 / 已绑定设备 | **设备清单下发的 `port`（强制）**——与运维侧 rathole 服务端配置对齐 |
| 启动云服务时偏好端口 ≠ 清单 port | 以清单 port 为准重启 serve，UI 提示一次 |

### 5.3 Rust core 模块

```
rust/src/
├── api.rs          # flutter_rust_bridge 入口（所有暴露给 Dart 的函数/流）
├── account.rs      # 登录 /api/user/info、绑定 /api/device-bind、本地凭据存取
├── supervisor.rs   # ChildProcess、launch/stop/kill_port_listener（移植 V1 并简化）
├── oc_client.rs    # 本地 opencode v2 API client（/api/info /api/project /api/session /api/pair）
├── rathole.rs      # toml 生成（设备清单→单 service：<name> 转发 opencode serve 端口）
├── term_spawn.rs   # 终端启动器（osascript iTerm2/Terminal、cmd start pwsh）
├── logbuf.rs       # 环形日志缓冲（VecDeque 2000 行）+ 事件流（frb StreamSink）
└── config.rs       # 本地配置持久化（JSON，应用数据目录，0600）
```

### 5.3.1 rathole 配置契约（与运维侧的对账约定）

```toml
# 自动生成（勿手改）
[client]
remote_addr = "<public_url 的 host>:8999"   # public_url 带显式端口则原样保留（平移 V1 规则）

[client.services.<设备name>]                 # 单 service；service 名 = 设备条目 name 原样
token = "<user.key>"
local_addr = "127.0.0.1:<清单 port>"
```

- **单 service**（V1 的第二个 `<name>-service` 转发设备端 axum——已随 axum 一并消失）；
- service 名与 `remote_addr` 推导规则与 V1 完全一致，运维侧 rathole 服务端 toml **零改动**即可接入 V2 客户端；
- 生成时机与 V1 相同：登录拉取 / 设备重绑 / 设置保存。

**本地凭据存储**：登录后 `user.key` 存入应用数据目录 JSON（macOS `~/Library/Application Support/mini-oc/`，Windows `%APPDATA%\mini-oc\`，权限 0600）。不存明文密码之外的任何 SB 信息（V2 已无用户级 SB 配置）。

### 5.4 服务编排状态机

```
Idle ──启动本地服务──► LocalOnly(opencode running)
Idle ──启动云服务───► Cloud(opencode + rathole running)
LocalOnly ──升级──► Cloud（自动补启 rathole）
Cloud ──降级──► LocalOnly（停 rathole 保 serve）
任意 ──停止──► Idle（SIGTERM→5s→SIGKILL；杀进程树；端口占用二次扫描）

启动前置检查（三层门控，V1 验证过的设计）：
1. opencode 二进制可解析（OPENCODE_BIN env → PATH → 常见安装位置）
2. rathole 二进制可解析（exe 同目录 assets → PATH）——仅云服务需要
3. 端口未被占用（占用→提示并给出「强制清理」选项）
```

启动顺序：先 opencode serve（`GET /api/info` 存活探测，10s 超时）→ 后 rathole（1.5s 存活探测 + 首条连接日志确认）。

### 5.5 终端启动器（独立窗口运行 opencode）

与 V1 行为一致的系统终端新窗口，命令为 v2 形态：

```
OPENCODE_PASSWORD=<user.key> opencode --server http://127.0.0.1:<port> --session <session_id> <directory>
```

- **凭据只经环境变量传递，绝不进命令行**（V1 已验证的安全设计，历史教训在案）；
- **macOS**：osascript 探测 iTerm2（运行中→当前窗口新 tab）否则 Terminal.app `do script`；launcher.sh 中 `exec` 保证 bash PID 即 opencode PID（复用 V1 方案）；
- **Windows**：临时 launcher.ps1（只引用 `$env:` 变量，首行写 `$PID` 到 pidfile）→ `cmd /c start pwsh -NoExit -File`；`CREATE_NO_WINDOW` 防 shim 弹窗（V1 踩坑结论完整继承）；
- 项目点击（无 session）：同命令去掉 `--session`，进入该项目默认新会话；
- 会话点击：带 `--session <id>` 直达该会话。

### 5.6 日志管道

```
opencode stdout/stderr ─┐
rathole stdout/stderr ──┼─► logbuf（环形 2000 行，带来源/级别/时间戳）
core 自身事件（启动/停止/错误/HTTP 摘要）─┘
        │ frb StreamSink（节流 100ms 批量推送）
        ▼
   Flutter 日志页（虚拟滚动列表、来源过滤、全屏、SelectableRegion 复制）
```

---

## 6. mini-oc-web 服务端详细设计

### 6.1 总体

延续 V1 的 BFF 模式与工程骨架（axum + 错误信封 + HMAC cookie + 限流 + wiremock 测试），**重构**以下三块：

1. 设备清单模型精简（§6.2）；
2. 状态刷新调用链（§6.3）；
3. 项目/会话数据源（§6.4，直连 v2 API + SB 快照）。

### 6.2 设备清单 V2 精简

```rust
pub struct UserDevice {
    pub desc: String,        // 描述（1-64 字）
    pub name: String,        // rathole 服务名 ^[a-z0-9-]{1,64}$（唯一键）
    pub port: u16,           // 设备端 opencode serve 端口（rathole 转发的本地端口）
    pub pctype: String,      // windows | macos（设备身份核验用）
    pub device_name: String, // 主机名（客户端绑定写入，只读展示）
    pub bound: bool,         // 绑定状态（客户端写入）
    pub public_url: String,  // 隧道公网地址 scheme://host[:port]（超管录入）
}
```

**删除**：`oc_port`（V1 的 nginx 首段端口分流前缀——V2 直连隧道端点，不再需要）、V1 `port` 的 TUI 语义（axum 服务不复存在）。**`port` 字段语义收敛为唯一端口**：opencode serve 端口。

地址拼装规则（继承 V1 双形态）：
- `public_url` 无端口（https 单域名布局）：`{public_url}/{port}` 路径形态；
- `public_url` 带端口（直连隧道端点）：原样使用。

### 6.3 状态刷新调用链（重构核心）

```
前端（设备页进入/下拉刷新/手动刷新）
  → GET /api/me
    → cookie 会话 → SB 加载注册表
    → join_all 并发（每设备，4s 超时，永不抛错）:
        GET {tunnel}/api/info   Basic opencode:<user.key>
        ├─ 200 → online=true, opencode_version=<v>（凭据与身份双验证 ✓）
        ├─ 401 → tunnel_auth_mismatch（隧道通但 key 不一致——配置错误显式暴露）
        └─ 超时/连接失败 → offline
    → 平台核验：/api/info 无法直接返回 os —— 改由设备绑定时上报核验（§6.5）
  → 响应 {user, devices:[{...device, status: online|offline|auth_mismatch, version}]}
```

对比 V1：探测目标从「设备端 axum `/status`」变为「设备 opencode `/api/info`」；`opencode_online` 概念消失（online 即 opencode online）；**新增 `auth_mismatch` 第三态**，把「隧道通但凭据错」从 silent offline 提升为显式配置错误。

前端状态点：绿（online）/红（offline）/橙（auth_mismatch）。

### 6.4 项目 / 会话数据源

```
GET /api/devices/:name/projects
  → live: GET {tunnel}/api/project（缓存 60s 防抖）
  → fallback: SB snapshot serv/opencode/snapshots/{user_id}/{name}/projects.md
GET /api/devices/:name/sessions?directory=
  → live: GET {tunnel}/api/session?directory=<dir>（分页拉全，截断 200 条）
  → fallback: SB snapshot .../sessions.md
响应统一带 source: "live" | "snapshot"（继承 V1 双源标注，禁止无标注混发）
```

**快照写入**：设备在线时，BFF 在 projects/sessions 请求成功后 best-effort 异步写快照 md（带 fetched_at 时间戳，TTL 7 天展示时标注「快照于 N 小时前」）。快照只由 BFF 写，设备端不再写 SB（V1 的 path-list 登记簿协议整体废弃——v2 API 本身就是数据源，登记簿是 V1 无 API 时代的替代品）。

**移动端跳转（pair-link）**：
```
GET /api/devices/:name/jump?directory=&session=[&format=json]
  → live: BFF 代设备调 POST {tunnel}/api/pair → code
         → 返回/302 {tunnel}/auth/connect/{code}（一次性，5 分钟有效）
  → 设备离线: 显式 502 device_offline（不再有 V1 的 legacy 假链接）
```
浏览器打开 connect 链接后，opencode Web UI 前端以返回 token 建立 `opencode_session_<port>` cookie 会话（V5 已验证 token 机制）。

### 6.5 API 变更清单（相对 V1）

| 接口 | 变化 |
|---|---|
| `POST /api/login` | 不变（key 登录） |
| `GET /api/me` | **重构**：探测改 `/api/info`，状态三态化（§6.3） |
| `GET /api/device-status/:name` | 改按 name 深探测（原 :port 端口在 V2 无跨设备唯一性保证） |
| `GET /api/devices/:name/projects`、`/sessions` | **重构**：live=设备 v2 API + snapshot fallback + source 标注 |
| `POST /api/devices/:name/sessions` | **移除**（V1 的假创建链路整体删除；创建动作在 opencode Web UI/TUI 内完成） |
| `GET /api/devices/:name/jump` | **重构**：pair-link 直连隧道；移除 legacy/userinfo-URL 回退 |
| `GET /api/devices/:name/detail` | 移除明文凭据下发，仅返回隧道地址与端口信息 |
| `POST /api/user/info` | 字段精简（删 sb 配置块、oc_port；设备清单 V2 结构） |
| `POST /api/device-bind` | 保留 + 上报 `os`（BFF 核验与 pctype 一致，写回 last_bound_at） |
| `/api/admin/*` | 保留；用户 CRUD 的设备字段按 §6.2；**移除 `/api/admin/info` 的 sb_password 明文返回** |
| `/api/me/key`、`/api/me/name`、`/api/me/sb` | key/name 保留；**me/sb 删除**（无用户级 SB） |

注册表 SB 文档：路径与读取方式不变（`web/opencode/config.md`，整体 JSON），schema 版本号升 `2`，首次读到 v1 结构自动迁移（剥离子段、删 UserSbConfig，写回）。

### 6.6 前端（Vue3 SPA，移动优先）

- 布局：底部 TabBar（移动）/ 侧栏（≥768px），大圆角卡片、亮暗主题；
- 页面：登录、设备（卡片+状态下拉刷新）、项目、会话（含移动端跳转按钮）、管理（超管：用户 CRUD + 设备清单编辑 + 密钥重置）；
- 管理「新增设备」表单字段：描述、服务名、平台类型、opencode 端口、穿透地址（**删除** V1 的「OpenCode 端口号」「TUI 端口号」两栏，收敛为单「端口」栏）。

---

## 7. 数据模型汇总

### 7.1 SB 用户注册表（`web/opencode/config.md`）

```json
{
  "version": 2,
  "users": [
    {
      "id": "100001",
      "name": "samuel",
      "key": "<32位随机>",
      "devices": [ { UserDevice（§6.2） } ],
      "created_at": "2026-10-20T00:00:00Z",
      "updated_at": "...",
      "last_used_at": null
    }
  ]
}
```

删除字段：`cloud_ip`（V1 设备→云回调地址，V2 客户端直接配置服务器地址）、`sb`（用户级 SB 配置）。

### 7.2 快照 md（`serv/opencode/snapshots/{user_id}/{device_name}/`）

- `projects.md`：`{fetched_at, data: Project[]}`
- `sessions.md`：`{fetched_at, directory: <最近目录>, data: Session.Info[]}`

### 7.3 客户端本地配置（应用数据目录 `config.json`）

```json
{
  "server_url": "https://oc.example.com",
  "user": {"id": "100001", "name": "samuel", "key": "<32位>"},
  "bound_device": "macbook-pro",
  "port": 9464,
  "opencode_bin": null,
  "theme": "system"
}
```

`port` 语义：纯本地模式（未绑定设备）的端口偏好；绑定设备后云服务端口以清单下发为准（§5.2.1）。

---

## 8. 安全设计

继承 V1 全部机制并修补已知债务：

1. **HMAC 无状态 cookie**（密钥持久化 0600）+ 管理会话 12h / 用户会话 7d 分离 TTL；
2. 登录限流（per-IP 10min 5 次）扩展到 `/api/user/info`（V1 已有）与 device-bind；
3. **常数时间**密码比较；
4. 修补：cookie 增加 `Secure`（当请求是 https 时）——修正 V1 README 与实现不一致；
5. 修补：`/api/admin/info` 不再返回 sb_password；detail 接口不再明文下发凭据；
6. 凭据传递全程：env 注入（serve/TUI）、BFF 服务端持有（隧道调用）、pair token 一次性（浏览器）；**任何路径不进 URL referrer 可见位置、不进日志**（http_trace 截断脱敏继承 V1）；
7. 快照 md 不含凭据。

---

## 9. 测试策略

| 层 | 方式 |
|---|---|
| mini-oc-core（共享 crate） | 纯函数单测：地址拼装、toml 生成、V1→V2 注册表迁移、字段校验 |
| mini-oc-web | wiremock 集成测试：模拟 SB + 模拟设备 opencode v2 API（`/api/info` 200/401/超时三态、project/session 分页、pair 链路、auth_mismatch、快照读写、迁移） |
| 客户端 Rust core | 单测（rathole toml、launcher 脚本生成内容快照测试）+ 手动平台验收清单 |
| Flutter UI | widget 测试（门控逻辑：未登录置灰、状态三态渲染）；真机/双平台手动验收 |
| 端到端手动验收 | 双平台：登录→绑定→云服务启停→移动端门户→跳转 Web UI→终端 TUI 全链路 |

---

## 10. 部署

- **mini-oc-web**：Docker 三阶段构建（node→rust→runtime）延续 V1，修 compose 的 SB external network 声明；env 不变（`WEB_PORT/SB_*/COOKIE_KEY_PATH/PORTAL_BASE`）；
- **mini-oc**：`flutter build macos --release`（dmg）+ `flutter build windows --release`（exe/zip）；rathole 二进制按 `<os>-<arch>` 打入 assets，构建时随包分发（V1 build.rs 的 bundle 方案改为 Flutter assets 约定路径）。

---

## 11. 里程碑划分（供 plan 拆解）

1. **M1 共享地基**：workspace + mini-oc-core（模型/校验/迁移/API client/地址拼装，全单测）
2. **M2 web 服务端**：BFF 重构（探测三态、双源列表、快照、pair 跳转、管理 API V2 表单）+ Vue3 SPA 移动优先重写 + wiremock 全套
3. **M3 客户端 core**：frb 骨架 + 账户/监督/rathole/日志/终端启动器（Rust 侧完成）
4. **M4 客户端 UI**：Flutter 四页 + 主题系统 + 动效 + 门控
5. **M5 端到端联调**：双平台验收 + Docker 部署验证

---

## 12. 本设计做出的关键决策（用户已确认）

| 决策 | 选择 |
|---|---|
| 客户端技术栈 | Flutter + Rust FFI（复刻 FlClash） |
| 独立窗口形态 | 系统终端新窗口跑 opencode TUI（与 V1 跳转一致） |
| 仓库布局 | 当前 mini-oc 仓库 monorepo |
| rathole token | user.key |
| 登录门控 | **全部功能强制登录**（本地服务也需登录，凭据统一 user.key） |
| 端口权威源 | **云服务以设备清单下发 port 为准**；纯本地模式用本地偏好 |

## 13. 设计者自决项（用户未指定，按最合理方案选定，可推翻）

| 项 | 决定 | 理由 |
|---|---|---|
| frb 桥接库 | flutter_rust_bridge v2 | 事实标准，StreamSink 满足日志流 |
| 快照机制 | BFF 写 SB（设备端不再写） | v2 API 即数据源；保持离线可见 + SB 存储核心地位 |
| V1 path-list 协议 | 废弃不迁移 | v2 API 完全覆盖其功能 |
| 设备探测身份核验 | 绑定时上报 os 核验 | `/api/info` 无 os 字段，V1 的运行时核验无法平移 |
| 假会话创建链路 | 移除 | V1 已确认的债务，指南要求按新流程重构 |
| 默认端口 | 9464（客户端可配） | 延续 V1 默认值降低认知成本 |
