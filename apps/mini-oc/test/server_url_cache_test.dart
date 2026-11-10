import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mini_oc/stores.dart';

import 'helpers/fake_rust.dart';

/// 回归：saveServerUrl 保存新地址后，serverUrlEffectiveProvider 必须反映新地址，
/// 而非缓存旧地址（否则登录时用旧地址，Rust login 写回 config 覆盖新保存的地址）。
void main() {
  test('saveServerUrl 保存后 serverUrlEffectiveProvider 反映新地址（缓存失效）', () async {
    final fake = FakeRustBridge(cfg: testCfg(serverUrl: 'http://127.0.0.1:8100'));
    final container = ProviderContainer(overrides: fake.overrides());
    addTearDown(container.dispose);

    // 首次读取（设置页 initState 触发）→ 缓存旧地址
    expect(container.read(serverUrlEffectiveProvider), 'http://127.0.0.1:8100');

    // 修改并保存地址
    await container.read(authProvider.notifier).saveServerUrl('http://new.example.com');

    // 应返回新地址（修复前：返回缓存的旧地址 127.0.0.1:8100）
    expect(container.read(serverUrlEffectiveProvider), 'http://new.example.com');
  });
}