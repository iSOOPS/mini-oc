import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mini_oc/src/rust/api/appenv.dart' show AppEnv;

import 'package:mini_oc/pages/login_page.dart';

import 'helpers/fake_rust.dart';

void main() {
  testWidgets('勾选记住密码登录：env 写入密码与勾选状态，地址取自 env', (tester) async {
    // 地址已在设置页保存（env 中）
    final fake = FakeRustBridge(
      appEnv: const AppEnv(
          serverUrl: 'https://oc.example.com', rememberPassword: false, password: ''),
    );
    await tester.pumpWidget(_app(fake));
    await tester.pumpAndSettle();

    // 登录页仅密钥一个输入框（无地址框）
    expect(find.byType(TextField), findsOneWidget);
    await tester.enterText(find.byType(TextField), 'my-key-123');
    await tester.tap(find.text('记住密码'));
    await tester.tap(find.text('登录'));
    await tester.pumpAndSettle();

    expect(fake.lastRemember, isTrue, reason: '勾选状态传入 login');expect(fake.lastLoginServer, 'https://oc.example.com', reason: '地址取自 env（设置页保存）');
    expect(fake.appEnv.serverUrl, 'https://oc.example.com');
    expect(fake.appEnv.rememberPassword, isTrue, reason: 'env 记录勾选');
    expect(fake.appEnv.password, 'my-key-123', reason: 'env 记录密码明文');
  });  testWidgets('取消勾选后登录：env 删除密码，但登录态保留（绑定依赖 config user）', (tester) async {
    // 预置 env 已记住旧密码（页面回显勾选）
    final fake = FakeRustBridge(
      appEnv: const AppEnv(
          serverUrl: 'https://old.example.com', rememberPassword: true, password: 'old-pass'),
    );
    await tester.pumpWidget(_app(fake));
    await tester.pumpAndSettle();

    // 用户取消勾选，改密码后登录
    await tester.tap(find.text('记住密码'));
    await tester.enterText(find.byType(TextField), 'new-key');
    await tester.tap(find.text('登录'));
    await tester.pumpAndSettle();

    expect(fake.lastRemember, isFalse, reason: '取消勾选传入 login');
    expect(fake.logoutCalled, isFalse, reason: '登录流程不得调用 logout（会清 config user 导致绑定失败）');
    expect(fake.appEnv.serverUrl, 'https://old.example.com', reason: '地址不受登录影响');
    expect(fake.appEnv.rememberPassword, isFalse, reason: '勾选状态清除');
    expect(fake.appEnv.password, '', reason: 'env 中旧密码被删除');
  });

  testWidgets('env 记住密码：密码自动填充且勾选回显（冷启动场景）', (tester) async {
    final fake = FakeRustBridge(
      appEnv: const AppEnv(
          serverUrl: 'https://oc.example.com', rememberPassword: true, password: 'kept-pass'),
    );
    await tester.pumpWidget(_app(fake));
    await tester.pumpAndSettle();

    expect(find.widgetWithText(TextField, 'kept-pass'), findsOneWidget,
        reason: '启动自动填充密码');
    final checkbox = tester.widget<Checkbox>(find.byType(Checkbox));
    expect(checkbox.value, isTrue, reason: '勾选状态回显');
  });
}

Widget _app(FakeRustBridge fake) {
  return ProviderScope(
    overrides: fake.overrides(),
    child: const MaterialApp(home: LoginPage()),
  );
}
