# mini-oc

多端 opencode 远程工作站：**Flutter 桌面客户端**（macOS / Windows）+ **Rust Web 门户**（移动优先）。

一句话概括：把自己的电脑变成一台可远程访问的 opencode 主机——桌面客户端在本地拉起 `opencode serve` 并通过 rathole 隧道暴露出去，Web 门户负责账号、设备管理与会话跳转。

---

## 架构

```text
┌──────────────────────────── 服务端 ────────────────────────────┐
│  mini-oc-web (axum :8100)          SilverBullet (:3000)        │
│  ┌──────────────────────┐          ┌──────────────────────┐    │
│  │ Vue3 SPA (移动优先)   │          │ web/opencode/        │    │
│  │ 用户门户 / 超管后台    │◄──HTTP──►│   config.md (用户表) │    │
│  │ BFF 探测 + 快照 + 跳转 │          │   snapshots/*.md     │    │
│  └─────────┬────────────┘          └──────────────────────┘    │
│            │  经 rathole 隧道（Basic Auth: opencode:<user.key>） │
└────────────┼───────────────────────────────────────────────────┘
             │  rathole server（系统外人工维护，:8999）
═════════════╪════════════ 隧道边界 ════════════════════════════════
             │
┌────────────┼──────────── 用户电脑（macOS / Windows）───────────┐
│  ┌─────────┴─────────┐   ┌─────────────────────────────────┐  │
│  │ mini-oc 客户端     │   │ rathole client（自动生成 toml）  │  │
│  │ Flutter UI + Rust  │   │   token = user.key              │  │
│  │  ├ 登录 / 设备绑定  │   │   local 127.0.0.1:<port>        │  │
│  │  ├ 服务开关        │   └──────────────┬──────────────────┘  │
│  │  ├ 项目 / 会话      │                  │ 转发              │
│  │  ├ 日志             │   ┌──────────────┴──────────────────┐  │
│  │  └ 设置             │   │ opencode serve --port <port>    │  │
│  └───────────────────┘   │ OPENCODE_SERVER_PASSWORD=user.key│  │
│                          │ 内嵌 Web UI + opencode v2 API     │  │
│                          └──────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────┘
```

**凭据主线**：`user.key`（32 位随机串）贯穿全链路——客户端登录门户、设备 `opencode serve` 的密码、Web 侧探测设备时的 Basic Auth 密码、rathole 隧道的 token，全部是它。

**关键简化**（相对 V1）：设备端从 3 个进程（`opencode serve` + 自建 axum TUI 服务 + rathole）减为 **2 个**（`opencode serve` + rathole）。设备端不再有自己的 HTTP 服务，探测 / 列表 / 跳转全部直连 opencode v2 官方 API。

---

## 仓库结构

| 目录 | 说明 |
|---|---|
| `apps/mini-oc-web` | Rust (axum) + Vue3 门户：用户/设备管理、状态探测、项目/会话浏览、会话跳转 |
| `apps/mini-oc` | Flutter + Rust FFI 桌面客户端（macOS / Windows） |
| `crates/mini-oc-core` | 共享库：数据模型、V1→V2 迁移、隧道地址拼装、opencode v2 API client |

Cargo workspace 成员为 `crates/mini-oc-core` 与 `apps/mini-oc-web`；`apps/mini-oc/rust` 是 flutter_rust_bridge 管理的**独立 crate**（frb 的 cargo expand 在多 crate 场景下会误传 `-p` 参数，故被 workspace `exclude`，它经 path 依赖复用 `mini-oc-core`）。

---

## 快速开始（部署 mini-oc-web）

前置：Docker + 一个已在运行的 SilverBullet 实例。

```bash
# 1) 构建前端产物
cd apps/mini-oc-web/web && npm ci && npm run build && cd ../..

# 2) 配置环境变量
cp apps/mini-oc-web/.env.example apps/mini-oc-web/.env   # 编辑 SB_PASSWORD 等

# 3) 网络：把既有 SilverBullet 容器接入同一网络
docker network create app-net                              # 首次
docker network connect app-net <sb-container>

# 4) 启动
cd apps/mini-oc-web && docker compose up -d --build
```

服务监听 `8100`，`/healthz` 返回 `ok` 即为就绪。

