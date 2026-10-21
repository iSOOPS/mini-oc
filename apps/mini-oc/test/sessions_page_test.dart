import 'dart:async';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:mini_oc/pages/sessions_page.dart';
import 'package:mini_oc/stores.dart';
import 'package:mini_oc/src/rust/api.dart' show ProjectSummary, SessionSummary;
import 'package:mini_oc/src/rust/supervisor.dart' show ServiceMode, ServiceState;

ProjectSummary _proj(String name, String path, {String? vcs}) =>
    ProjectSummary(id: path, name: name, path: path, vcs: vcs);

SessionSummary _ses(String id, String title, String dir) => SessionSummary(
      id: id,
      title: title,
      directory: dir,
      updatedMs: DateTime.now().millisecondsSinceEpoch - 60_000,
    );

Widget wrap({
  required Stream<ServiceState> states,
  required List<ProjectSummary> projects,
  OpenTerminalFn? open,
}) {
  return ProviderScope(
    overrides: [
      serviceStateProvider.overrideWith((ref) => states),
      listProjectsProvider.overrideWith((ref) => Future.value(projects)),
      listSessionsProvider.overrideWith((ref, dir) => Future.value([
            _ses('ses_1', '重构讨论', dir),
            _ses('ses_2', 'bug 修复', dir),
          ])),
      if (open != null) openInTerminalProvider.overrideWithValue(open),
    ],
    child: const MaterialApp(home: SessionsPage()),
  );
}

void main() {
  testWidgets('serve 未运行时显示启动引导', (tester) async {
    await tester.pumpWidget(wrap(
      states: Stream.value(const ServiceState(mode: ServiceMode.off, port: 0)),
      projects: [_proj('proj-a', '/x/proj-a')],
    ));
    await tester.pumpAndSettle();
    expect(find.text('服务未启动'), findsOneWidget);
    expect(find.text('proj-a'), findsNothing);
  });

  testWidgets('项目列表渲染并支持检索过滤', (tester) async {
    await tester.pumpWidget(wrap(
      states: Stream.value(const ServiceState(mode: ServiceMode.localOnly, port: 9464)),
      projects: [
        _proj('proj-a', '/Users/x/proj-a', vcs: 'git'),
        _proj('web', '/Users/x/website'),
      ],
    ));
    await tester.pumpAndSettle();
    expect(find.text('proj-a'), findsOneWidget);
    expect(find.text('web'), findsOneWidget);

    // 检索 "proj" 只命中 proj-a
    await tester.enterText(find.byType(TextField), 'proj');
    await tester.pumpAndSettle();
    expect(find.text('proj-a'), findsOneWidget);
    expect(find.text('web'), findsNothing);
  });

  testWidgets('展开项目显示会话，点击会话调用终端打开（参数正确）', (tester) async {
    final calls = <(String, String?)>[];
    await tester.pumpWidget(wrap(
      states: Stream.value(const ServiceState(mode: ServiceMode.localOnly, port: 9464)),
      projects: [_proj('proj-a', '/Users/x/proj-a')],
      open: (dir, sid) async => calls.add((dir, sid)),
    ));
    await tester.pumpAndSettle();

    // 展开项目
    await tester.tap(find.text('proj-a'));
    await tester.pumpAndSettle();
    expect(find.text('重构讨论'), findsOneWidget);
    expect(find.text('bug 修复'), findsOneWidget);

    // 点击会话
    await tester.tap(find.text('重构讨论'));
    await tester.pumpAndSettle();
    expect(calls, [('/Users/x/proj-a', 'ses_1')]);
    expect(find.text('已在系统终端打开 ✓'), findsOneWidget);
  });
}
