import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mini_oc/stores.dart';

import 'helpers/fake_rust.dart';

/// 回归：登录后设备被自动绑定（Rust auto_bind 后台写 config.bound_device）时，
/// auth 内存态必须同步刷新，否则主页隧道开关仍置灰（「已绑定却提示未绑定」）。
void main() {
  test('登录后自动绑定（auto_bind 写 config）→ auth.boundDevice 同步刷新', () async {
    final fake = FakeRustBridge(cfg: testCfg());
    // 模拟：login 后 report_registration 返回 approved+enabled，触发 auto_bind
    fake.reportBound = true;
    fake.autoBindName = 'service-macbook';

    final container = ProviderContainer(overrides: fake.overrides());
    addTearDown(container.dispose);

    // 初始未绑定
    expect(container.read(authProvider).boundDevice, isNull);

    await container.read(authProvider.notifier)
        .login('https://oc.isoops.com', 'my-key');

    expect(container.read(authProvider).boundDevice, 'service-macbook',
        reason: '自动绑定后 auth 内存态必须刷新，否则主页隧道开关仍置灰');
  });
}