import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:window_manager/window_manager.dart';

import 'pages/home_page.dart';
import 'pages/login_page.dart';
import 'pages/login_server_page.dart';
import 'pages/logs_page.dart';
import 'pages/sessions_page.dart';
import 'pages/settings_page.dart';
import 'shell.dart';
import 'src/rust/api.dart' as rust;
import 'src/rust/frb_generated.dart';
import 'stores.dart';
import 'theme.dart';

/// 全局导航 key：用于在关闭流程中从任意位置弹「正在关闭」对话框
/// （`MiniOcApp` 的 context 在 Navigator 之外，直接 showDialog 会找不到 Navigator）。
final rootNavigatorKey = GlobalKey<NavigatorState>();

/// 路由：三级守卫（未登录 redirect /login；已登录主页）。
final routerProvider = Provider<GoRouter>((ref) {
  final auth = ValueNotifier(ref.read(authProvider).isLoggedIn);
  ref.listen(authProvider, (_, next) => auth.value = next.isLoggedIn);

  return GoRouter(
    navigatorKey: rootNavigatorKey,
    initialLocation: '/home',
    refreshListenable: auth,
    redirect: (context, state) {
      final loggedIn = auth.value;
      // 登录流程相关页面（/login 及子页如 /login/server）未登录也可访问
      final inLoginFlow = state.matchedLocation.startsWith('/login');
      if (!loggedIn) return inLoginFlow ? null : '/login';
      if (inLoginFlow) return '/home';
      return null;
    },
    routes: [
      GoRoute(path: '/login', builder: (_, __) => const LoginPage()),
      GoRoute(path: '/login/server', builder: (_, __) => const LoginServerPage()),
      ShellRoute(
        builder: (_, __, child) => AppShell(child: child),
        routes: [
          GoRoute(path: '/home', builder: (_, __) => const HomePage()),
          GoRoute(path: '/sessions', builder: (_, __) => const SessionsPage()),
          GoRoute(path: '/logs', builder: (_, __) => const LogsPage()),
          GoRoute(path: '/settings', builder: (_, __) => const SettingsPage()),
        ],
      ),
    ],
  );
});

void main() async {
  WidgetsFlutterBinding.ensureInitialized();
  // I3 修复：接管关窗事件——点 X 也走优雅停机（防孤儿进程占端口）
  await windowManager.ensureInitialized();
  await windowManager.setPreventClose(true);
  // 关键：Rust 桥接必须在任何 provider 读到 rust 之前初始化
  // （authProvider.build 同步调 rust.loadConfig，若未初始化则首帧白屏）
  await RustLib.init();
  runApp(const ProviderScope(child: MiniOcApp()));
}

class MiniOcApp extends ConsumerStatefulWidget {
  const MiniOcApp({super.key});

  @override
  ConsumerState<MiniOcApp> createState() => _MiniOcAppState();
}

class _MiniOcAppState extends ConsumerState<MiniOcApp> with WindowListener {
  /// 关闭流程是否已启动（防重复触发点 X 多次）。
  bool _closing = false;

  @override
  void initState() {
    super.initState();
    windowManager.addListener(this);
  }

  @override
  void dispose() {
    windowManager.removeListener(this);
    super.dispose();
  }

  /// 任意关闭路径（点 X / ⌘Q / 系统）→ 弹「正在关闭」等待框 + 优雅停机后真正退出。
  @override
  void onWindowClose() async {
    if (_closing) return; // 已在关闭中，忽略重复关闭请求
    _closing = true;

    // 弹出关闭等待框（模态、不可点外关闭），虚拟展示停机进度。
    // 用 rootNavigatorKey 的 context 弹框（MiniOcApp 自身在 Navigator 之外，
    // 直接 context 会报「context does not include a Navigator」）。
    final navContext = rootNavigatorKey.currentContext;
    if (navContext != null) {
      unawaited(showDialog<void>(
        context: navContext,
        barrierDismissible: false,
        useRootNavigator: true,
        builder: (_) => const ShutdownDialog(),
      ));
    }

    // 优雅停机（Rust 侧已限时，最长约 4.5s；再套 6s 兜底）。
    // 无论成功/失败/超时都继续；最后再保证等待框至少可见一小段时间。
    try {
      await rust.stopAll().timeout(const Duration(seconds: 6));
    } catch (_) {
      // 超时/失败也放行退出（终止信号已尽力发出）
    }
    // 最短展示时长：确保「正在关闭」反馈可见，而非一闪而过。
    await Future.delayed(const Duration(milliseconds: 900));

    // 真正销毁窗口退出。
    await windowManager.destroy();
  }

