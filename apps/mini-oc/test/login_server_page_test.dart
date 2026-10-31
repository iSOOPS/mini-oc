import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';
import 'package:mini_oc/src/rust/api/appenv.dart' show AppEnv;

import 'package:mini_oc/pages/login_server_page.dart';

import 'helpers/fake_rust.dart';

void main() {
  testWidgets('设置页预填当前地址，修改后保存到 env 并返回', (tester) async {
    final fake = FakeRustBridge(
      appEnv: const AppEnv(
          serverUrl: 'https://old.example.com', rememberPassword: false, password: ''),
    );
    final router = GoRouter(
      initialLocation: '/login/server',
      routes: [
        GoRoute(path: '/login', builder: (_, __) => const Scaffold(body: Text('LOGIN'))),
        GoRoute(path: '/login/server', builder: (_, __) => const LoginServerPage()),
      ],
    );
    await tester.pumpWidget(
      ProviderScope(overrides: fake.overrides(), child: MaterialApp.router(routerConfig: router)),
    );
    await tester.pumpAndSettle();

    // 预填当前地址
    expect(find.widgetWithText(TextField, 'https://old.example.com'), findsOneWidget);

    // 修改并保存
    await tester.enterText(find.byType(TextField), 'https://oc.isoops.com');
    await tester.tap(find.text('保存'));
    await tester.pumpAndSettle();

    expect(fake.appEnv.serverUrl, 'https://oc.isoops.com', reason: '新地址写入 env');
    expect(find.text('LOGIN'), findsOneWidget, reason: '保存后返回登录页');
  });
}
