import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_riverpod/misc.dart' show Override;
import 'package:flutter_test/flutter_test.dart';
import 'package:mini_oc/pages/settings_page.dart';
import 'package:mini_oc/stores.dart';
import 'package:mini_oc/src/rust/api.dart' show ReportOutcome;
import 'package:mini_oc/src/rust/api/appenv.dart' show AppEnv;
import 'package:mini_oc/src/rust/config.dart' show ClientConfig;

/// 固定初始态 AuthNotifier（绕过 Rust 配置读取）。
class _FixedAuth extends AuthNotifier {
  _FixedAuth(this.initial);
  final AuthState initial;
  @override
  AuthState build() => initial;
}

List<Override> _overrides(ReportOutcome Function() query) => [
      authProvider.overrideWith(
          () => _FixedAuth(const AuthState(userId: '1', userName: 'u'))),
      appEnvProvider.overrideWithValue(
          AppEnv(serverUrl: '', rememberPassword: false, password: '')),
      rustCfgProvider.overrideWith((ref) => ClientConfig(
            serverUrl: 'https://oc.example.com',
            port: 9464,
            theme: 'system',
          )),
      queryRegistrationFnProvider.overrideWithValue(() async => query()),
    ];

void main() {
  testWidgets('设置页进入且内存态为空（App 重启后）→ 自动查询 web 端真实上报状态',
      (tester) async {
    await tester.pumpWidget(
      ProviderScope(
        overrides: _overrides(() => ReportOutcome(
              ok: true,
              status: 'approved',
              deviceId: 'dev-1',
              enabled: true,
              bound: true,
            )),
        child: const MaterialApp(home: Scaffold(body: SettingsPage())),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.textContaining('已绑定'), findsOneWidget,
        reason: '查询结果显示真实上报状态（不再显示"尚未上报"）');
  });

  testWidgets('查询失败（网络不可达）→ 不误报，保留"尚未上报"可手动重试',
      (tester) async {
    await tester.pumpWidget(
      ProviderScope(
        overrides: _overrides(() => ReportOutcome(
              ok: false,
              status: '',
              deviceId: '',
              enabled: false,
              bound: false,
              error: '网络错误',
            )),
        child: const MaterialApp(home: Scaffold(body: SettingsPage())),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.textContaining('尚未上报'), findsOneWidget,
        reason: '查询失败不覆盖内存态，保留可重试文案');
  });
}