镜像为三阶段构建（Node 构建 SPA → Rust 静态编译 → debian-slim 运行时），Dockerfile `ENV` 注入 `WEB_STATIC_DIR=/app/web/dist` 与 `COOKIE_KEY_PATH=/data/cookie_key`，密钥落在 `mini-oc-web-data` 卷里持久化。

---

## 本地开发

```bash
# 后端：cargo run -p mini-oc-web      → http://127.0.0.1:8100
cargo run -p mini-oc-web

# 前端：Vite 开发服务器（/api 代理到 8100）
cd apps/mini-oc-web/web && npm run dev
```

**本地零配置**：未设置 `COOKIE_KEY_PATH` / `WEB_STATIC_DIR` 时自动回退本地默认——cookie key 写入 `<cwd>/data/cookie_key`，SPA 目录按布局探测（`apps/mini-oc-web/web/dist` → `apps/mini-oc-web/web` → `web/dist` → `web`，仓库根直接运行即可找到）。显式环境变量永远优先；容器部署由 Dockerfile `ENV` 注入，行为不变。

SB 不可用时接口返回 503，SPA 页面仍可正常加载。

### 测试

```bash
cargo test -p mini-oc-core -p mini-oc-web        # 共享库 + Web 后端
cd apps/mini-oc/rust && cargo test                # 客户端 Rust core
cd apps/mini-oc && flutter test                    # 客户端 Dart 组件测试
```

Web 端集成测试用 wiremock 模拟 SilverBullet 与设备 opencode server，无需真实外部服务。

---

## 环境变量

### mini-oc-web

| 变量 | 默认 | 说明 |
|---|---|---|
| `WEB_PORT` | `8100` | 监听端口 |
| `WEB_STATIC_DIR` | 本地：布局探测（回退 `web/dist`）；容器：`/app/web/dist` | SPA 静态目录 |
| `COOKIE_KEY_PATH` | 本地：`<cwd>/data/cookie_key`；容器：`/data/cookie_key` | HMAC 密钥（首启自动生成 32 字节，权限 0600） |
| `SB_BASE_URL` | `http://127.0.0.1:3000` | SilverBullet 地址 |
| `SB_USER` | `admin` | SilverBullet 用户名 |
| `SB_PASSWORD` | — | SilverBullet 密码；**兼作超管登录密码** |

> 「本地默认」仅在未设置对应环境变量时生效；显式变量永远优先。

### mini-oc 客户端

| 变量 | 说明 |
|---|---|
| `MINI_OC_HOME` | 覆盖配置目录（默认 macOS `~/Library/Application Support/mini-oc`、Windows `%APPDATA%\mini-oc`），测试隔离用 |
| `OPENCODE_BIN` | `opencode` 二进制路径（优先于配置项与 PATH） |
| `RATHOLE_BIN` | `rathole` 客户端二进制路径（优先于随包分发与 PATH） |

客户端另有 `MINI_OC_SERVER_URL` / `MINI_OC_REMEMBER_PASSWORD` / `MINI_OC_PASSWORD` 三个注入变量：任一存在时，环境配置只读不写（容器部署场景）。

---

## 运维约定（rathole）

rathole **服务端**配置由管理员在系统外维护（本系统不做服务端管理）。接入新设备时需保持三处一致：

1. 管理页**审核同意时填写**的设备**服务名**（`name`）＝ 服务端 toml 的 service 名；
2. 设备上报的 **port** ＝ 服务端该 service 的转发端口；
3. 服务端该 service 的 **token** ＝ 该用户的 32 位密钥（「重置密钥」后需同步更新）。

### 隧道地址的两种形态

设备 `public_url` 的写法决定了隧道地址如何拼装（`crates/mini-oc-core/src/tunnel.rs`）：

| `public_url` 形态 | 探测 / 代理地址 | rathole `remote_addr` |
|---|---|---|
| 无显式端口（`https://oc.example.com`） | `{public_url}/{port}`（路径形态） | `{host}:8999`（统一入口端口） |
| 带显式端口（`http://1.2.3.4:9101`） | `public_url` 原样 | `{host}:{port}` 原样 |

### 设备接入流程

客户端登录后**自动上报**（设置页也可手动「上报」）→ 管理页「待审核设备」中**同意**（填写服务名、穿透地址）或**拒绝**。

