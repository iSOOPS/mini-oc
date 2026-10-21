import 'dart:io';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'src/rust/api.dart' as rust;
import 'src/rust/account.dart' show AccountInfo;
import 'src/rust/config.dart' show ClientConfig;
import 'src/rust/frb_generated.dart';
import 'src/rust/supervisor.dart' show ServiceState;

/// Rust 桥接初始化。
final rustReadyProvider = FutureProvider<void>((ref) async {
  await RustLib.init();
});

/// 会话状态：已登录用户 + 绑定设备。
class AuthState {
  const AuthState({this.userId, this.userName, this.boundDevice});
  final String? userId;
  final String? userName;
  final String? boundDevice;

  bool get isLoggedIn => userId != null;
  AuthState copyWith({String? userId, String? userName, String? boundDevice}) => AuthState(
        userId: userId ?? this.userId,
        userName: userName ?? this.userName,
        boundDevice: boundDevice ?? this.boundDevice,
      );
}

class AuthNotifier extends Notifier<AuthState> {
  @override
  AuthState build() {
    // 冷启动从 Rust 配置恢复
    final cfg = rust.loadConfig();
    return AuthState(
      userId: cfg.user?.id,
      userName: cfg.user?.name,
      boundDevice: cfg.boundDevice,
    );
  }

  /// 登录并返回设备清单（页面据此进入绑定流程）。
  Future<AccountInfo> login(String serverUrl, String key) async {
    final info = await rust.login(serverUrl: serverUrl, key: key);
    state = AuthState(userId: info.id, userName: info.name);
    return info;
  }

  Future<void> bindDevice(String name) async {
    final os = Platform.isMacOS ? 'macos' : 'windows';
    await rust.bindDevice(deviceName: name, pctype: os, os: os);
    state = state.copyWith(boundDevice: name);
  }

  Future<void> logout() async {
    await rust.logout();
    state = const AuthState();
  }
}

final authProvider =
    NotifierProvider<AuthNotifier, AuthState>(AuthNotifier.new);

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
