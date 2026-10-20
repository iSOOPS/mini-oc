# mini-oc-web V2 实施计划（计划 1/2：共享地基 + 服务端）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 monorepo 中搭建 Cargo workspace，实现共享 crate `mini-oc-core`（数据模型/迁移/隧道拼装/opencode v2 client），并重构 mini-oc-web 为 V2（探测三态、双源列表、快照、pair 跳转、移动优先 Vue3 SPA、Docker 部署）。

**Architecture:** BFF 模式（axum）+ Vue3 SPA + SilverBullet md 存储。设备端凭据统一 `opencode:<user.key>`，探测直连设备 opencode v2 `/api/info`。共享逻辑下沉 `crates/mini-oc-core`，web 与后续客户端 crate 复用。

**Tech Stack:** Rust (axum 0.7, reqwest 0.12 + rustls, tokio, serde) / Vue 3.5 + Vite 6 + Pinia 2 / wiremock 集成测试 / Docker 三阶段构建。

**Spec:** `docs/superpowers/specs/2026-10-20-mini-oc-v2-design.md`

## Global Constraints

- 所有持久化数据存 SilverBullet md（`/.fs/` HTTP API），web 端无数据库；
- TLS 一律 rustls（禁 native-tls/OpenSSL）；
- 用户注册表 schema version = `2`，SB 路径不变：`web/opencode/config.md`；
- `user.key` 不出现在：URL query（pair 机制除外）、日志明文、错误响应；
- 错误信封统一 `{"error":{"code","message"}}`；错误码集合见 Task 5；
- 中文代码注释；commit 信息英文 conventional（feat/fix/test/docs/chore）；
- 原项目参考路径：V1 web = `/Users/samuel/Documents/GitForAi/mini-oc-web`（移植时优先复制后精简，保持已验证的安全逻辑）；
- opencode v2 实测事实以 spec §2 为准（用户名硬编码 `opencode`）。

## Review Focus

1. **V1 注册表旧数据**（含 `oc_port`/`cloud_ip`/`sb` 字段或更旧格式）：首次读取必须无损迁移到 V2 并写回，不得 panic 或丢用户。→ Task 2 迁移测试锁定。
2. **public_url 两种形态**（`https://oc.example.com` vs `http://1.2.3.4:9101`）：拼装规则不同，猜错则探测永远 offline。→ Task 3 表驱动测试锁定（含畸形输入）。
3. **auth_mismatch 态**（隧道通但用户已 regenerate-key，设备仍用旧 key）：必须显式橙态，不得误报 offline。→ Task 6 wiremock 401 场景测试。
4. **SB 不可用**：所有触 SB 接口返回 503 `service_unavailable`，不得 panic。→ Task 5 wiremock 断连测试。
5. **opencode session 分页**：`/api/session` 返回 `{data, cursor}`，设备会话 >50 条时必须翻页拉全再截断。→ Task 4 cursor 循环测试。

---

### Task 1: Workspace 骨架

**Files:**
- Create: `Cargo.toml`（workspace 根）
- Create: `crates/mini-oc-core/Cargo.toml`、`crates/mini-oc-core/src/lib.rs`
- Create: `apps/mini-oc-web/Cargo.toml`、`apps/mini-oc-web/src/main.rs`（hello axum）
- Create: `.gitignore`

**Interfaces:**
- Produces: workspace 成员 `crates/mini-oc-core`（包名 `mini-oc-core`）与 `apps/mini-oc-web`（包名 `mini-oc-web`）；`cargo test --workspace` 可运行。

- [ ] **Step 1: 创建 workspace 根 Cargo.toml**

```toml
[workspace]
resolver = "2"
members = ["crates/mini-oc-core", "apps/mini-oc-web"]

[workspace.package]
version = "2.0.0"
edition = "2021"

[workspace.dependencies]
tokio = { version = "1", features = ["full"] }
axum = "0.7"
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls", "json"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "1"
chrono = { version = "0.4", features = ["serde"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

- [ ] **Step 2: 创建空 core crate 与 hello web crate（main.rs 仅 `axum::Router::new().fallback(|| async {"ok"})` + `#[tokio::main]` serve `0.0.0.0:$WEB_PORT`）**

- [ ] **Step 3: 验证构建与测试**

Run: `cargo build --workspace && cargo test --workspace`
Expected: 全部通过

- [ ] **Step 4: Commit** `chore: workspace skeleton with core and web crates`

