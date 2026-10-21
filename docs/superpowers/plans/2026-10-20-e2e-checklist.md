# mini-oc V2 端到端验收清单

日期：2026-10-21 · 分支：feature/v2

## 自动化验证（已通过）

| 验收项 | 结果 |
|---|---|
| mini-oc-web 全部集成测试（wiremock：SB/设备/API 三方） | ✅ 34 个测试 |
| mini-oc-web SPA 构建与服务（vue-tsc + vite + 静态回退冒烟） | ✅ |
| Docker 镜像构建 + 容器运行（healthz 200 + SPA 加载） | ✅ daocloud 镜像源 |
| 客户端 Rust core 全部单测/集成测试 | ✅ 29 个测试 |
| Flutter widget 测试（登录门控/仪表盘门控/会话检索/日志过滤/复制/全屏/断点 Rail-Bar） | ✅ 13 个测试 |
| **真实链路 e2e**：真 opencode serve 启动→探针→项目列表→会话查询→终端计划（key 零泄露）→无孤儿进程 | ✅ `cargo test --test e2e_local_test -- --ignored` |
| opencode v2 实测事实（serve/pair/basic-auth/分页 API/`OPENCODE_SERVER_PASSWORD`） | ✅ 已在 spec §2 固化 |

## 待执行（需要环境）

### macOS 真机 GUI（需安装完整 Xcode）

```bash
cd apps/mini-oc && ./scripts/build-macos.sh
# 打开 build/macos/Build/Products/Release/mini_oc.app
```

- [ ] 登录页输入服务器地址+密钥 → 进入概览
- [ ] 未绑定设备时云服务开关置灰 + 引导弹窗
- [ ] 启动本地服务 → 概览状态卡变绿（PID/端口）
- [ ] 会话页出现本机项目列表 → 检索 → 展开项目见会话
- [ ] 点击会话 → 系统终端（iTerm/Terminal）新窗口直达该会话
- [ ] 日志页实时滚动 opencode 输出 → 来源过滤 → 全屏 → 复制
- [ ] 设置页修改端口 → 保存 → 重启生效
- [ ] 退出应用 → `pgrep opencode` 无孤儿进程

### 云链路（需 mini-oc-web 部署 + rathole 服务端配置）

- [ ] 管理页创建用户（设备：服务名/平台/端口/穿透地址）→ 密钥面板展示
- [ ] 客户端登录 → 绑定设备（平台校验通过）
- [ ] 启动云服务 → rathole 连接成功（服务端 toml token=用户密钥）
- [ ] 手机浏览器开 mini-oc-web → 设备页绿点在线（含版本号）
- [ ] 手机会话页点「在设备上打开」→ opencode Web UI（pair 一次性链接）
- [ ] 重置用户密钥 → 设备状态变橙（auth_mismatch）→ 客户端重启云服务恢复

### Windows（需 Windows 环境）

- [ ] 按 `apps/mini-oc/scripts/build-windows.md` 构建
- [ ] 同 macOS 清单 1-8 项 + `taskkill /T /F` 进程树验证

## 已知限制

1. **Xcode 未安装**（本机）：macOS release 打包与 GUI 真机验收待环境就绪；
2. **rathole 服务端**在系统外人工维护（V1 约定不变），token=用户密钥需管理员同步；
3. 注册表整读整写（V1 权衡保留，ADR 20/21）；双 admin 并发保存存在覆盖窗口。
