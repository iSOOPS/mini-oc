import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_riverpod/misc.dart' show Override;
import 'package:flutter_test/flutter_test.dart';
import 'package:mini_oc/pages/settings_page.dart';
import 'package:mini_oc/stores.dart';
import 'package:mini_oc/src/rust/config.dart' show ClientConfig;

/// 可变配置源：模拟「Rust 侧 config.json 更新」。
class MutableCfg {
  MutableCfg(ClientConfig cfg) : _cfg = cfg;

  ClientConfig _cfg;
  final List<ClientConfig> saved = [];

  List<Override> overrides() => [
        rustCfgProvider.overrideWith((ref) => _cfg),
        saveConfigFnProvider.overrideWithValue(({required ClientConfig cfg}) async {
          _cfg = cfg; // 保存即写盘（模拟）
          saved.add(cfg);
        }),
      ];
}

ClientConfig cfgWithPort(int port) => ClientConfig(
      serverUrl: 'https://oc.example.com',
      port: port,
      theme: 'system',
    );

Finder portField() => find.byType(TextField).at(1);

TextField portFieldOf(WidgetTester tester) => tester.widget<TextField>(portField());

void main() {
  // 单一 ProviderContainer：与生产一致（App 全局一个，切页不换 container）。
  // 生产语义关键点：rustCfgProvider 是非 autoDispose 的 Provider——首次求值后
  // 缓存，后续 watch 不再重算。SettingsPage 若在 initState 读缓存旧值 →
  // 保存成功后切页回来仍显示旧端口（bug 场景）。
  testWidgets('修改端口保存后，切页回来显示新端口（生产缓存场景）', (tester) async {
    final bridge = MutableCfg(cfgWithPort(9464));
    final container = ProviderContainer(overrides: bridge.overrides());
    addTearDown(container.dispose);

    Future<void> pumpSettings() async {
      await tester.pumpWidget(
        UncontrolledProviderScope(
          container: container,
          child: const MaterialApp(home: Scaffold(body: SettingsPage())),
        ),
      );
      await tester.pumpAndSettle();
    }

    await pumpSettings();

    await tester.enterText(portField(), '9466');
    await tester.pump();
    await tester.ensureVisible(find.text('保存设置'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('保存设置'));
    await tester.pumpAndSettle();

    expect(bridge.saved.single.port, 9466, reason: '保存的配置端口为 9466');

    // 切页（卸载）→ 回来（重挂），container 不变（生产：ShellRoute 切页）
    await tester.pumpWidget(const SizedBox());
    await pumpSettings();

    expect(portFieldOf(tester).controller?.text, '9466',
        reason: '切页回来应显示最新保存的端口');
  });

  testWidgets('未保存切页回来，端口保持原值', (tester) async {
    final bridge = MutableCfg(cfgWithPort(9464));
    final container = ProviderContainer(overrides: bridge.overrides());
    addTearDown(container.dispose);

    Future<void> pumpSettings() async {
      await tester.pumpWidget(
        UncontrolledProviderScope(
          container: container,
          child: const MaterialApp(home: Scaffold(body: SettingsPage())),
        ),
      );
      await tester.pumpAndSettle();
    }

    await pumpSettings();

    await tester.enterText(portField(), '9466');
    await tester.pump();
    // 不保存，切页回来
    await tester.pumpWidget(const SizedBox());
    await pumpSettings();

    expect(portFieldOf(tester).controller?.text, '9464',
        reason: '未保存的修改不落盘，切页回来显示原值');
  });
}
