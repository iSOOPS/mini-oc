import 'dart:async';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:mini_oc/pages/logs_page.dart';
import 'package:mini_oc/src/rust/logbuf.dart' show LogLine;

LogLine _l(String source, String msg) => LogLine(
      tsMs: 1791900000000,
      source: source,
      level: 'info',
      message: msg,
    );

/// 可注入日志流 + 记录 Rust 清空调用的测试 Notifier。
class _TestLogsNotifier extends LogsNotifier {
  _TestLogsNotifier(this.stream);
  final Stream<LogLine> stream;
  int clearCalls = 0;

  @override
  Stream<LogLine> logStream() => stream;

  @override
  Future<void> clearRustLogs() async {
    clearCalls++;
  }
}

Widget _wrap(Stream<LogLine> stream, _TestLogsNotifier notifier) {
  return ProviderScope(
    overrides: [logsProvider.overrideWith(() => notifier)],
    child: const MaterialApp(home: LogsPage()),
  );
}

void main() {
  testWidgets('来源过滤后列表仅含选中来源', (tester) async {
    final ctrl = StreamController<LogLine>();
    final notifier = _TestLogsNotifier(ctrl.stream);
    await tester.pumpWidget(_wrap(ctrl.stream, notifier));
    ctrl.add(_l('opencode', 'serve started'));
    ctrl.add(_l('rathole', 'connected'));
    ctrl.add(_l('system', 'app init'));
    await tester.pumpAndSettle();

    expect(find.textContaining('serve started'), findsOneWidget);
    expect(find.textContaining('connected'), findsOneWidget);
    expect(find.textContaining('app init'), findsOneWidget);

    // 过滤 rathole
    await tester.tap(find.text('rathole'));
    await tester.pumpAndSettle();
    expect(find.textContaining('connected'), findsOneWidget);
    expect(find.textContaining('serve started'), findsNothing);
    expect(find.textContaining('app init'), findsNothing);
    await ctrl.close();
  });

  testWidgets('全屏切换后 AppBar 隐藏', (tester) async {
    final ctrl = StreamController<LogLine>();
    final notifier = _TestLogsNotifier(ctrl.stream);
    await tester.pumpWidget(_wrap(ctrl.stream, notifier));
    ctrl.add(_l('system', 'hello'));
    await tester.pumpAndSettle();

    expect(find.byType(AppBar), findsOneWidget);
    await tester.tap(find.byTooltip('全屏'));
    await tester.pumpAndSettle();
    expect(find.byType(AppBar), findsNothing, reason: '全屏无 AppBar');
    await tester.tap(find.byTooltip('退出全屏'));
    await tester.pumpAndSettle();
    expect(find.byType(AppBar), findsOneWidget);
    await ctrl.close();
  });

  testWidgets('复制全部调用剪贴板且内容完整', (tester) async {
    final copied = <String>[];
    final original = ClipboardProxy.setter;
    ClipboardProxy.setter = (t) async => copied.add(t);
    addTearDown(() => ClipboardProxy.setter = original);

    final ctrl = StreamController<LogLine>();
    final notifier = _TestLogsNotifier(ctrl.stream);
    await tester.pumpWidget(_wrap(ctrl.stream, notifier));
    ctrl.add(_l('opencode', 'a'));
    ctrl.add(_l('system', 'b'));
    await tester.pumpAndSettle();

    await tester.tap(find.byTooltip('复制全部'));
    await tester.pumpAndSettle();
    expect(copied.length, 1);
    expect(copied.first, contains('[opencode] a'));
    expect(copied.first, contains('[system] b'));
    expect(find.textContaining('已复制'), findsOneWidget);
    await ctrl.close();
  });

  testWidgets('点击清空 → 列表清空且 Rust 清空被调用', (tester) async {
    final ctrl = StreamController<LogLine>();
    final notifier = _TestLogsNotifier(ctrl.stream);
    await tester.pumpWidget(_wrap(ctrl.stream, notifier));
    ctrl.add(_l('opencode', 'hello'));
    await tester.pumpAndSettle();
    expect(find.textContaining('hello'), findsOneWidget);

    await tester.tap(find.byTooltip('清空'));
    await tester.pumpAndSettle();

    expect(find.textContaining('hello'), findsNothing, reason: '点击清空后日志消失');
    expect(find.text('暂无日志'), findsOneWidget, reason: '清空后显示空态');
    expect(notifier.clearCalls, 1, reason: '清空需同步清 Rust 环形缓冲');
    await ctrl.close();
  });
}