设备生命周期：`待审核 → 启用 ⇄ 停用 → 移除`（先停用后移除；启用中的设备删除返回 409）。

停用设备不下发给客户端、不参与探测与 jump。客户端（mini-oc）会根据 `/api/user/info` 下发的设备清单自动生成 rathole 客户端配置，无需人工干预。

---

## API 一览

所有接口返回 JSON；错误统一为 `{"error":{"code","message"}}`。

### 用户门户

| 方法 | 路径 | 说明 |
|---|---|---|
| POST | `/api/login` | 密钥登录，下发会话 cookie（7 天） |
| POST | `/api/logout` | 注销 |
| GET | `/api/me` | 当前用户 + 启用设备列表（含探测状态） |
| GET | `/api/me/key` | 取回自己的密钥 |
| POST | `/api/me/name` | 改用户名（全局唯一） |
| POST | `/api/user/info` | 客户端按 key 拉属主完整配置 |
| POST | `/api/device-bind` | 客户端绑定设备（写 bound / device_name，校验平台一致） |
| POST | `/api/device-register` | 客户端设备上报注册（四态幂等匹配） |
| GET | `/api/device-status/:name` | 单设备深探测 |
| GET | `/api/devices/:name/projects` | 项目列表（live / 快照双源） |
| GET | `/api/devices/:name/sessions` | 会话列表（live / 快照双源） |
| GET | `/api/devices/:name/jump` | 会话跳转（签发 pair code → 深链） |
| GET | `/api/devices/:name/detail` | 设备连接详情（不含凭据） |

### 超管后台

| 方法 | 路径 | 说明 |
|---|---|---|
| POST | `/api/admin/login` | 超管登录（密码 = `SB_PASSWORD`，会话 12 小时） |
| POST | `/api/admin/logout` | 注销 |
| GET / POST | `/api/admin/users` | 用户列表 / 新建用户（只填用户名，密钥自动生成） |
| PUT | `/api/admin/users/:id` | 改用户名 + 设备白名单字段更新（`name` / `publicUrl` / `enabled`） |
| DELETE | `/api/admin/users/:id` | 删除用户 |
| POST | `/api/admin/users/:id/regenerate-key` | 重置密钥 |
| POST | `/api/admin/users/:id/devices/:did/approve` | 待审核设备转正（填服务名 + 穿透地址） |
| DELETE | `/api/admin/users/:id/devices/:did/reject` | 拒绝待审核设备 |
| DELETE | `/api/admin/users/:id/devices/:did` | 移除设备（仅限已停用） |

### 其他

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/healthz` | 健康检查 |
| GET | `/*` | SPA 静态资源与 history 路由回退（`/api` 前缀不回退，缺失接口返回 404 JSON） |

---

## 设计要点

**双源列表**。设备在线时从 opencode v2 API 取实时数据，同时异步写快照到 SilverBullet（`serv/opencode/snapshots/{user_id}/{device}/{projects|sessions}.md`）；设备离线时回退读快照。响应里始终带 `source` 字段（`live` / `snapshot`）——离线陈旧数据不会被伪装成实时数据。

**探测三态**。`200` → `online`（附版本号）；`401/403` → `auth_mismatch`（隧道通但凭据不一致，通常是重置过密钥）；超时或连接失败 → `offline`。

**跳转防开放重定向**。BFF 代设备签发一次性 pair code，构造的 connect URL 会校验与隧道 base 同源（scheme + host + port 完全一致）且路径前缀匹配；会话深链的 session id 与 token 均做字符白名单校验。

**限流**。内存 per-IP 限流器：10 分钟内 5 次失败即拒绝；超管登录走独立 bucket，防针对高价值目标的爆破。

**会话 cookie**。HMAC-SHA256 签名，服务端强制 TTL 过期（不依赖浏览器 `Max-Age`），`HttpOnly` + `SameSite=Lax`，反代后 HTTPS 时追加 `Secure`。

---

## 客户端

见 [`apps/mini-oc/README.md`](apps/mini-oc/README.md)：功能页签、构建脚本（`scripts/build-macos.sh`、`scripts/build-windows.md`）、frb 代码生成流程。

---

## License

MIT，见 [LICENSE](LICENSE)。
