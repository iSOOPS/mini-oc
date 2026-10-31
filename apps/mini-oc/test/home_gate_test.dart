import 'dart:async';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:mini_oc/pages/home_page.dart';
import 'package:mini_oc/stores.dart';
import 'package:mini_oc/src/rust/supervisor.dart' show ServiceMode, ServiceState;

ServiceState _st(ServiceMode mode, {String? err}) => ServiceState(
      mode: mode,
      servePid: mode == ServiceMode.off ? null : 123,
      ratholePid: mode == ServiceMode.cloud ? 456 : null,
      port: 9464,
      lastError: err,
    );

/// 固定初始态的 AuthNotifier（绕过 Rust 配置读取）。
class _FixedAuth extends AuthNotifier {
  _FixedAuth(this.initial);
  final AuthState initial;
  @override
  AuthState build() => initial;
}

Widget wrap({required Stream<ServiceState> states, required AuthState auth}) {
  return ProviderScope(
    overrides: [
      authProvider.overrideWith(() => _FixedAuth(auth)),
      serviceStateProvider.overrideWith((ref) => states),
    ],
    child: const MaterialApp(home: HomePage()),
  );
}

void main() {
  testWidgets('未绑定设备时隧道开关置灰并显示引导', (tester) async {
    await tester.pumpWidget(wrap(
      states: Stream.value(_st(ServiceMode.off)),
      auth: const AuthState(userId: '100001', userName: 'samuel'), // boundDevice = null
    ));
    await tester.pumpAndSettle();

    // 云开关 tile 存在但 Switch 禁用（onChanged null）
    final tile = find.widgetWithText(ListTile, '启动隧道');
    expect(tile, findsOneWidget);
    final switchWidget = tester.widget<Switch>(
      find.descendant(of: tile, matching: find.byType(Switch)),
    );
    expect(switchWidget.onChanged, isNull, reason: '未绑定时隧道 Switch 禁用');

    // 本地服务开关不受影响（可切换）
    final localTile = find.widgetWithText(ListTile, '启动本地服务');
    final localSwitch = tester.widget<Switch>(
      find.descendant(of: localTile, matching: find.byType(Switch)),
    );
    expect(localSwitch.onChanged, isNotNull, reason: '本地服务不受绑定门控');

    // 点击置灰 tile 弹出绑定引导
    await tester.tap(tile);
    await tester.pumpAndSettle();
    expect(find.text('需要先绑定设备'), findsOneWidget);
  });

  testWidgets('状态流推 Off+错误后显示错误条与重试按钮', (tester) async {
    final ctrl = StreamController<ServiceState>.broadcast();
    await tester.pumpWidget(wrap(
      states: ctrl.stream,
      auth: const AuthState(userId: '100001', userName: 'samuel', boundDevice: 'macbook-pro'),
    ));
    ctrl.add(_st(ServiceMode.off, err: 'opencode serve 意外退出'));
    await tester.pumpAndSettle();

    expect(find.textContaining('意外退出'), findsOneWidget, reason: '错误条显示');
    expect(find.text('重试'), findsOneWidget);
    await ctrl.close();
  });

  testWidgets('全部运行中显示状态与 PID', (tester) async {
    await tester.pumpWidget(wrap(
      states: Stream.value(_st(ServiceMode.cloud)),
      auth: const AuthState(userId: '100001', userName: 'samuel', boundDevice: 'macbook-pro'),
    ));
    await tester.pumpAndSettle();
    expect(find.text('全部服务运行中'), findsOneWidget);
    expect(find.textContaining('PID'), findsOneWidget);
  });
}