---

### Task 2: core 数据模型 + V1→V2 迁移

**Files:**
- Create: `crates/mini-oc-core/src/device.rs`
- Test: `crates/mini-oc-core/tests/model_test.rs`

**Interfaces:**
- Produces:
  - `pub struct UserDevice { desc, name, port: u16, pctype, device_name, bound: bool, public_url }`（serde camelCase，字段校验 `UserDevice::validate(&self) -> Result<(), String>`：name `^[a-z0-9-]{1,64}$`、pctype ∈ {windows, macos}、public_url `^https?://[^/]+$`、desc 1-64 字）
  - `pub struct PortalUser { id, name, key, devices, created_at, updated_at, last_used_at: Option<String> }`（id 6 位数字、name `^[A-Za-z0-9_-]{1,64}$`、key 32 位 `[A-Za-z0-9]`）
  - `pub struct UsersFile { version: u32, users: Vec<PortalUser> }`
  - `pub fn generate_user_id(existing: &[String]) -> String`、`pub fn generate_key() -> String`
  - `pub fn migrate_v1(raw: &str) -> Result<UsersFile, String>`：V1 JSON（含 `oc_port: Option<u16>`、`cloud_ip`、`sb` 对象、可能无 version 字段）→ V2（剥除废弃字段）；未知字段忽略；单条设备损坏跳过该设备不炸全表

- [ ] **Step 1: 写失败测试**：V1 样例 JSON（取自 spec §6.2 删除字段列表构造）迁移后字段精确断言；坏设备条目跳过；校验规则表驱动（非法 name/pctype/public_url 拒绝）；id/key 生成格式与唯一性。

- [ ] **Step 2: `cargo test -p mini-oc-core` 验证 FAIL**（未定义）

- [ ] **Step 3: 实现 device.rs**（serde `#[serde(default)]` + `deny_unknown_fields=false` 兼容旧字段；rand 用 `getrandom` 或手拼——core 不引入额外依赖时用 `uuid` v4 去 hyphen 不可靠，直接用 `rand = "0.8"`，workspace 加入）

- [ ] **Step 4: 测试 PASS**

- [ ] **Step 5: Commit** `feat(core): user/device models with v1 migration`

---

### Task 3: core 隧道地址拼装

**Files:**
- Create: `crates/mini-oc-core/src/tunnel.rs`
- Test: `crates/mini-oc-core/tests/tunnel_test.rs`

**Interfaces:**
- Produces:
  - `pub fn tunnel_base_url(public_url: &str, port: u16) -> String`——public_url 无显式端口：`{public_url}/{port}`；有端口：原样返回
  - `pub fn remote_addr_for_rathole(public_url: &str) -> String`——host + `:8999`；public_url 带显式端口则 `host:port` 原样（平移 V1 `config.rs` 规则与测试用例）
  - 校验失败（非 http(s)、带 path）返回 `Err`：签名调整为 `pub fn ... -> Result<String, TunnelError>`

- [ ] **Step 1: 失败测试（表驱动，用例含 spec §6.2 两种形态 + 畸形输入拒绝 + IPv6 host）**
- [ ] **Step 2: 验证 FAIL**
- [ ] **Step 3: 实现**（`url` crate 解析，workspace 加 `url = "2"`）
- [ ] **Step 4: PASS**
- [ ] **Step 5: Commit** `feat(core): tunnel url assembly`

---

### Task 4: core opencode v2 API client

**Files:**
- Create: `crates/mini-oc-core/src/ocapi.rs`（含 `pub mod types` serde 结构）
- Test: `crates/mini-oc-core/tests/ocapi_test.rs`（wiremock）

**Interfaces:**
- Consumes: 无（独立）
- Produces:
  - `pub struct OcClient::new(base_url: &str, password: &str) -> Self`（内部存 Basic `opencode:<password>`；reqwest Client 复用单例）
  - `pub async fn info(&self) -> Result<ServerInfo, OcError>`；`ServerInfo { version: String, pid: u64 }`（serde 宽松：多余字段忽略）
  - `pub async fn projects(&self) -> Result<Vec<Project>, OcError>`；`Project { id, canonical, vcs: Option<String>, name: Option<String> }`
  - `pub async fn sessions(&self, directory: Option<&str>) -> Result<Vec<SessionInfo>, OcError>`——**cursor 循环拉全**（每页 limit=50，最多 10 页截断），`SessionInfo { id, project_id, title: Option<String>, directory: String, time: SessionTime{created,updated} }`（serde camelCase，宽容缺字段）；附A JSON 为黄金测试样例
  - `pub async fn pair(&self) -> Result<String, OcError>`（POST `/api/pair` → code）
  - `OcError { Unauthorized(u16), Transport, Other(String) }`——探测三态的判别依据
