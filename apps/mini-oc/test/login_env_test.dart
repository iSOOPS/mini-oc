import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';
import 'package:mini_oc/src/rust/api/appenv.dart' show AppEnv;

import 'package:mini_oc/main.dart' show routerProvider;
import 'package:mini_oc/pages/login_page.dart';
import 'package:mini_oc/stores.dart';

import 'helpers/fake_rust.dart';

void main() {
  testWidgets('登录页：无地址输入框，仅密码框；密码默认隐藏', (tester) async {
    final fake = FakeRustBridge(
      appEnv: const AppEnv(serverUrl: '', rememberPassword: false, password: ''),
    );
    await tester.pumpWidget(wrapWithProviders(fake, child: const LoginPage()));

    // 登录页仅一个输入框（用户密钥），不再有地址框
    expect(find.byType(TextField), findsOneWidget, reason: '登录页仅保留密钥输入框');
    final keyField = tester.widget<TextField>(find.byType(TextField));
    expect(keyField.obscureText, isTrue, reason: '密码默认隐藏');
  });

  testWidgets('登录页：勾选记住密码时密码框显示眼睛切换按钮', (tester) async {
    final fake = FakeRustBridge(
      appEnv: const AppEnv(serverUrl: '', rememberPassword: false, password: ''),
    );
    await tester.pumpWidget(wrapWithProviders(fake, child: const LoginPage()));

    // 密码框 suffixIcon 存在（眼睛）
    final keyFieldFinder = find.byWidgetPredicate((w) => w is TextField && w.obscureText == true);
    expect(keyFieldFinder, findsOneWidget);
    final field = tester.widget<TextField>(keyFieldFinder);
    expect(field.decoration?.suffixIcon, isNotNull, reason: '密码框有眼睛切换按钮');

    // 记住密码勾选框存在，默认未勾选
    final checkbox = tester.widget<Checkbox>(find.byType(Checkbox));
    expect(checkbox.value, isFalse, reason: '记住密码默认不勾选');
  });

  testWidgets('登录页：眼睛点击切换明文/密文', (tester) async {
    final fake = FakeRustBridge(
      appEnv: const AppEnv(serverUrl: '', rememberPassword: false, password: ''),
    );
    await tester.pumpWidget(wrapWithProviders(fake, child: const LoginPage()));

    await tester.enterText(
        find.byWidgetPredicate((w) => w is TextField && w.obscureText == true), 'my-secret');
    await tester.tap(find.byType(IconButton).first);
    await tester.pumpAndSettle();

    final fields = tester.widgetList<TextField>(find.byType(TextField)).toList();
    expect(fields.any((f) => f.obscureText == false && f.controller?.text == 'my-secret'),
        isTrue, reason: '点击眼睛后显示明文');
  });

  testWidgets('登录页：env 记住密码时自动填充密码并勾选', (tester) async {
    final fake = FakeRustBridge(
      appEnv: const AppEnv(
          serverUrl: 'https://oc.isoops.com', rememberPassword: true, password: 'saved-pass'),
    );
    await tester.pumpWidget(wrapWithProviders(fake, child: const LoginPage()));
    await tester.pumpAndSettle();

    expect(
        find.widgetWithText(TextField, 'saved-pass'), findsOneWidget,
        reason: '记住密码时自动填充');
    final checkbox = tester.widget<Checkbox>(find.byType(Checkbox));
    expect(checkbox.value, isTrue, reason: '记住密码时勾选框回显勾选');
  });

  testWidgets('登录页右上角设置图标 → 切换到服务器地址设置页', (tester) async {
    final fake = FakeRustBridge(
      appEnv: const AppEnv(serverUrl: '', rememberPassword: false, password: ''),
    );
    final router = GoRouter(
      initialLocation: '/login',
      routes: [
        GoRoute(path: '/login', builder: (_, __) => const LoginPage()),
        GoRoute(
            path: '/login/server',
            builder: (_, __) => const Scaffold(body: Text('SERVER_PAGE'))),
      ],
    );
    await tester.pumpWidget(
      ProviderScope(
        overrides: fake.overrides(),
        child: MaterialApp.router(routerConfig: router),
      ),
    );

    // 右上角设置图标（AppBar 或 Stack 角落）
    final settingBtn = find.byIcon(Icons.settings_outlined);
    expect(settingBtn, findsOneWidget, reason: '登录页右上角有设置图标');
    await tester.tap(settingBtn);
    await tester.pumpAndSettle();
    expect(find.text('SERVER_PAGE'), findsOneWidget, reason: '点击后切换到地址设置页');
  });

  testWidgets('真实路由守卫下：未登录点击设置图标仍可进入 /login/server（不被守卫弹回）', (tester) async {
    final fake = FakeRustBridge(
      appEnv: const AppEnv(serverUrl: '', rememberPassword: false, password: ''),
    );
    final container = ProviderContainer(overrides: fake.overrides());
    addTearDown(container.dispose);
    await tester.pumpWidget(
      UncontrolledProviderScope(
        container: container,
        child: MaterialApp.router(routerConfig: container.read(routerProvider)),
      ),
    );
    await tester.pumpAndSettle();

    // 当前在 /login（未登录守卫）
    final settingBtn = find.byIcon(Icons.settings_outlined);
    expect(settingBtn, findsOneWidget);
    await tester.tap(settingBtn);
    await tester.pumpAndSettle();

    expect(find.text('访问地址'), findsOneWidget,
        reason: '守卫必须放行 /login/server，点击设置后进入地址设置页');
  });
}

/// 仅含登录页所需 provider 的测试容器。
Widget wrapWithProviders(FakeRustBridge fake, {required Widget child}) {
  return ProviderScope(
    overrides: fake.overrides(),
    child: MaterialApp(home: child),
  );
}
