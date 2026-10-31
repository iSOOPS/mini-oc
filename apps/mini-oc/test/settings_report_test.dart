import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_riverpod/misc.dart' show Override;
import 'package:flutter_test/flutter_test.dart';
import 'package:mini_oc/src/rust/config.dart' show ClientConfig;

import 'package:mini_oc/pages/settings_page.dart';
import 'package:mini_oc/stores.dart';
import 'package:mini_oc/src/rust/api.dart' as rust;

/// 固定上报结果的 RegistrationNotifier。
class _FixedRegistration extends RegistrationNotifier {
  _FixedRegistration(this.outcome);
  final rust.ReportOutcome? outcome;
  int reportCalls = 0;

  @override
  rust.ReportOutcome? build() => outcome;

  @override
  Future<void> report() async {
    reportCalls++;
    // 对齐真实 report() 的状态写入语义（固定结果，不触真实桥接）
    state = outcome;
  }
}

Widget _app({rust.ReportOutcome? outcome}) {
  final overrides = <Override>[
    rustCfgProvider.overrideWith((ref) => const ClientConfig(
          serverUrl: 'https://oc.example.com',
          port: 9464,
          theme: 'system',
        )),
  ];
  if (outcome != null) {
    overrides.add(registrationProvider.overrideWith(() => _FixedRegistration(outcome)));
  }
  return ProviderScope(
    overrides: overrides,
    child: const MaterialApp(home: SettingsPage()),
  );
}

/// 用外部构造的 _FixedRegistration（可断言 reportCalls）。
Widget _appWithFixed(_FixedRegistration fixed) {
  return ProviderScope(
    overrides: [
      rustCfgProvider.overrideWith((ref) => const ClientConfig(
            serverUrl: 'https://oc.example.com',
            port: 9464,
            theme: 'system',
          )),
      registrationProvider.overrideWith(() => fixed),
    ],
    child: const MaterialApp(home: SettingsPage()),
  );
}

void main() {
  testWidgets('上报 approved+enabled+bound=true → 显示「已绑定」', (tester) async {
    await tester.pumpWidget(_app(
      outcome: const rust.ReportOutcome(
        ok: true, status: 'approved', deviceId: 'uuid-1', enabled: true, bound: true, error: null,
      ),
    ));
    await tester.pumpAndSettle();
    expect(find.textContaining('已绑定'), findsOneWidget,
        reason: '批准并完成绑定的设备应显示已绑定');
  });

  testWidgets('上报 approved+enabled+bound=false（自动绑定失败兜底）→ 显示「已上报，未绑定」且可重试', (tester) async {
    await tester.pumpWidget(_app(
      outcome: const rust.ReportOutcome(
        ok: true, status: 'approved', deviceId: 'uuid-1', enabled: true, bound: false, error: null,
      ),
    ));
    await tester.pumpAndSettle();
    expect(find.textContaining('未绑定'), findsOneWidget,
        reason: '批准但绑定未完成时提示用户可重新上报');
  });

  testWidgets('上报 pending → 显示「待审核」且按钮可点', (tester) async {
    final fixed = _FixedRegistration(const rust.ReportOutcome(
      ok: true, status: 'pending', deviceId: 'uuid-1', enabled: false, bound: false, error: null,
    ));
    await tester.pumpWidget(_appWithFixed(fixed));
    await tester.pumpAndSettle();
    expect(find.textContaining('待审核'), findsOneWidget);
    final btn = tester.widget<FilledButton>(
        find.ancestor(of: find.text('上报'), matching: find.byType(FilledButton)));
    expect(btn.onPressed, isNotNull, reason: 'pending 态按钮可点（重报是唯一感知批准的途径）');
  });

  testWidgets('pending 态进设置页 → 自动静默重报一次（感知批准）', (tester) async {
    final fixed = _FixedRegistration(const rust.ReportOutcome(
      ok: true, status: 'pending', deviceId: 'uuid-1', enabled: false, bound: false, error: null,
    ));
    await tester.pumpWidget(_appWithFixed(fixed));
    await tester.pumpAndSettle();
    expect(fixed.reportCalls, 1, reason: '进页面时 pending 自动静默重报一次');
  });

  testWidgets('非 pending 态进设置页 → 不自动重报', (tester) async {
    final fixed = _FixedRegistration(const rust.ReportOutcome(
      ok: true, status: 'approved', deviceId: 'uuid-1', enabled: true, bound: true, error: null,
    ));
    await tester.pumpWidget(_appWithFixed(fixed));
    await tester.pumpAndSettle();
    expect(fixed.reportCalls, 0, reason: '已绑定无需重报');
  });

  testWidgets('未上报 → 引导文案', (tester) async {
    await tester.pumpWidget(_app());
    await tester.pumpAndSettle();
    expect(find.textContaining('尚未上报'), findsOneWidget);
  });

  testWidgets('上报失败 → 红色错误文案', (tester) async {
    await tester.pumpWidget(_app(
      outcome: const rust.ReportOutcome(
        ok: false, status: '', deviceId: '', enabled: false, bound: false, error: '网络错误',
      ),
    ));
    await tester.pumpAndSettle();
    expect(find.textContaining('上报失败'), findsOneWidget);
  });
}