  @override
  Widget build(BuildContext context) {
    final router = ref.watch(routerProvider);
    return MaterialApp.router(
      title: 'mini-oc',
      themeMode: ThemeMode.system,
      theme: AppTheme.light(),
      darkTheme: AppTheme.dark(),
      routerConfig: router,
      debugShowCheckedModeBanner: false,
    );
  }
}

/// 关闭等待框：进度条 + 虚拟的停机步骤文案。
///
/// 实际停机由 Rust `stopAll` 在后台执行（有 2s/进程上限），本框用定时器
/// 平滑推进进度并轮播「正在关闭」的虚拟步骤，给用户明确的退出反馈。
class ShutdownDialog extends StatefulWidget {
  const ShutdownDialog({super.key});

  @override
  State<ShutdownDialog> createState() => _ShutdownDialogState();
}

class _ShutdownDialogState extends State<ShutdownDialog> {
  /// 虚拟停机步骤（依次轮播；进度条随步骤推进）。
  static const _steps = [
    '正在停止本地服务（opencode）…',
    '正在停止隧道（rathole）…',
    '正在清理残留进程…',
    '正在保存配置…',
    '正在退出…',
  ];

  /// 每个步骤的展示时长（虚拟，非真实耗时）。
  static const _stepDuration = Duration(milliseconds: 700);

  int _step = 0;
  double _progress = 0;
  Timer? _timer;

  @override
  void initState() {
    super.initState();
    // 进度条：每 80ms 前进一小格，覆盖约 5 个步骤的时长。
    _timer = Timer.periodic(const Duration(milliseconds: 80), (_) {
      if (!mounted) return;
      setState(() {
        _progress += 0.025;
        if (_progress > 1.0) _progress = 1.0;
        // 按进度映射当前步骤文案
        final totalMs = _stepDuration.inMilliseconds * _steps.length;
        final elapsed = (_progress * totalMs).round();
        final idx = (elapsed ~/ _stepDuration.inMilliseconds)
            .clamp(0, _steps.length - 1);
        _step = idx;
      });
    });
  }

  @override
  void dispose() {
    _timer?.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return PopScope(
      canPop: false, // 关闭期间禁止任何方式关闭本框
      child: AlertDialog(
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(20)),
        title: Row(
          children: [
            SizedBox(
              width: 22,
              height: 22,
              child: CircularProgressIndicator(
                strokeWidth: 2.5,
                value: _progress,
                color: scheme.primary,
              ),
            ),
            const SizedBox(width: 12),
            const Text('正在关闭', style: TextStyle(fontSize: 18)),
          ],
        ),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            // 当前步骤文案（虚拟）
            AnimatedSwitcher(
              duration: const Duration(milliseconds: 250),
              child: Text(
                _steps[_step],
                key: ValueKey(_step),
                style: TextStyle(fontSize: 14, color: scheme.onSurfaceVariant),
              ),
            ),
            const SizedBox(height: 16),
            // 确定型进度条
            ClipRRect(
              borderRadius: BorderRadius.circular(8),
              child: LinearProgressIndicator(
                value: _progress,
                minHeight: 8,
                backgroundColor: scheme.surfaceContainerHighest,
                color: scheme.primary,
              ),
            ),
            const SizedBox(height: 8),
            // 已完成的步骤（虚拟清单）
            for (var i = 0; i < _steps.length; i++)
              Padding(
                padding: const EdgeInsets.symmetric(vertical: 2),
                child: Row(
                  children: [
                    Icon(
                      i < _step
                          ? Icons.check_circle
                          : (i == _step
                              ? Icons.timelapse
                              : Icons.radio_button_unchecked),
                      size: 14,
                      color: i < _step
                          ? scheme.primary
                          : (i == _step
                              ? scheme.primary
                              : scheme.outline),
                    ),
                    const SizedBox(width: 8),
                    Text(
                      _steps[i],
                      style: TextStyle(
                        fontSize: 12,
                        color: i <= _step
                            ? scheme.onSurface
                            : scheme.outline,
                      ),
                    ),
                  ],
                ),
              ),
          ],
        ),
      ),
    );
  }
}
