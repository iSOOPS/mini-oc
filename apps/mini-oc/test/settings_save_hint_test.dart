import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_riverpod/misc.dart' show Override;
import 'package:flutter_test/flutter_test.dart';
import 'package:mini_oc/pages/settings_page.dart';
import 'package:mini_oc/stores.dart';
import 'package:mini_oc/src/rust/config.dart' show ClientConfig;

/// 可变配置源（模拟 Rust config.json 状态）。
class MutableCfg {
  MutableCfg(ClientConfig cfg) : _cfg = cfg;
  ClientConfig _cfg;
  List<ClientConfig> saved = [];

  List<Override> overrides() => [
        rustCfgProvider.overrideWith((ref) => _cfg),
        saveConfigFnProvider.overrideWithValue(({required ClientConfig cfg}) async {
          _cfg = cfg;
          saved.add(cfg);
        }),
      ];
}

Finder portField() => find.byType(TextField).at(1);
Finder serverField() => find.byType(TextField).at(0);

Future<void> pumpSettings(WidgetTester tester, MutableCfg bridge) async {
  await tester.pumpWidget(
    ProviderScope(
      overrides: bridge.overrides(),
      child: const MaterialApp(home: Scaffold(body: SettingsPage())),
    ),
  );
  await tester.pumpAndSettle();
}

Future<void> tapSave(WidgetTester tester) async {
  await tester.ensureVisible(find.text('保存设置'));
  await tester.pumpAndSettle();
  await tester.tap(find.text('保存设置'));
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('只改端口保存 → 提示不含重启字样（I19）', (tester) async {
    final bridge = MutableCfg(const ClientConfig(
      serverUrl: 'https://oc.example.com',
      port: 9464,
      theme: 'system',
    ));
    await pumpSettings(tester, bridge);

    await tester.enterText(portField(), '9466');
    await tester.pump();
    await tapSave(tester);

    expect(bridge.saved.single.port, 9466);
    expect(find.textContaining('已保存'), findsOneWidget, reason: '保存成功提示');
    expect(find.textContaining('重新登录'), findsNothing,
        reason: '仅改端口不需要重启/重登提示');
  });

  testWidgets('改服务器地址保存 → 提示需重新登录（I19）', (tester) async {
    final bridge = MutableCfg(const ClientConfig(
      serverUrl: 'https://old.example.com',
      port: 9464,
      theme: 'system',
    ));
    await pumpSettings(tester, bridge);

    await tester.enterText(serverField(), 'https://new.example.com');
    await tester.pump();
    await tapSave(tester);

    expect(bridge.saved.single.serverUrl, 'https://new.example.com');
    expect(find.textContaining('重新登录'), findsOneWidget,
        reason: '服务器地址变更提示需重新登录');
  });

  testWidgets('设置页无外观区块（I19：外观功能已移除）', (tester) async {
    final bridge = MutableCfg(const ClientConfig(
      serverUrl: 'https://oc.example.com',
      port: 9464,
      theme: 'system',
    ));
    await pumpSettings(tester, bridge);

    expect(find.text('外观'), findsNothing, reason: '外观 section 移除');
    expect(find.text('跟随系统'), findsNothing);
    expect(find.text('亮色'), findsNothing);
    expect(find.text('暗色'), findsNothing);
  });
}
