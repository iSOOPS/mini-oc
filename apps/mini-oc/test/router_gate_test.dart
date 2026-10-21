import 'dart:io';
import 'package:flutter/material.dart';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated_io.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';

import 'package:mini_oc/shell.dart';
import 'package:mini_oc/stores.dart';
import 'package:mini_oc/src/rust/frb_generated.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  setUpAll(() async {
    // Rust 桥接（AuthNotifier.build 冷启动会读本地配置）
    final candidates = [
      '../rust/target/debug/libmini_oc_app.dylib',
      'rust/target/debug/libmini_oc_app.dylib',
    ];
    final path = candidates.firstWhere((c) => File(c).existsSync(), orElse: () => candidates.first);
    await RustLib.init(externalLibrary: ExternalLibrary.open(path));
    // 空配置目录 → 未登录态
    // ignore: invalid_use_of_visible_for_testing
    // （MINI_OC_HOME 由 rust 侧进程 env 读取——测试进程注入）
    // rust 侧读取的是自身进程 env，Dart VM 与 native 同进程，set env 经 Process? 不行——
    // 用另一种方式：直接断言 AuthState 语义（不触发 rust 配置读取）。
  });

  testWidgets('AuthState 门控语义', (tester) async {
    const anon = AuthState();
    expect(anon.isLoggedIn, isFalse);
    const named = AuthState(userId: '100001', userName: 'samuel');
    expect(named.isLoggedIn, isTrue);
    expect(named.copyWith(boundDevice: 'd').boundDevice, 'd');
  });

  testWidgets('宽屏渲染 NavigationRail，窄屏渲染 NavigationBar', (tester) async {
    final router = GoRouter(
      initialLocation: '/home',
      routes: [
        ShellRoute(
          builder: (_, __, child) => AppShell(child: child),
          routes: [
            GoRoute(path: '/home', builder: (_, __) => const SizedBox()),
          ],
        ),
      ],
    );
    Widget build() => MaterialApp.router(routerConfig: router);

    tester.view.physicalSize = const Size(1400, 900);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(build());
    expect(find.byType(NavigationRail), findsOneWidget);
    expect(find.byType(NavigationBar), findsNothing);

    tester.view.physicalSize = const Size(500, 900);
    await tester.pumpWidget(build());
    await tester.pumpAndSettle();
    expect(find.byType(NavigationBar), findsOneWidget);
    expect(find.byType(NavigationRail), findsNothing);
  });
}
