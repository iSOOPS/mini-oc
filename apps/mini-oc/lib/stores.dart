import 'dart:io';

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'src/rust/api.dart' as rust;
import 'src/rust/account.dart' show AccountInfo;
import 'src/rust/config.dart' show ClientConfig;
import 'src/rust/frb_generated.dart';
import 'src/rust/portkill.dart' show PortOccupant;
import 'src/rust/supervisor.dart' show ServiceState;

/// Rust 桥接初始化。
final rustReadyProvider = FutureProvider<void>((ref) async {
  await RustLib.init();
});

/// 会话状态：已登录用户 + 绑定设备 + 记住密码。
class AuthState {
  const AuthState({
    this.userId,
    this.userName,
    this.boundDevice,
    this.rememberPassword = false,
    this.savedPassword,
  });
  final String? userId;
  final String? userName;
  final String? boundDevice;

  /// config 中的记住密码勾选状态（启动回显勾选框）
  final bool rememberPassword;

  /// config 中的密码明文（rememberPassword = true 时用于自动填充）
  final String? savedPassword;

  /// 是否已记住密码（勾选且密码非空）→ 允许自动登录直达主页。
  bool get hasRememberedLogin =>
      rememberPassword && (savedPassword?.isNotEmpty ?? false);

  bool get isLoggedIn => userId != null;
  AuthState copyWith({
    String? userId,
    String? userName,
    String? boundDevice,
    bool? rememberPassword,
    String? savedPassword,
  }) =>
      AuthState(
        userId: userId ?? this.userId,
        userName: userName ?? this.userName,
        boundDevice: boundDevice ?? this.boundDevice,
        rememberPassword: rememberPassword ?? this.rememberPassword,
        savedPassword: savedPassword ?? this.savedPassword,
      );
}

/// 基于 [cur] 构造新 ClientConfig（保留未改字段；frb 生成类无 copyWith）。
ClientConfig _configWith(
  ClientConfig cur, {
  String? serverUrl,
  bool? rememberPassword,
  String? password,
}) =>
    ClientConfig(
      serverUrl: serverUrl ?? cur.serverUrl,
      user: cur.user,
      boundDevice: cur.boundDevice,
      port: cur.port,
      opencodeBin: cur.opencodeBin,
      deviceId: cur.deviceId,
      theme: cur.theme,
      rememberPassword: rememberPassword ?? cur.rememberPassword,
      password: password ?? cur.password,
    );

class AuthNotifier extends Notifier<AuthState> {
  // —— 可覆写的 rust 桥接访问器（测试注入用；生产走全局 rust 库）——

  /// 读 config（测试覆写为内存操作，避免测试写真实磁盘）。
  ClientConfig readConfig() => rust.loadConfig();

  /// 写 config。
  Future<void> writeConfig(ClientConfig cfg) => rust.saveConfig(cfg: cfg);

  /// 清 config 登录态（rust logout）。
  Future<void> clearAuth() => rust.logout();

  /// rust 登录网络调用（测试覆写为内存假数据）。
  Future<AccountInfo> doRustLogin(String serverUrl, String key) =>
      rust.login(serverUrl: serverUrl, key: key);

  /// 设备上报（登录后主动触发，拿到自动绑定结果；测试注入内存假数据）。
  Future<rust.ReportOutcome> doReport() => rust.reportRegistration();

  @override
  AuthState build() {
    // 冷启动从 Rust 配置恢复。
    // 记住密码语义（config.json）：勾选 → 恢复登录态（自动登录）+ 密码自动填充；
    // 未勾选 → 不保留登录态，停在登录页。
    final cfg = rust.loadConfig();
    final remembered = cfg.rememberPassword && cfg.password.isNotEmpty && cfg.user != null;
    return AuthState(
      userId: remembered ? cfg.user?.id : null,
      userName: remembered ? cfg.user?.name : null,
      boundDevice: remembered ? cfg.boundDevice : null,
      rememberPassword: cfg.rememberPassword,
      savedPassword: cfg.rememberPassword ? cfg.password : null,
    );
  }