- Client trait 抽象：`pub trait OcApi`（async-trait）供 web 层 wiremock 注入；`OcClient` 为默认实现

- [ ] **Step 1: 失败测试**：wiremock 起伪 opencode server——info 200/401/超时；projects 数组；sessions 两页 cursor 拼接 + 10 页截断；pair code 提取；附A 黄金样例反序列化
- [ ] **Step 2: 验证 FAIL**
- [ ] **Step 3: 实现**
- [ ] **Step 4: PASS**
- [ ] **Step 5: Commit** `feat(core): opencode v2 api client`

---

### Task 5: web 骨架移植（error/auth/sb/state/router）

**Files:**
- Create: `apps/mini-oc-web/src/{state.rs,error.rs,auth.rs,sb.rs,routes/mod.rs}`
- Modify: `apps/mini-oc-web/src/main.rs`
- Test: `apps/mini-oc-web/tests/common/mod.rs`、`apps/mini-oc-web/tests/health_test.rs`

**Interfaces:**
- Consumes: `mini-oc-core::{UsersFile, migrate_v1}`
- Produces:
  - `AppState { sb: SbClient, cookie_key: Vec<u8>, rate: RateLimiter, started_at }`、`AppState::from_env()`
  - 错误码集合（error.rs，复用 V1 名 + 新增 `auth_mismatch`）：`unauthorized/not_found/forbidden/rate_limited/service_unavailable/auth_mismatch/invalid_target/device_offline/pair_url_invalid/internal`
  - `SbClient::get_users()/put_users(&UsersFile)`（404→空表→走 `migrate_v1`；401 自动重登一次）
  - HMAC cookie：`sign_cookie(subject,&key,ttl)` / `verify_cookie`；用户 cookie 名 `mini_oc_session`（7d），管理 cookie `mini_oc_admin`（12h）；**Secure flag 当 x-forwarded-proto=https**（修补 V1）
  - `GET /healthz` → 200
  - 测试环境 `common::setup()`：wiremock SB + 随机端口 spawn axum（复用 V1 tests/common 模式）

- [ ] **Step 1: 失败测试**——healthz 200；SB 断连时任意触 SB 接口返回 503 `service_unavailable` 不 panic
- [ ] **Step 2: 验证 FAIL**
- [ ] **Step 3: 从 V1 复制 error.rs/auth.rs/sb.rs 适配**（V1 路径 `src/{error,auth,sb}.rs`；限流器随 auth.rs）
- [ ] **Step 4: PASS**
- [ ] **Step 5: Commit** `feat(web): scaffold with error envelope, auth, sb client`

---

### Task 6: 登录 + /api/me 探测三态

**Files:**
- Create: `apps/mini-oc-web/src/probe.rs`、`apps/mini-oc-web/src/routes/auth_routes.rs`
- Test: `apps/mini-oc-web/tests/login_me_test.rs`

**Interfaces:**
- Consumes: `OcApi` trait（wiremock 设备伪服务）、`tunnel_base_url`
- Produces:
  - `POST /api/login {key}` → Set-Cookie；限流 per-IP 10min/5 次
  - `POST /api/logout`
  - `GET /api/me` → `{user:{id,name}, devices:[{desc,name,port,pctype,device_name,bound,public_url, status:"online"|"offline"|"auth_mismatch", version:Option<String>}]}`——`probe_device(client,&UserDevice,password)` 三态判别：200→online+version；401→auth_mismatch；Transport/超时(4s)→offline；join_all 并发，永不抛错
  - `GET /api/me/key`（重显密钥）
  - `POST /api/me/name {name}`

- [ ] **Step 1: 失败测试**——登录成功/错 key 401/限流 429；/api/me：wiremock 设备 info 200（online+version）、401（auth_mismatch）、不通（offline）三场景；未登录 401
- [ ] **Step 2: 验证 FAIL**
- [ ] **Step 3: 实现**
- [ ] **Step 4: PASS**
- [ ] **Step 5: Commit** `feat(web): login and 3-state device probe`

---

### Task 7: 设备信息下发 + 绑定

