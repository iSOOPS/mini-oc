import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_riverpod/misc.dart' show Override;
import 'package:flutter_test/flutter_test.dart';
import 'package:mini_oc/pages/home_page.dart';
import 'package:mini_oc/stores.dart';
import 'package:mini_oc/src/rust/supervisor.dart' show ServiceMode, ServiceState;

ServiceState _st(ServiceMode mode) => ServiceState(
      mode: mode,
      servePid: mode == ServiceMode.off || mode == ServiceMode.tunnelOnly ? null : 123,
      ratholePid:
          mode == ServiceMode.off || mode == ServiceMode.localOnly ? null : 456,
      port: 9464,
    );

/// 启停动作记录 Fake（I20 新动作面）。
class StartStopFake {
  int startAllCalls = 0;
  int startTunnelCalls = 0;
  int startLocalCalls = 0;
  int stopAllCalls = 0;
  int stopTunnelCalls = 0;
  int stopLocalCalls = 0;

  List<Override> overrides(Stream<ServiceState> states) => [
        authProvider.overrideWith(() => _FixedAuth(const AuthState(
              userId: '100001',
              userName: 'samuel',
              boundDevice: 'macbook-pro',
            ))),
        serviceStateProvider.overrideWith((ref) => states),
        startAllFnProvider.overrideWithValue(() async => startAllCalls++),
        startTunnelFnProvider.overrideWithValue(() async => startTunnelCalls++),
        startLocalFnProvider.overrideWithValue(() async => startLocalCalls++),
        stopAllFnProvider.overrideWithValue(() async => stopAllCalls++),
        stopTunnelFnProvider.overrideWithValue(() async => stopTunnelCalls++),
        stopLocalFnProvider.overrideWithValue(() async => stopLocalCalls++),
      ];
}

class _FixedAuth extends AuthNotifier {
  _FixedAuth(this.initial);
  final AuthState initial;
  @override
  AuthState build() => initial;
}

Future<void> pump(WidgetTester tester, StartStopFake fake, ServiceMode mode) async {
  await tester.pumpWidget(
    ProviderScope(
      overrides: fake.overrides(Stream.value(_st(mode))),
      child: const MaterialApp(home: Scaffold(body: HomePage())),
    ),
  );
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('全部停止态：「启动全部」按钮 → startAll（I20）', (tester) async {
    final fake = StartStopFake();
    await pump(tester, fake, ServiceMode.off);

    await tester.tap(find.text('启动全部'));
    await tester.pumpAndSettle();
    expect(fake.startAllCalls, 1, reason: '停止态点启动全部 → startAll');
  });

  testWidgets('全部运行中：按钮变「停止全部」→ stopAll（I20）', (tester) async {
    final fake = StartStopFake();
    await pump(tester, fake, ServiceMode.cloud);

    expect(find.text('停止全部'), findsOneWidget);
    expect(find.text('启动全部'), findsNothing);
    await tester.tap(find.text('停止全部'));
    await tester.pumpAndSettle();
    expect(fake.stopAllCalls, 1);
  });

  testWidgets('仅隧道运行中：按钮显示「停止全部」；隧道开关关闭 → stopTunnel 保 serve（I20）',
      (tester) async {
    final fake = StartStopFake();
    await pump(tester, fake, ServiceMode.tunnelOnly);

    expect(find.text('停止全部'), findsOneWidget, reason: '任一运行即停止全部');

    final tunnelTile = find.widgetWithText(ListTile, '启动隧道');
    final sw = tester.widget<Switch>(
        find.descendant(of: tunnelTile, matching: find.byType(Switch)));
    expect(sw.value, isTrue, reason: '隧道运行中开关为开');

    await tester.tap(tunnelTile);
    await tester.pumpAndSettle();
    expect(fake.stopTunnelCalls, 1, reason: '关隧道 → stopTunnel（不清 serve）');
    expect(fake.stopAllCalls, 0);
  });

  testWidgets('本地运行中：本地开关关闭 → stopLocal 保隧道（I20）', (tester) async {
    final fake = StartStopFake();
    await pump(tester, fake, ServiceMode.localOnly);

    await tester.tap(find.widgetWithText(ListTile, '启动本地服务'));
    await tester.pumpAndSettle();
    expect(fake.stopLocalCalls, 1, reason: '关本地 → stopLocal（不清隧道）');
    expect(fake.stopAllCalls, 0);
  });

  testWidgets('隧道停止态：点隧道开关 → startTunnel（不启动 serve，I20）', (tester) async {
    final fake = StartStopFake();
    await pump(tester, fake, ServiceMode.off);

    await tester.tap(find.widgetWithText(ListTile, '启动隧道'));
    await tester.pumpAndSettle();
    expect(fake.startTunnelCalls, 1, reason: '仅启动隧道');
    expect(fake.startLocalCalls, 0, reason: '不得顺带启动 serve');
    expect(fake.startAllCalls, 0);
  });
}
