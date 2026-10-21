import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../stores.dart';

/// 仪表盘（Task 8 填充服务开关）。
class HomePage extends ConsumerWidget {
  const HomePage({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return const Center(child: Text('概览（Task 8 填充）'));
  }
}
