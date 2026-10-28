import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:mini_oc/pages/settings_page.dart';
import 'package:mini_oc/src/rust/api.dart' show ReportOutcome;
import 'package:mini_oc/src/rust/config.dart' show ClientConfig;
import 'package:mini_oc/stores.dart';


/// 可注入上报结果的 RegistrationNotifier：记录 report() 调用。
class _ScriptedRegistration extends RegistrationNotifier {
  _ScriptedRegistration({this.outcome});
  ReportOutcome? outcome;
  int calls = 0;

  @override
  ReportOutcome? build() => outcome;

  @override
  Future<void> report() async {
    calls++;
    state = outcome;
    // 对齐真实 RegistrationNotifier：上报成功后刷新 config 缓存
    if (outcome?.ok == true) {
      ref.invalidate(rustCfgProvider);
    }
  }
}

/// 绕过 Rust 配置读取的 AuthNotifier。
class _FixedAuth extends AuthNotifier {
  _FixedAuth(this.initial);
  final AuthState initial;
  @override
  AuthState build() => initial;
}

ReportOutcome _ok(String status, bool enabled) => ReportOutcome(
      ok: true,
      status: status,
      deviceId: 'd-123',
      enabled: enabled,
      error: null,
    );

FilledButton reportBtn(WidgetTester tester) {
  final btn = find.ancestor(of: find.text('上报'), matching: find.byType(FilledButton));
  return tester.widget<FilledButton>(btn);
}

/// 设置页测试容器：mock 上报 + 固定 config（含 deviceId，验证保存透传）。
Widget wrap({ReportOutcome? initial, required ClientConfig cfg}) {
  return ProviderScope(
    overrides: [
      registrationProvider.overrideWith(() => _ScriptedRegistration(outcome: initial)),
      authProvider.overrideWith(() => _FixedAuth(const AuthState())),
      rustCfgProvider.overrideWithValue(cfg),
      saveConfigFnProvider.overrideWithValue(({required cfg}) async {}),
    ],
    child: const MaterialApp(home: SettingsPage()),
  );
}