  /// 登录并返回设备清单（页面据此进入绑定流程）。
  ///
  /// [remember] 为记住密码勾选状态：
  /// - true：密码明文写入 config，保留登录态（下次启动自动登录）
  /// - false：删除 config 中的密码；**config 登录态保留**（本次会话仍可绑定设备、
  ///   上报注册；仅当下次冷启动时因 config 无密码而停在登录页）
  Future<AccountInfo> login(String serverUrl, String key, {bool remember = false}) async {
    final info = await doRustLogin(serverUrl, key);
    // 记住密码落盘到 config.json（Rust login 已写 server_url/user/bound，
    // 这里补 remember/password）
    final cfg = readConfig();
    await writeConfig(_configWith(cfg,
        rememberPassword: remember, password: remember ? key : ''));
    // 登录态保留（无论勾选与否）：绑定设备 / 设备上报都依赖 config.user。
    // 「不记住」只影响下次冷启动（config 无密码 → build() 不恢复登录态）。
    //
    // 绑定恢复（I16）：Rust login 已保留清单中仍存在的 bound_device——
    // 登录后立即从 config 读回，云服务开关不再依赖「弹层重选」。
    String? boundDevice = cfg.boundDevice;
    // 登录后主动上报一次：同步完成设备上报与自动绑定。approved+enabled 设备会被
    // Rust 侧 auto_bind 写 config.bound_device，但那是 Rust 后台异步、Dart 内存态
    // 不感知——若不主动刷新，主页隧道开关读旧内存仍置灰（「已绑定却提示未绑定」）。
    // reportRegistration 幂等，与 Rust 后台静默上报重复无害。
    try {
      final outcome = await doReport();
      if (outcome.bound) {
        boundDevice = readConfig().boundDevice ?? boundDevice;
      }
    } catch (_) {
      // 上报失败静默降级：boundDevice 保持 config 原值（不影响登录）
    }
    // 失效配置缓存：让 login_page 后续读 rustCfgProvider 拿到最新的 boundDevice
    ref.invalidate(rustCfgProvider);
    state = AuthState(
      userId: info.id,
      userName: info.name,
      boundDevice: boundDevice,
      rememberPassword: remember,
      savedPassword: remember ? key : null,
    );
    return info;
  }

  /// 更新记住密码勾选（登录页勾选框实时同步 config）。
  Future<void> setRememberPassword(bool remember, {String? password}) async {
    final cfg = readConfig();
    await writeConfig(_configWith(cfg,
        rememberPassword: remember,
        password: remember ? (password ?? cfg.password) : ''));
    state = state.copyWith(
        rememberPassword: remember,
        savedPassword: remember ? (password ?? cfg.password) : null);
  }

  /// 保存服务器地址到 config（登录页设置页用）。
  Future<void> saveServerUrl(String serverUrl) async {
    final cfg = readConfig();
    await writeConfig(_configWith(cfg, serverUrl: serverUrl.trim()));
    // 失效缓存：serverUrlEffectiveProvider / rustCfgProvider 均为无 autoDispose 的
    // 缓存 Provider，保存后必须 invalidate。否则登录页读到的仍是旧地址，Rust login
    // 会用旧地址写回 config.json，覆盖本次保存（表现为「保存未生效、退回旧地址」）。
    ref.invalidate(serverUrlEffectiveProvider);
    ref.invalidate(rustCfgProvider);
  }

  Future<void> bindDevice(String name) async {
    final os = Platform.isMacOS ? 'macos' : 'windows';
    await rust.bindDevice(deviceName: name, pctype: os, os: os);
    state = state.copyWith(boundDevice: name);
  }

  Future<void> logout() async {
    await clearAuth();
    // 登出时清除 config 中记住的密码与勾选
    final cfg = readConfig();
    await writeConfig(_configWith(cfg, rememberPassword: false, password: ''));
    state = const AuthState();
  }
}

final authProvider =
    NotifierProvider<AuthNotifier, AuthState>(AuthNotifier.new);

/// 生效服务器地址（config.json 的 server_url → 默认 https://oc.isoops.com）。
final serverUrlEffectiveProvider = Provider<String>((ref) => rust.serverUrlEffective());

/// 本地配置只读快照（避免 UI 直接触 Rust 桥接，测试可注入）。
final rustCfgProvider = Provider<ClientConfig>((ref) => rust.loadConfig());

/// 服务运行状态流。
final serviceStateProvider = StreamProvider<ServiceState>((ref) {
  return rust.subscribeState();
});

// ---------------------------------------------------------------------------
// 服务启停动作（可测试注入；端口占用弹框依赖的桥接函数同此模式）
// ---------------------------------------------------------------------------

/// 启动本地服务动作（默认直连 rust；测试注入内存实现）。
typedef StartLocalFn = Future<void> Function();
final startLocalFnProvider = Provider<StartLocalFn>((ref) => rust.startLocal);

/// 停止本地服务（保留隧道，I20）。
typedef StopLocalFn = Future<void> Function();
final stopLocalFnProvider = Provider<StopLocalFn>((ref) => rust.stopLocal);

/// 仅启动隧道（rathole，I20）。
typedef StartTunnelFn = Future<void> Function();
final startTunnelFnProvider = Provider<StartTunnelFn>((ref) => rust.startTunnel);

/// 停止隧道（保留本地服务，I20）。
typedef StopTunnelFn = Future<void> Function();
final stopTunnelFnProvider = Provider<StopTunnelFn>((ref) => rust.stopTunnel);

/// 启动全部（本地服务 + 隧道，I20）。
typedef StartAllFn = Future<void> Function();
final startAllFnProvider = Provider<StartAllFn>((ref) => rust.startAll);

/// 停止全部服务动作。
typedef StopAllFn = Future<void> Function();
final stopAllFnProvider = Provider<StopAllFn>((ref) => rust.stopAll);

/// 端口占用错误前缀（Dart 侧常量即可——与 Rust `PORT_ERR_PREFIX` 同值契约，
/// 由 rust/tests/portkill_test.rs 的 `port_err_prefix_is_machine_readable` 锁定；
/// 单独 provider 化供测试注入，避免测试环境初始化 frb 桥接）。
typedef PortErrPrefixFn = String Function();
final portErrPrefixFnProvider =
    Provider<PortErrPrefixFn>((ref) => () => rust.portErrPrefix());