**Files:**
- Create: `apps/mini-oc-web/src/routes/device_routes.rs`（本任务只做 user-info/device-bind/device-status）
- Test: `apps/mini-oc-web/tests/device_bind_test.rs`

**Interfaces:**
- Produces:
  - `POST /api/user/info {key}` → `RemoteUserInfo { id, name, key, devices:[UserDevice] }`（限流；key 属主匹配；**无 sb 字段**）
  - `POST /api/device-bind {key, user_id, name, bound, pctype, device_name, os}`——key 属主=user_id；找到设备写 bound/device_name；**os ≠ pctype → 403 forbidden**（身份核验）；last_bound_at 时间戳更新
  - `GET /api/device-status/:name`（cookie；按 name 匹配本用户设备，找不到 404；单设备深探测三态）

- [ ] **Step 1: 失败测试**——绑定成功写回 SB（wiremock 断言 PUT 体）；os 不匹配 403；跨用户 key 403；user/info 不含 sb
- [ ] **Step 2: 验证 FAIL**
- [ ] **Step 3: 实现**
- [ ] **Step 4: PASS**
- [ ] **Step 5: Commit** `feat(web): user info and device bind with os check`

---

### Task 8: 项目/会话双源 + 快照

**Files:**
- Create: `apps/mini-oc-web/src/snapshot.rs`
- Modify: `apps/mini-oc-web/src/routes/device_routes.rs`
- Test: `apps/mini-oc-web/tests/projects_sessions_test.rs`

**Interfaces:**
- Consumes: `OcApi`
- Produces:
  - `GET /api/devices/:name/projects` → `{source:"live"|"snapshot", fetched_at:Option<String>, data:[Project]}`
  - `GET /api/devices/:name/sessions?directory=` → 同结构（`data:[SessionInfo]`）
  - live 路径：设备隧道调 `OcApi`；成功后 **best-effort 异步写快照**（不阻塞响应；失败仅 tracing warn）
  - snapshot 路径：设备 offline/auth_mismatch 时读 `serv/opencode/snapshots/{user_id}/{name}/projects.md`、`sessions.md`（`{fetched_at,data}`）；无快照→ `data:[]` + `source:"snapshot"` + `fetched_at:null`（前端展示「无缓存」）；**禁止 live 失败时静默当 snapshot 成功**——响应必须带 source
  - live 结果 60s 内存缓存防抖（per device+path）

- [ ] **Step 1: 失败测试**——设备在线：live + 快照文件被写（wiremock SB 收到 PUT）；设备 401：fallback snapshot（预先种快照 md）+ source=snapshot；设备超时无快照：空 data；缓存窗口内二次请求不打设备（wiremock 计数）
- [ ] **Step 2: 验证 FAIL**
- [ ] **Step 3: 实现**
- [ ] **Step 4: PASS**
- [ ] **Step 5: Commit** `feat(web): dual-source projects/sessions with sb snapshots`

---

### Task 9: 会话跳转 pair-link

**Files:**
- Create: `apps/mini-oc-web/src/jump.rs`
- Modify: `apps/mini-oc-web/src/routes/device_routes.rs`
- Test: `apps/mini-oc-web/tests/jump_test.rs`

**Interfaces:**
- Produces:
  - `GET /api/devices/:name/jump?directory=&session=&format=json`
  - live：`OcApi::pair()` → code → 跳转 URL `{tunnel}/auth/connect/{code}`；302（浏览器）或 `{url, expires_in:300}`（json）
  - `validate_jump_url(&url, public_url)`（平移 V1 jump.rs 三重校验防开放重定向，含单测）
  - 设备 offline/auth_mismatch → 502 `device_offline`（**无 legacy 回退**）
  - `GET /api/devices/:name/detail` → `{tunnel_base, port, status}`（**无凭据**）

- [ ] **Step 1: 失败测试**——pair 链路 302 与 json 两形态；设备离线 502；恶意 code 响应（错误 origin）被 validate 拒绝
- [ ] **Step 2: 验证 FAIL**
- [ ] **Step 3: 实现**
- [ ] **Step 4: PASS**
- [ ] **Step 5: Commit** `feat(web): pair-link session jump`

---

### Task 10: 管理端 API（V2 表单 + 安全修补）

**Files:**
- Create: `apps/mini-oc-web/src/routes/admin_routes.rs`
- Test: `apps/mini-oc-web/tests/admin_test.rs`

