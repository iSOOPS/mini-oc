import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:mini_oc/stores.dart';
import 'package:mini_oc/src/rust/account.dart' show DeviceBrief;
import 'package:mini_oc/src/rust/api/appenv.dart' show AppEnv;
import 'package:mini_oc/src/rust/config.dart' show ClientConfig;

import 'helpers/fake_rust.dart';

void main() {
  test('login 后 config 已有绑定且清单仍含该设备 → AuthState.boundDevice 恢复（I16）',
      () async {
    final fake = FakeRustBridge(
      appEnv: const AppEnv(
          serverUrl: 'https://x', rememberPassword: false, password: ''),
      deviceBriefs: [
        DeviceBrief(
          name: 'macbook-pro',
          port: 9464,
          pctype: 'macos',
          bound: true,
          deviceName: 'host',
          publicUrl: 'https://oc.example.com',
          deviceId: '',
          enabled: true,
        ),
      ],
    );
    // 模拟 Rust login 保留 bound_device 的新行为（I16 修复后：清单仍含则不清空）
    fake.cfg = const ClientConfig(
      serverUrl: 'https://x',
      boundDevice: 'macbook-pro',
      port: 9464,
      theme: 'system',
    );

    final container = ProviderContainer(overrides: fake.overrides());
    addTearDown(container.dispose);

    final notifier = container.read(authProvider.notifier);
    await notifier.login('https://x', 'my-key', remember: false);

    final state = container.read(authProvider);
    expect(state.isLoggedIn, isTrue);
    expect(state.boundDevice, 'macbook-pro',
        reason: '登录后绑定态从 config 恢复——云服务开关不再永久置灰（I16）');
  });
}