/// 端口占用查询（端口占用弹框数据源）。
typedef QueryPortOccupantFn = PortOccupant? Function(int port);
final queryPortOccupantFnProvider = Provider<QueryPortOccupantFn>(
    (ref) => (port) => rust.queryPortOccupant(port: port));

/// 杀死端口占用者。
typedef KillPortOccupantFn = Future<void> Function(int port);
final killPortOccupantFnProvider = Provider<KillPortOccupantFn>(
    (ref) => (port) => rust.killPortOccupant(port: port));

/// 项目列表（可测试注入）。
final listProjectsProvider = FutureProvider<List<rust.ProjectSummary>>((ref) async {
  return rust.listProjects();
});

/// 指定目录的会话列表（可测试注入）。
final listSessionsProvider =
    FutureProvider.family<List<rust.SessionSummary>, String>((ref, directory) async {
  return rust.listSessions(directory: directory);
});

/// 终端打开动作（可测试注入，参数断言用）。
typedef OpenTerminalFn = Future<void> Function(String directory, String? sessionId);
final openInTerminalProvider = Provider<OpenTerminalFn>((ref) {
  return (directory, sessionId) => rust.openInTerminal(directory: directory, sessionId: sessionId);
});

// ---------------------------------------------------------------------------
// 设备上报（设置页手动触发；自动上报在 Rust 侧 start_cloud 前置链路）
// ---------------------------------------------------------------------------

/// 上报动作（可测试注入）：默认调 rust.reportRegistration()。
typedef ReportRegistrationFn = Future<rust.ReportOutcome> Function();
final reportRegistrationFnProvider = Provider<ReportRegistrationFn>((ref) {
  return () => rust.reportRegistration();
});

/// 保存配置动作（可测试注入）：默认调 rust.saveConfig()。
typedef SaveConfigFn = Future<void> Function({required ClientConfig cfg});
final saveConfigFnProvider = Provider<SaveConfigFn>((ref) {
  return ({required ClientConfig cfg}) => rust.saveConfig(cfg: cfg);
});

/// 只读上报状态查询（可测试注入）：调 web 端 /api/user/info 判断本机设备
/// 真实状态（I12：App 重启后内存态丢失，设置页进入时回填）。
typedef QueryRegistrationFn = Future<rust.ReportOutcome> Function();
final queryRegistrationFnProvider = Provider<QueryRegistrationFn>(
    (ref) => rust.queryRegistration);

/// 最近一次设备上报结果（null = 本次会话尚未上报；内存态，不持久化，不做轮询）。
/// 进入设置页时会经 [queryRegistrationFnProvider] 从 web 端拉真实状态回填（I12）。
class RegistrationNotifier extends Notifier<rust.ReportOutcome?> {
  @override
  rust.ReportOutcome? build() => null;

  /// 从 web 端查询真实上报状态（只读，I12）。
  ///
  /// 仅查询成功（ok=true）时写入——失败（网络不可达等）不覆盖任何现有状态：
  /// 内存态为 null 时保持「尚未上报」（可手动触发上报），已有成功态时保留。
  Future<void> restoreFromServer() async {
    try {
      final outcome = await ref.read(queryRegistrationFnProvider)();
      if (outcome.ok) {
        state = outcome;
      }
    } catch (_) {
      // 查询异常静默降级：保留现有状态（含 null → 「尚未上报」可手动触发）
    }
  }

  /// 手动触发上报。永不抛出：失败结果（ok=false）也装入 state 供 UI 展示。
  ///
  /// 上报成功后（Rust 侧已把新 deviceId 写回 config.json）刷新 [rustCfgProvider]，
  /// 避免设置页后续「保存设置」用陈旧快照把 deviceId 抹回旧值。
  /// 自动绑定成功时同步刷新登录态内存（[authProvider].boundDevice），
  /// 否则主页云服务开关读旧内存置灰，需重启才能恢复。
  Future<void> report() async {
    try {
      final outcome = await ref.read(reportRegistrationFnProvider)();
      state = outcome;
      if (outcome.ok) {
        // invalidate + 读最新 config（容错：测试/桥接未初始化时跳过内存刷新，
        // 不影响 state 本身）。auto_bind 已写盘 → config 含 bound_device。
        try {
          final cfg = ref.refresh(rustCfgProvider);
          if (outcome.bound && cfg.boundDevice != null) {
            final auth = ref.read(authProvider.notifier);
            auth.state = auth.state.copyWith(boundDevice: cfg.boundDevice);
          }
        } catch (_) {
          // 刷新登录态失败不影响上报结果展示
        }
      }
    } catch (_) {
      // Rust 侧承诺永不 panic，此处兜底防御（如桥接未初始化）
      state = rust.ReportOutcome(
        ok: false,
        status: '',
        deviceId: '',
        enabled: false,
        bound: false,
        error: '上报失败：桥接异常',
      );
    }
  }
}

final registrationProvider =
    NotifierProvider<RegistrationNotifier, rust.ReportOutcome?>(
        RegistrationNotifier.new);