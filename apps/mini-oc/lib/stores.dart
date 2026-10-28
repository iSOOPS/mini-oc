import 'dart:io';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'src/rust/api.dart' as rust;
import 'src/rust/account.dart' show AccountInfo;
import 'src/rust/api/appenv.dart' show AppEnv;
import 'src/rust/config.dart' show ClientConfig;
import 'src/rust/frb_generated.dart';
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

  /// env 中的记住密码勾选状态（启动回显勾选框）
  final bool rememberPassword;

  /// env 中的密码明文（rememberPassword = true 时用于自动填充）
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

class AuthNotifier extends Notifier<AuthState> {
  @override
  AuthState build() {
    // 冷启动从 Rust 配置恢复。
    // 记住密码语义（env 文件）：勾选 → 恢复登录态（自动登录）+ 密码自动填充；
    // 未勾选 → 不保留登录态，停在登录页。
    final cfg = rust.loadConfig();
    final env = rust.loadAppEnv();
    final remembered = env.rememberPassword && env.password.isNotEmpty && cfg.user != null;
    return AuthState(
      userId: remembered ? cfg.user?.id : null,
      userName: remembered ? cfg.user?.name : null,
      boundDevice: remembered ? cfg.boundDevice : null,
      rememberPassword: env.rememberPassword,
      savedPassword: env.rememberPassword ? env.password : null,
    );
  }

  /// 登录并返回设备清单（页面据此进入绑定流程）。
  ///
  /// [remember] 为记住密码勾选状态：
  /// - true：密码明文写入 env，保留登录态（下次启动自动登录）
  /// - false：删除 env 中的密码，清除登录态（下次启动停在登录页）
  Future<AccountInfo> login(String serverUrl, String key, {bool remember = false}) async {
    final info = await rust.login(serverUrl: serverUrl, key: key);
    // env 记住密码落盘（server_url / 勾选状态 / 密码明文）
    await rust.saveAppEnv(
      cfg: AppEnv(
        serverUrl: serverUrl,
        rememberPassword: remember,
        password: remember ? key : '',
      ),
    );
    if (remember) {
      state = AuthState(
        userId: info.id,
        userName: info.name,
        rememberPassword: true,
        savedPassword: key,
      );
    } else {
      // 不记住：登出清登录态（保留 server_url/port），下次启动停登录页
      await rust.logout();
      state = AuthState(
        rememberPassword: false,
        savedPassword: null,
      );
    }
    return info;
  }

  /// 更新记住密码勾选（登录页勾选框实时同步 env）。
  Future<void> setRememberPassword(bool remember, {String? serverUrl, String? password}) async {
    final env = rust.loadAppEnv();
    await rust.saveAppEnv(
      cfg: AppEnv(
        serverUrl: serverUrl ?? env.serverUrl,
        rememberPassword: remember,
        password: remember ? (password ?? env.password) : '',
      ),
    );
    state = state.copyWith(rememberPassword: remember, savedPassword: remember ? (password ?? env.password) : null);
  }

  /// 保存服务器地址到 env（登录页设置页用）。
  Future<void> saveServerUrl(String serverUrl) async {
    final env = rust.loadAppEnv();
    await rust.saveAppEnv(
      cfg: AppEnv(
        serverUrl: serverUrl.trim(),
        rememberPassword: env.rememberPassword,
        password: env.password,
      ),
    );
  }

  Future<void> bindDevice(String name) async {
    final os = Platform.isMacOS ? 'macos' : 'windows';
    await rust.bindDevice(deviceName: name, pctype: os, os: os);
    state = state.copyWith(boundDevice: name);
  }

  Future<void> logout() async {
    await rust.logout();
    // 登出时删除 env 中记住的密码与勾选
    final env = rust.loadAppEnv();
    await rust.saveAppEnv(
      cfg: AppEnv(
        serverUrl: env.serverUrl,
        rememberPassword: false,
        password: '',
      ),
    );
    state = const AuthState();
  }
}

final authProvider =
    NotifierProvider<AuthNotifier, AuthState>(AuthNotifier.new);

/// 应用 env 只读快照（地址 / 记住密码 / 密码；测试可注入）。
final appEnvProvider = Provider<AppEnv>((ref) => rust.loadAppEnv());

/// 生效服务器地址（env → config 兜底 → 默认 https://oc.isoops.com）。
final serverUrlEffectiveProvider = Provider<String>((ref) => rust.serverUrlEffective());

/// 本地配置只读快照（避免 UI 直接触 Rust 桥接，测试可注入）。
final rustCfgProvider = Provider<ClientConfig>((ref) => rust.loadConfig());

/// 服务运行状态流。
final serviceStateProvider = StreamProvider<ServiceState>((ref) {
  return rust.subscribeState();
});

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

/// 最近一次设备上报结果（null = 本次会话尚未上报；内存态，不持久化，不做轮询）。
class RegistrationNotifier extends Notifier<rust.ReportOutcome?> {
  @override
  rust.ReportOutcome? build() => null;

  /// 手动触发上报。永不抛出：失败结果（ok=false）也装入 state 供 UI 展示。
  Future<void> report() async {
    try {
      state = await ref.read(reportRegistrationFnProvider)();
    } catch (_) {
      // Rust 侧承诺永不 panic，此处兜底防御（如桥接未初始化）
      state = rust.ReportOutcome(
        ok: false,
        status: '',
        deviceId: '',
        enabled: false,
        error: '上报失败：桥接异常',
      );
    }
  }
}

final registrationProvider =
    NotifierProvider<RegistrationNotifier, rust.ReportOutcome?>(
        RegistrationNotifier.new);
