import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'pages/home_page.dart';
import 'pages/login_page.dart';
import 'theme.dart';
import 'pages/logs_page.dart';
import 'pages/sessions_page.dart';
import 'pages/settings_page.dart';
import 'shell.dart';
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

void main() {
  runApp(const ProviderScope(child: MiniOcApp()));
}

class MiniOcApp extends ConsumerWidget {
  const MiniOcApp({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
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
