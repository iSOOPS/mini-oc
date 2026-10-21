import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';

import 'package:mini_oc/pages/home_page.dart';
import 'package:mini_oc/pages/login_page.dart';
import 'package:mini_oc/stores.dart';
import 'package:mini_oc/src/rust/account.dart' show AccountInfo, DeviceBrief;
import 'package:mini_oc/src/rust/config.dart' show ClientConfig;
import 'package:mini_oc/src/rust/supervisor.dart' show ServiceMode, ServiceState;

/// 可编排的 AuthNotifier：记录 login/bindDevice 调用。
class _ScriptedAuth extends AuthNotifier {
  _ScriptedAuth({required this.info});
  final AccountInfo info;
  final binds = <String>[];

  @override
  AuthState build() => const AuthState();

  @override
  Future<AccountInfo> login(String serverUrl, String key) async {
    state = AuthState(userId: info.id, userName: info.name);
    return info;
  }

  @override
  Future<void> bindDevice(String name) async {
    binds.add(name);
    state = state.copyWith(boundDevice: name);
  }
}

AccountInfo _info(List<DeviceBrief> devices) => AccountInfo(
      id: '100001',
      name: 'samuel',
      devices: devices,
    );

DeviceBrief _dev(String name) => DeviceBrief(
      name: name,
      port: 9464,
      pctype: 'macos',
      bound: false,
      deviceName: '',
      publicUrl: 'https://oc.example.com',
    );

Widget app(_ScriptedAuth auth) {
  final router = GoRouter(
    initialLocation: '/login',
    routes: [
      GoRoute(path: '/login', builder: (_, __) => const LoginPage()),
      GoRoute(path: '/home', builder: (_, __) => const HomePage()),
    ],
  );
  return ProviderScope(
    overrides: [
      authProvider.overrideWith(() => auth),
      // HomePage 依赖（避免触碰真实 Rust）
      serviceStateProvider.overrideWith(
        (ref) => Stream.value(const ServiceState(mode: ServiceMode.off, port: 0)),
      ),
      rustCfgProvider.overrideWithValue(
        const ClientConfig(serverUrl: 'https://oc.example.com', port: 9464, theme: 'system'),
      ),
    ],
    child: MaterialApp.router(routerConfig: router),
  );
}

void main() {
  testWidgets('登录成功且有设备时弹出选择层，选择后绑定（I5）', (tester) async {
    final auth = _ScriptedAuth(info: _info([_dev('macbook-pro'), _dev('office-win')]));
    await tester.pumpWidget(app(auth));
    await tester.pump(const Duration(milliseconds: 300));

    await tester.enterText(find.byType(TextField).at(0), 'https://oc.example.com');
    await tester.enterText(find.byType(TextField).at(1), 'somekey');
    await tester.tap(find.text('登录'));
    await tester.pump(const Duration(milliseconds: 300));

    expect(find.text('选择要绑定的设备'), findsOneWidget, reason: '绑定弹层出现');
    expect(find.textContaining('macbook-pro'), findsOneWidget);

    await tester.tap(find.textContaining('macbook-pro'));
    await tester.pump(const Duration(milliseconds: 300));

    expect(auth.binds, ['macbook-pro'], reason: 'bindDevice 被调用且参数正确');
    expect(find.byType(HomePage), findsOneWidget, reason: '绑定后进入概览');
  });

  testWidgets('登录成功无设备直接进入概览', (tester) async {
    final auth = _ScriptedAuth(info: _info([]));
    await tester.pumpWidget(app(auth));
    await tester.pump(const Duration(milliseconds: 300));

    await tester.enterText(find.byType(TextField).at(0), 'https://oc.example.com');
    await tester.enterText(find.byType(TextField).at(1), 'somekey');
    await tester.tap(find.text('登录'));
    await tester.pump(const Duration(milliseconds: 300));

    expect(find.text('选择要绑定的设备'), findsNothing);
    expect(find.byType(HomePage), findsOneWidget);
  });
}
