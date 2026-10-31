import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_riverpod/misc.dart' show Override;
import 'package:mini_oc/src/rust/account.dart' show AccountInfo, DeviceBrief;
import 'package:mini_oc/src/rust/api.dart' as rust;
import 'package:mini_oc/src/rust/api/appenv.dart' show AppEnv;
import 'package:mini_oc/src/rust/config.dart' show ClientConfig;

import 'package:mini_oc/stores.dart';

/// Rust 桥接调用的内存记录器：拦截 stores 里所有 rust.* 调用。
class RustCallLog {
  bool logoutCalled = false;
  final List<AppEnv> savedEnvs = [];

  String? lastLoginServer;
  String? lastLoginKey;
}

/// 内存版 Fake Rust 桥接：记录调用、返回预置数据，供登录相关 UI 测试注入。
class FakeRustBridge {
  FakeRustBridge({required this.appEnv, this.deviceBriefs = const []});

  AppEnv appEnv;
  final List<DeviceBrief> deviceBriefs;

  /// rustCfgProvider 返回的配置（默认空配置；测试可改写以模拟 config.json 状态，
  /// 如 I16 的 boundDevice 保留）。
  ClientConfig cfg = const ClientConfig(
    serverUrl: '',
    port: 9464,
    theme: 'system',
  );

  /// rust 桥接调用记录（含 logout 与 saveAppEnv）。
  final RustCallLog calls = RustCallLog();

  /// login 收到的最后参数（断言用）。
  String? get lastLoginServer => calls.lastLoginServer;
  String? get lastLoginKey => calls.lastLoginKey;

  /// logout 是否被调用（登录流程中禁止）。
  bool get logoutCalled => calls.logoutCalled;

  /// login 收到的 remember 参数（由 FakeAuthNotifier 写入）。
  bool? lastRemember;

  List<Override> overrides() => [
        appEnvProvider.overrideWithValue(appEnv),
        serverUrlEffectiveProvider
            .overrideWithValue(appEnv.serverUrl.isEmpty ? 'https://oc.isoops.com' : appEnv.serverUrl),
        authProvider.overrideWith(() => FakeAuthNotifier(this)),
        rustCfgProvider.overrideWith((ref) => cfg),
      ];

  AppEnv saveEnv(AppEnv next) {
    appEnv = next;
    calls.savedEnvs.add(next);
    return appEnv;
  }
}

/// Fake 登录：**复用真实 AuthNotifier 全部业务逻辑**（login 的 env 写入、
/// 绑定分支、logout 决策），仅覆写 rust 桥接访问器为内存操作。
class FakeAuthNotifier extends AuthNotifier {
  FakeAuthNotifier(this.rustLog);
  final FakeRustBridge rustLog;

  /// 记录 login 收到的 remember 参数。
  bool? lastRememberValue;

  @override
  AuthState build() {
    final env = rustLog.appEnv;
    return AuthState(
      rememberPassword: env.rememberPassword,
      savedPassword: env.rememberPassword ? env.password : null,
    );
  }

  // —— 拦截 rust 桥接访问器（内存化，不触真实磁盘/网络）——

  @override
  Future<void> persistEnv(AppEnv env) async {
    rustLog.saveEnv(env);
  }

  @override
  AppEnv readEnv() => rustLog.appEnv;

  @override
  Future<void> clearAuth() async {
    rustLog.calls.logoutCalled = true;
  }

  /// 拦截 rust.login（网络调用）：返回预置账户信息，不触真实服务器。
  @override
  Future<AccountInfo> doRustLogin(String serverUrl, String key) {
    return Future.value(AccountInfo(
      id: '100001',
      name: 'tester',
      devices: rustLog.deviceBriefs,
    ));
  }

  @override
  Future<AccountInfo> login(String serverUrl, String key, {bool remember = false}) {
    rustLog.calls.lastLoginServer = serverUrl;
    rustLog.calls.lastLoginKey = key;
    lastRememberValue = remember;
    rustLog.lastRemember = remember;
    // 关键：调 super.login() 走真实 AuthNotifier 业务逻辑
    return super.login(serverUrl, key, remember: remember);
  }

  @override
  Future<void> saveServerUrl(String serverUrl) {
    rustLog.saveEnv(AppEnv(
      serverUrl: serverUrl,
      rememberPassword: rustLog.appEnv.rememberPassword,
      password: rustLog.appEnv.password,
    ));
    return Future.value();
  }

  @override
  Future<void> bindDevice(String name) async {
    await rust.bindDevice(deviceName: name, pctype: 'macos', os: 'macos');
    state = state.copyWith(boundDevice: name);
  }
}
