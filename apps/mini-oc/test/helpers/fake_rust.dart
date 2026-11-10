import 'dart:async';

import 'package:flutter_riverpod/misc.dart' show Override;
import 'package:mini_oc/src/rust/account.dart' show AccountInfo, DeviceBrief;
import 'package:mini_oc/src/rust/api.dart' as rust;
import 'package:mini_oc/src/rust/config.dart' show ClientConfig;

import 'package:mini_oc/stores.dart';

/// 便捷构造测试用 ClientConfig（补齐 port/theme 等无关字段）。
ClientConfig testCfg({
  String serverUrl = '',
  bool rememberPassword = false,
  String password = '',
  String? boundDevice,
}) =>
    ClientConfig(
      serverUrl: serverUrl,
      boundDevice: boundDevice,
      port: 9464,
      theme: 'system',
      rememberPassword: rememberPassword,
      password: password,
    );

/// Rust 桥接调用的内存记录器：拦截 stores 里所有 rust.* 调用。
class RustCallLog {
  bool logoutCalled = false;
  final List<ClientConfig> savedCfgs = [];

  String? lastLoginServer;
  String? lastLoginKey;
}

/// 内存版 Fake Rust 桥接：记录调用、返回预置数据，供登录相关 UI 测试注入。
class FakeRustBridge {
  FakeRustBridge({ClientConfig? cfg, this.deviceBriefs = const []})
      : cfg = cfg ?? testCfg();

  ClientConfig cfg;
  final List<DeviceBrief> deviceBriefs;

  /// rust 桥接调用记录（含 logout 与 saveConfig）。
  final RustCallLog calls = RustCallLog();

  /// login 收到的最后参数（断言用）。
  String? get lastLoginServer => calls.lastLoginServer;
  String? get lastLoginKey => calls.lastLoginKey;

  /// logout 是否被调用（登录流程中禁止）。
  bool get logoutCalled => calls.logoutCalled;

  /// login 收到的 remember 参数（由 FakeAuthNotifier 写入）。
  bool? lastRemember;

  /// 模拟登录后上报结果：bound=true 表示服务端设备已可自动绑定。
  bool reportBound = false;

  /// 模拟 Rust auto_bind 写回的 bound_device 名称（doReport 据此更新 cfg）。
  String? autoBindName;

  List<Override> overrides() => [
        serverUrlEffectiveProvider.overrideWith(
            (ref) => cfg.serverUrl.isEmpty ? 'https://oc.isoops.com' : cfg.serverUrl),
        authProvider.overrideWith(() => FakeAuthNotifier(this)),
        rustCfgProvider.overrideWith((ref) => cfg),
      ];

  ClientConfig saveConfig(ClientConfig next) {
    cfg = next;
    calls.savedCfgs.add(next);
    return cfg;
  }
}

/// Fake 登录：**复用真实 AuthNotifier 全部业务逻辑**（login 的 config 写入、
/// 绑定分支、logout 决策），仅覆写 rust 桥接访问器为内存操作。
class FakeAuthNotifier extends AuthNotifier {
  FakeAuthNotifier(this.rustLog);
  final FakeRustBridge rustLog;

  /// 记录 login 收到的 remember 参数。
  bool? lastRememberValue;

  @override
  AuthState build() {
    final cfg = rustLog.cfg;
    return AuthState(
      rememberPassword: cfg.rememberPassword,
      savedPassword: cfg.rememberPassword ? cfg.password : null,
    );
  }

  // —— 拦截 rust 桥接访问器（内存化，不触真实磁盘/网络）——

  @override
  ClientConfig readConfig() => rustLog.cfg;

  @override
  Future<void> writeConfig(ClientConfig cfg) async {
    rustLog.saveConfig(cfg);
  }

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

  /// 拦截 rust.reportRegistration：可配置模拟自动绑定（auto_bind 写 config）。默认 bound=false 不改变 boundDevice。
  @override
  Future<rust.ReportOutcome> doReport() async {
    if (rustLog.reportBound && rustLog.autoBindName != null) {
      // 模拟 Rust auto_bind：approved+enabled 设备自动写入 config.bound_device
      final c = rustLog.cfg;
      rustLog.cfg = ClientConfig(
        serverUrl: c.serverUrl,
        user: c.user,
        boundDevice: rustLog.autoBindName,
        port: c.port,
        opencodeBin: c.opencodeBin,
        deviceId: c.deviceId,
        theme: c.theme,
        rememberPassword: c.rememberPassword,
        password: c.password,
      );
    }
    return rust.ReportOutcome(
        ok: true,
        status: 'approved',
        deviceId: '',
        enabled: true,
        bound: rustLog.reportBound,
        error: null);
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
  Future<void> bindDevice(String name) async {
    await rust.bindDevice(deviceName: name, pctype: 'macos', os: 'macos');
    state = state.copyWith(boundDevice: name);
  }
}