void main() {
  const cfgWithDevice = ClientConfig(
    serverUrl: 'https://oc.example.com',
    port: 9464,
    theme: 'system',
    deviceId: 'd-123',
  );

  testWidgets('「设备上报」区存在，按钮初始可点（provider 初始 null）', (tester) async {
    await tester.pumpWidget(wrap(initial: null, cfg: cfgWithDevice));
    await tester.pumpAndSettle();

    expect(find.text('设备上报'), findsOneWidget, reason: '「设备上报」区存在');
    expect(find.text('上报'), findsOneWidget, reason: '「上报」按钮存在');
    expect(reportBtn(tester).onPressed, isNotNull, reason: '初始未上报，按钮可点');
  });

  testWidgets('approved&&enabled → 按钮禁用，显示「已上报」', (tester) async {
    await tester.pumpWidget(wrap(initial: _ok('approved', true), cfg: cfgWithDevice));
    await tester.pumpAndSettle();

    expect(reportBtn(tester).onPressed, isNull, reason: '已上报且启用，按钮禁用');
    expect(find.text('已上报'), findsOneWidget);
  });

  testWidgets('pending → 按钮禁用，显示「待审核」', (tester) async {
    await tester.pumpWidget(wrap(initial: _ok('pending', false), cfg: cfgWithDevice));
    await tester.pumpAndSettle();

    expect(reportBtn(tester).onPressed, isNull, reason: '待审核，按钮禁用');
    expect(find.text('待审核'), findsOneWidget);
  });

  testWidgets('approved&&!enabled → 显示「已停用」，按钮保持可点', (tester) async {
    await tester.pumpWidget(wrap(initial: _ok('approved', false), cfg: cfgWithDevice));
    await tester.pumpAndSettle();

    expect(find.text('已停用'), findsOneWidget);
    expect(reportBtn(tester).onPressed, isNotNull, reason: '已停用时按钮可点（重新上报）');
  });

  testWidgets('上报失败（ok=false）→ 显示错误文本，按钮可点', (tester) async {
    const failed = ReportOutcome(
      ok: false, status: '', deviceId: '', enabled: false, error: '网络错误');
    await tester.pumpWidget(wrap(initial: failed, cfg: cfgWithDevice));
    await tester.pumpAndSettle();

    expect(find.textContaining('网络错误'), findsOneWidget, reason: '错误文本可见');
    expect(reportBtn(tester).onPressed, isNotNull, reason: '失败后可重试');
  });

  testWidgets('save_preserves_device_id：保存构造透传 deviceId', (tester) async {
    await tester.pumpWidget(wrap(initial: null, cfg: cfgWithDevice));
    await tester.pumpAndSettle();

    // 模拟保存：页面用同一构造函数（buildSavedConfig）生成新 config
    final saved = buildSavedConfig(
      serverText: 'https://oc.example.com',
      portText: '9464',
      theme: 'dark',
      current: cfgWithDevice,
    );
    expect(saved.deviceId, 'd-123', reason: '保存后 deviceId 不丢');
    expect(saved.serverUrl, 'https://oc.example.com');
    expect(saved.theme, 'dark');
  });

  testWidgets('回归：上报后 config 已变（缓存未刷）→ 保存透传新 deviceId', (tester) async {
    // 模拟场景：进设置页时 provider 缓存 deviceId=null 的旧快照 → 点「上报」
    // （Rust 侧把新 deviceId="d-xyz" 写回 config.json）→ 改端口点「保存设置」。
    // 保存必须读到重算后的新 config（deviceId="d-xyz"），而非旧快照。
    final snapshots = <ClientConfig>[
      const ClientConfig(serverUrl: 'https://oc.example.com', port: 9464, theme: 'system'),
      const ClientConfig(
          serverUrl: 'https://oc.example.com', port: 9464, theme: 'system', deviceId: 'd-xyz'),
    ];
    var readCount = 0;
    var saveCount = 0;
    final savedCfgs = <ClientConfig>[];

    final reported = _ok('approved', true);
    final container = ProviderContainer(overrides: [
      // 可变 provider：首次读返回旧快照（deviceId=null），invalidate 后重算返回新值
      rustCfgProvider.overrideWith((ref) {
        final snap = snapshots[readCount.clamp(0, snapshots.length - 1)];
        readCount++;
        return snap;
      }),
      registrationProvider.overrideWith(() => _ScriptedRegistration(outcome: reported)),
      authProvider.overrideWith(() => _FixedAuth(const AuthState())),
      saveConfigFnProvider.overrideWithValue(({required cfg}) async {
        saveCount++;
        savedCfgs.add(cfg);
      }),
    ]);
    addTearDown(container.dispose);

    await tester.pumpWidget(UncontrolledProviderScope(
      container: container,
      child: const MaterialApp(home: SettingsPage()),
    ));
    await tester.pumpAndSettle();

    // 1) 点「上报」：mock fn 成功（state 置 approved）→ notifier invalidate 配置缓存
    await tester.tap(find.text('上报'));
    await tester.pumpAndSettle();
    expect(saveCount, 0, reason: '上报本身不触发保存');

    // 2) 改端口触发 _dirty，滚动到视口外的「保存设置」按钮并点击
    await tester.enterText(find.byType(TextField).at(1), '9999');
    await tester.pumpAndSettle();
    await tester.dragUntilVisible(
        find.text('保存设置'), find.byType(ListView), const Offset(0, -120));
    await tester.pumpAndSettle();
    await tester.tap(find.text('保存设置'));
    await tester.pumpAndSettle();

    // 3) 断言：保存走 saveConfigFnProvider 且透传的是新 deviceId
    expect(saveCount, 1, reason: '保存调用注入的 saveConfig fn');
    expect(savedCfgs.single.deviceId, 'd-xyz',
        reason: '保存必须基于重算后的最新 config（新 deviceId），而非上报前的旧快照');
    expect(savedCfgs.single.port, 9999, reason: '表单改动生效');
  });

  testWidgets('设置页不再暴露 opencode 二进制路径输入框，端口文案更新', (tester) async {
    await tester.pumpWidget(wrap(initial: null, cfg: cfgWithDevice));
    await tester.pumpAndSettle();

    expect(find.text('opencode 二进制路径（留空自动探测）'), findsNothing,
        reason: '二进制路径输入框已从 UI 移除');
    expect(find.text('opencode 端口（上报给服务端）'), findsOneWidget,
        reason: '端口文案更新');
  });

  test('provider 层：report() 走注入 fn 并置状态', () async {
    final calls = <int>[];
    final container = ProviderContainer(overrides: [
      reportRegistrationFnProvider.overrideWithValue(() async {
        calls.add(1);
        return _ok('approved', true);
      }),
    ]);
    addTearDown(container.dispose);

    final notifier = container.read(registrationProvider.notifier);
    expect(container.read(registrationProvider), isNull, reason: '初始 null');

    await notifier.report();
    expect(calls, [1], reason: 'report 调用注入 fn');
    expect(container.read(registrationProvider)?.status, 'approved');
  });

  test('provider 层：fn 抛异常时兜底 ok=false 不崩溃', () async {
    final container = ProviderContainer(overrides: [
      reportRegistrationFnProvider
          .overrideWithValue(() => Future.error('桥接崩溃')),
    ]);
    addTearDown(container.dispose);

    await container.read(registrationProvider.notifier).report();
    final out = container.read(registrationProvider);
    expect(out?.ok, isFalse, reason: '异常装进 ok=false');
    expect(out?.error, isNotNull);
  });
}
