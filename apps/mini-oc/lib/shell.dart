import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';

/// 自适应外壳：宽 ≥900 NavigationRail（桌面），否则 NavigationBar（窄窗）。
class AppShell extends StatelessWidget {
  const AppShell({super.key, required this.child});
  final Widget child;

  static const _tabs = [
    ('/home', Icons.dashboard_outlined, Icons.dashboard, '概览'),
    ('/sessions', Icons.folder_outlined, Icons.folder, '会话'),
    ('/logs', Icons.terminal_outlined, Icons.terminal, '日志'),
    ('/settings', Icons.settings_outlined, Icons.settings, '设置'),
  ];

  @override
  Widget build(BuildContext context) {
    final location = GoRouterState.of(context).matchedLocation;
    final index = _tabs.indexWhere((t) => location.startsWith(t.$1)).clamp(0, _tabs.length - 1);
    final wide = MediaQuery.sizeOf(context).width >= 900;

    if (wide) {
      return Scaffold(
        body: Row(
          children: [
            NavigationRail(
              selectedIndex: index,
              onDestinationSelected: (i) => context.go(_tabs[i].$1),
              labelType: NavigationRailLabelType.all,
              destinations: [
                for (final t in _tabs)
                  NavigationRailDestination(
                    icon: Icon(t.$2),
                    selectedIcon: Icon(t.$3),
                    label: Text(t.$4),
                  ),
              ],
            ),
            VerticalDivider(width: 1, thickness: 1, color: Theme.of(context).dividerColor),
            Expanded(child: child),
          ],
        ),
      );
    }
    return Scaffold(
      body: child,
      bottomNavigationBar: NavigationBar(
        selectedIndex: index,
        onDestinationSelected: (i) => context.go(_tabs[i].$1),
        destinations: [
          for (final t in _tabs)
            NavigationDestination(
              icon: Icon(t.$2),
              selectedIcon: Icon(t.$3),
              label: t.$4,
            ),
        ],
      ),
    );
  }
}
