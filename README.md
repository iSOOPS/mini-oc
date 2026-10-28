# mini-oc

多端 opencode 远程工作站：**Flutter 桌面客户端**（macOS / Windows）+ **Rust Web 门户**（移动优先）。

> V2 重构。设计文档：`docs/superpowers/specs/2026-10-20-mini-oc-v2-design.md`

## 组成

| 目录 | 说明 |
|---|---|
| `apps/mini-oc-web` | Rust (axum) + Vue3 门户：用户/设备管理、状态探测、项目/会话浏览、会话跳转 |
| `apps/mini-oc` | Flutter + Rust FFI 桌面客户端（开发中，见计划 2） |
| `crates/mini-oc-core` | 共享库：数据模型、V1→V2 迁移、隧道地址拼装、opencode v2 API client |

## 快速开始（mini-oc-web）

```bash
cd apps/mini-oc-web/web && npm ci && npm run build && cd ..
cp .env.example .env   # 编辑 SB_PASSWORD 等
docker network create app-net  # 首次；并把既有 SilverBullet 容器接入
docker compose up -d --build
```

## 本地开发：

```bash
cargo run -p mini-oc-web            # 后端 :8100（SB 不可用时接口 503，SPA 正常）
cd apps/mini-oc-web/web && npm run dev   # Vite 开发服务器（/api 代理到 8100）
```

本地运行零配置：未设置 `COOKIE_KEY_PATH` / `WEB_STATIC_DIR` 时自动回退本地默认——
cookie key 写入 `<cwd>/data/cookie_key`，SPA 目录按布局探测（仓库根直接运行即可找到）。
显式环境变量永远优先；容器部署由 Dockerfile `ENV` 注入 `/data/cookie_key` 与 `/app/web/dist`，行为不变。

## 环境变量

| 变量 | 默认 | 说明 |
|---|---|---|
| `WEB_PORT` | 8100 | 监听端口 |
| `WEB_STATIC_DIR` | 本地：布局探测（回退 `web/dist`）；容器：`/app/web/dist` | SPA 静态目录 |
| `COOKIE_KEY_PATH` | 本地：`<cwd>/data/cookie_key`；容器：`/data/cookie_key` | HMAC 密钥（首启自动生成） |
| `SB_BASE_URL` / `SB_USER` / `SB_PASSWORD` | — | SilverBullet 连接；**SB_PASSWORD 兼作超管密码** |

> 「本地默认」仅在未设置对应环境变量时生效；显式变量永远优先。

## 运维约定（rathole）

rathole **服务端**配置由管理员在系统外维护（本系统不做服务端管理）。接入新设备时需保持三处一致：

1. 本系统管理页登记的设备**服务名**（`name`）＝ 服务端 toml 的 service 名；
2. 设备登记的 **port** ＝ 服务端该 service 的转发端口；
3. 服务端该 service 的 **token** ＝ 该用户的 32 位密钥（管理页「重置密钥」后需同步更新）。

客户端（mini-oc）会根据 `/api/user/info` 下发的设备清单自动生成 rathole 客户端配置，无需人工干预。