**Interfaces:**
- Produces:
  - `POST /api/admin/login {password}`（常数时间比较 vs `SB_PASSWORD` env）/ logout
  - `GET /api/admin/users`（含 key——仅超管）/ `POST`（`{name, devices:[UserDevice输入]}`：校验 + 唯一 id/key + **设备 name 全局唯一**（跨用户，因 rathole service 名空间全局）→ 整体 PUT 回 SB）
  - `PUT/DELETE /api/admin/users/:id`、`POST /api/admin/users/:id/regenerate-key`
  - **无 `/api/admin/info` 密钥泄露接口**（V1 债务修补：不存在此端点）

- [ ] **Step 1: 失败测试**——CRUD 全链路（SB PUT 体断言 V2 字段集、无 oc_port/sb）；跨用户设备重名 409/400；错误密码 401；非超管 cookie 401
- [ ] **Step 2: 验证 FAIL**
- [ ] **Step 3: 实现**
- [ ] **Step 4: PASS**
- [ ] **Step 5: Commit** `feat(web): admin api v2`

---

### Task 11: Vue3 SPA（移动优先）

**Files:**
- Create: `apps/mini-oc-web/web/`（`package.json`、`vite.config.ts`、`src/{main.ts,api.ts,store.ts,router.ts}`、`src/pages/{LoginPage,DevicesPage,ProjectsPage,SessionsPage,AdminLoginPage,AdminPage}.vue`、`src/components/`）
- Create: `apps/mini-oc-web/web/src/styles/`（CSS 变量主题：亮/暗 + `prefers-color-scheme`）

**Interfaces:**
- Consumes: Task 6-10 全部 API（响应结构以其测试为准）
- Produces: `npm run build` → `web/dist`；axum SPA fallback（routes/mod.rs 补静态服务，V1 模式移植）

要点（不逐组件展开，实施者按 FlClash 观感自由实现）：
- 移动优先：底部 TabBar（≥768px 侧栏）；大圆角卡片；下拉刷新设备页（调 /api/me）
- 设备卡三态色点（绿/红/橙）+ 版本号；auth_mismatch 显示「凭据不一致，请重启客户端云服务」提示
- 会话页 source 标注横幅（实时/快照于 N 小时前）；跳转按钮调 jump?format=json → window.open（拦截时降级展示手动链接 + 5 分钟有效期提示，平移 V1 行为）
- 管理页：用户列表 + 新增/编辑表单（设备字段：描述/服务名/平台/端口/穿透地址——**单端口栏**）+ 密钥创建/重置展示面板（一次性弹层）
- 登录页 key 输入；router 守卫三级（public/需登录/admin），软跳转防刷新死循环（平移 V1）

- [ ] **Step 1: 实现全部页面与 api.ts/store.ts**
- [ ] **Step 2: `npm run build` + `cargo test -p mini-oc-web`（SPA fallback 集成测试：GET / 返回 index.html、/devices history 回退）**
- [ ] **Step 3: 手动冒烟**（`cargo run` + 浏览器走登录→设备→会话→跳转，用 wiremock 伪设备或本机 `OPENCODE_SERVER_PASSWORD=test opencode serve --port 19464` 当真实设备，hosts 无需改——public_url 填 `http://127.0.0.1:19464`）
- [ ] **Step 4: Commit** `feat(web): mobile-first vue3 spa`

---

### Task 12: Docker 部署 + 文档

**Files:**
- Create: `apps/mini-oc-web/{Dockerfile,docker-compose.yml,.env.example}`
- Create: `README.md`（根，双语简版）、`apps/mini-oc-web/README.md`

- [ ] **Step 1: Dockerfile 三阶段**（node:23-alpine 构建 SPA → rust:1.85-slim → debian:bookworm-slim 运行时；**基础镜像用公共 docker.io 源**，替换 V1 私有 harbor；npm/cargo 用国内镜像 build-arg 可选）
- [ ] **Step 2: docker-compose**：`mini-oc-web` + external network 声明接入已有 SB（修补 V1 的 ghost service 引用）；healthcheck /healthz；volume cookie_key
- [ ] **Step 3: `docker compose build` 本地验证镜像可构建**
- [ ] **Step 4: README**（部署步骤、env 表、设备清单运维约定：rathole 服务端 toml service 名/端口与本系统登记一致 + token=<user.key>）
- [ ] **Step 5: Commit** `chore(web): docker deployment and docs`
