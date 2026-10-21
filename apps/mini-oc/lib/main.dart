import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:window_manager/window_manager.dart';

import 'pages/home_page.dart';
import 'pages/login_page.dart';
import 'pages/logs_page.dart';
import 'pages/sessions_page.dart';
import 'pages/settings_page.dart';
import 'shell.dart';
import 'src/rust/api.dart' as rust;
import 'stores.dart';
import 'theme.dart';

/// 路由：三级守卫（未登录 redirect /login；已登录主页）。
final routerProvider = Provider<GoRouter>((ref) {
  final auth = ValueNotifier(ref.read(authProvider).isLoggedIn);
  ref.listen(authProvider, (_, next) => auth.value = next.isLoggedIn);

  return GoRouter(
    initialLocation: '/home',
    refreshListenable: auth,
    redirect: (context, state) {
      final loggedIn = auth.value;
      final loggingIn = state.matchedLocation == '/login';
      if (!loggedIn) return loggingIn ? null : '/login';
      if (loggingIn) return '/home';
      return null;
    },
    routes: [
      GoRoute(path: '/login', builder: (_, __) => const LoginPage()),
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
  runApp(const ProviderScope(child: MiniOcApp()));
}

class MiniOcApp extends ConsumerStatefulWidget {
  const MiniOcApp({super.key});

  @override
  ConsumerState<MiniOcApp> createState() => _MiniOcAppState();
}

class _MiniOcAppState extends ConsumerState<MiniOcApp> with WindowListener {
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

  /// 任意关闭路径（点 X / ⌘Q / 系统）→ 优雅停机（上限 6s）后真正退出。
  @override
  void onWindowClose() async {
    try {
      await rust.stopAll().timeout(const Duration(seconds: 6));
    } catch (_) {
      // 超时/失败也放行退出（SIGTERM 已尽力发出）
    }
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
