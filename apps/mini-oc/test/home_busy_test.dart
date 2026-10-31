import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:mini_oc/pages/home_page.dart';

void main() {
  group('ServiceActionBusyNotifier（全局启停互斥）', () {
    test('busy 期间重入被拒绝；结束后可再次运行', () async {
      final container = ProviderContainer();
      addTearDown(container.dispose);

      final notifier = container.read(serviceActionBusyProvider.notifier);
      expect(container.read(serviceActionBusyProvider), isFalse,
          reason: '初始不忙');

      // 第一次：挂起直到我们放行
      var case1Done = false;
      final release = Completer<void>();
      final first = notifier.run(() async {
        await release.future;
        case1Done = true;
        return 1;
      });

      // 微任务推进后 busy 应为 true
      await Future<void>.delayed(Duration.zero);
      expect(container.read(serviceActionBusyProvider), isTrue,
          reason: '动作进行中 busy=true');

      // 重入：立即抛错，不执行动作体
      var reentered = false;
      await expectLater(
        notifier.run(() async {
          reentered = true;
          return 2;
        }),
        throwsA('操作进行中'),
      );
      expect(reentered, isFalse, reason: '重入动作体未执行');

      // 放行第一次 → 完成后 busy 复位
      release.complete();
      expect(await first, 1);
      expect(case1Done, isTrue);
      expect(container.read(serviceActionBusyProvider), isFalse,
          reason: '动作结束后 busy 复位');

      // 复位后可再次运行（切页重置问题不再复现——provider 全局存活）
      expect(await notifier.run(() async => 3), 3);
    });

    test('动作抛错也复位 busy', () async {
      final container = ProviderContainer();
      addTearDown(container.dispose);

      final notifier = container.read(serviceActionBusyProvider.notifier);
      await expectLater(
        notifier.run(() async => throw Exception('boom')),
        throwsException,
      );
      expect(container.read(serviceActionBusyProvider), isFalse,
          reason: '失败路径也必须复位 busy');
    });
  });
}
