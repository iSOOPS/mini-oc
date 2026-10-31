import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_riverpod/misc.dart' show Override;
import 'package:flutter_test/flutter_test.dart';
import 'package:mini_oc/pages/home_page.dart';
import 'package:mini_oc/stores.dart';
import 'package:mini_oc/src/rust/api/appenv.dart' show AppEnv;

import 'package:mini_oc/src/rust/portkill.dart' show PortOccupant;
import 'package:mini_oc/src/rust/supervisor.dart' show ServiceMode, ServiceState;

import 'helpers/fake_rust.dart';

ServiceState _st(ServiceMode mode) => ServiceState(
      mode: mode,
      servePid: mode == ServiceMode.off ? null : 123,
      ratholePid: mode == ServiceMode.cloud ? 456 : null,
      port: 9464,
    );

/// 端口占用场景的可注入桥接 Fake。
class PortKillFakeRust extends FakeRustBridge {
  PortKillFakeRust({required this.portInUse})
      : super(appEnv: AppEnv(serverUrl: '', rememberPassword: false, password: ''));
  final bool portInUse;

  int killPortCalls = 0;

  @override
  List<Override> overrides() => [
        ...super.overrides(),
        portErrPrefixFnProvider.overrideWithValue(() => 'PORT_IN_USE:'),
        // 占用查询：portInUse=true 时返回固定占用者
        queryPortOccupantFnProvider.overrideWithValue((port) =>
            portInUse ? PortOccupant(pid: 4321, name: 'opencode') : null),
        killPortOccupantFnProvider.overrideWithValue((port) async {
          killPortCalls++;
        }),
      ];
}

void main() {
  testWidgets('启动本地服务遇端口占用 → 弹框展示占用进程 → 确认杀死并重试成功',
      (tester) async {
    final fake = PortKillFakeRust(portInUse: true);
    // 第一次 startLocal 抛端口占用错误，重试（杀死后）成功
    var startCalls = 0;
    final containerOverrides = [
      ...fake.overrides(),
      startLocalFnProvider.overrideWithValue(() async {
        startCalls++;
        if (startCalls == 1) {
          throw Exception('PORT_IN_USE:9464 已被占用');
        }
      }),
      serviceStateProvider.overrideWith((ref) => Stream.value(_st(ServiceMode.off))),
    ];

    await tester.pumpWidget(
      ProviderScope(
        overrides: containerOverrides,
        child: const MaterialApp(home: Scaffold(body: HomePage())),
      ),
    );
    await tester.pumpAndSettle();

    // 打开本地服务开关
    await tester.tap(find.text('启动本地服务'));
    await tester.pumpAndSettle();

    // 弹框出现：展示占用进程信息
    expect(find.textContaining('端口被占用'), findsOneWidget, reason: '弹框标题');
    expect(find.textContaining('4321'), findsOneWidget, reason: '占用进程 PID');
    expect(
      find.descendant(
        of: find.byType(AlertDialog),
        matching: find.textContaining('opencode'),
      ),
      findsOneWidget,
      reason: '弹框内占用进程名',
    );

    // 确认「结束并重试」
    await tester.tap(find.text('结束进程并重试'));
    await tester.pumpAndSettle();

    expect(fake.killPortCalls, 1, reason: '杀端口被调用一次');
    expect(startCalls, 2, reason: '杀死后自动重试启动');
    expect(find.textContaining('服务已启动'), findsOneWidget, reason: '重试成功提示');
    expect(find.textContaining('端口被占用'), findsNothing, reason: '弹框已关闭');
  });

  testWidgets('占用查询无结果 → 普通错误提示不弹框', (tester) async {
    final fake = PortKillFakeRust(portInUse: false);
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          ...fake.overrides(),
          startLocalFnProvider.overrideWithValue(() async {
            throw Exception('PORT_IN_USE:9464 已被占用');
          }),
          serviceStateProvider.overrideWith((ref) => Stream.value(_st(ServiceMode.off))),
        ],
        child: const MaterialApp(home: Scaffold(body: HomePage())),
      ),
    );
    await tester.pumpAndSettle();

    await tester.tap(find.text('启动本地服务'));
    await tester.pumpAndSettle();

    expect(find.textContaining('端口被占用'), findsNothing, reason: '无占用者不弹框');
    expect(find.textContaining('启动失败'), findsOneWidget, reason: '走普通错误 SnackBar');
    expect(fake.killPortCalls, 0);
  });
}
