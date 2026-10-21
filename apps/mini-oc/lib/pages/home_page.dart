import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../stores.dart';
import '../src/rust/supervisor.dart' show ServiceMode, ServiceState;
import '../src/rust/api.dart' as rust;

/// 仪表盘：服务双开关 + 运行状态 + 账户卡。
class HomePage extends ConsumerStatefulWidget {
  const HomePage({super.key});

  @override
  ConsumerState<HomePage> createState() => _HomePageState();
}

class _HomePageState extends ConsumerState<HomePage> {
  bool _starting = false;

  Future<void> _start(bool cloud) async {
    if (_starting) return;
    setState(() => _starting = true);
    final messenger = ScaffoldMessenger.maybeOf(context);
    try {
      if (cloud) {
        await ref.read(rustStartCloudProvider.future);
      } else {
        await ref.read(rustStartLocalProvider.future);
      }
      messenger?.showSnackBar(const SnackBar(content: Text('服务已启动 ✓')));
    } catch (e) {
      messenger?.showSnackBar(SnackBar(content: Text('启动失败：$e')));
    } finally {
      if (mounted) setState(() => _starting = false);
    }
  }

  Future<void> _stop() async {
    if (_starting) return;
    setState(() => _starting = true);
    try {
      await rust.stopAll();
    } catch (e) {
      ScaffoldMessenger.maybeOf(context)?.showSnackBar(SnackBar(content: Text('停止失败：$e')));
    } finally {
      if (mounted) setState(() => _starting = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final auth = ref.watch(authProvider);
    final stateAsync = ref.watch(serviceStateProvider);
    final scheme = Theme.of(context).colorScheme;

    final st = stateAsync.value;
    final running = st != null && st.mode != ServiceMode.off;
    final hasError = st?.lastError != null;

    return ListView(
      padding: const EdgeInsets.all(16),
      children: [
        // 账户卡
        Card(
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: Row(
              children: [
                CircleAvatar(
                  backgroundColor: scheme.primaryContainer,
                  child: Text(
                    auth.userName?.substring(0, 1).toUpperCase() ?? '?',
                    style: TextStyle(color: scheme.onPrimaryContainer, fontWeight: FontWeight.w700),
                  ),
                ),
                const SizedBox(width: 12),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(auth.userName ?? '未登录',
                          style: const TextStyle(fontWeight: FontWeight.w600, fontSize: 16)),
                      Text('#${auth.userId ?? '—'} · 设备 ${auth.boundDevice ?? '未绑定'}',
                          style: TextStyle(color: scheme.onSurfaceVariant, fontSize: 13)),
                    ],
                  ),
                ),
                IconButton(
                  icon: const Icon(Icons.logout),
                  tooltip: '退出登录',
                  onPressed: () async {
                    await ref.read(authProvider.notifier).logout();
                    if (context.mounted) context.go('/login');
                  },
                ),
              ],
            ),
          ),
        ),
        const SizedBox(height: 12),

        // 服务开关卡
        Card(
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                const Text('服务', style: TextStyle(fontWeight: FontWeight.w600, fontSize: 16)),
                const SizedBox(height: 12),
                _SwitchTile(
                  title: '启动云服务',
                  subtitle: 'opencode serve + rathole 隧道',
                  icon: Icons.cloud_outlined,
                  active: running && st!.mode == ServiceMode.cloud,
                  busy: _starting,
                  // 门控：未绑定设备置灰 + 引导
                  enabled: auth.boundDevice != null,
                  onDisabledTap: () => _showBindGuide(context),
                  onChanged: (_) => _start(true),
                ),
                const SizedBox(height: 8),
                _SwitchTile(
                  title: '启动本地服务',
                  subtitle: '仅 opencode serve（本机使用）',
                  icon: Icons.computer_outlined,
                  active: running,
                  busy: _starting,
                  enabled: true,
                  onChanged: (_) => running ? _stop() : _start(false),
                ),
                if (running)
                  TextButton.icon(
                    onPressed: _starting ? null : _stop,
                    icon: const Icon(Icons.stop_circle_outlined, size: 18),
                    label: const Text('停止全部服务'),
                  ),
              ],
            ),
          ),
        ),
        const SizedBox(height: 12),

        // 状态卡
        Card(
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  children: [
                    _ModeDot(mode: st?.mode ?? ServiceMode.off),
                    const SizedBox(width: 8),
                    Text(_modeText(st?.mode ?? ServiceMode.off),
                        style: const TextStyle(fontWeight: FontWeight.w600)),
                    const Spacer(),
                    if (st?.servePid != null)
                      Text('PID ${st!.servePid}',
                          style: TextStyle(color: scheme.onSurfaceVariant, fontSize: 13)),
                  ],
                ),
                const SizedBox(height: 8),
                Text('端口：${st?.port ?? "—"}',
                    style: TextStyle(color: scheme.onSurfaceVariant, fontSize: 13)),
                if (hasError) ...[
                  const SizedBox(height: 12),
                  Container(
                    width: double.infinity,
                    padding: const EdgeInsets.all(12),
                    decoration: BoxDecoration(
                      color: scheme.errorContainer,
                      borderRadius: BorderRadius.circular(12),
                    ),
                    child: Row(
                      children: [
                        Icon(Icons.error_outline, color: scheme.onErrorContainer, size: 18),
                        const SizedBox(width: 8),
                        Expanded(
                          child: Text(st!.lastError!,
                              style: TextStyle(color: scheme.onErrorContainer, fontSize: 13)),
                        ),
                        TextButton(
                          onPressed: _starting ? null : () => _start(false),
                          child: const Text('重试'),
                        ),
                      ],
                    ),
                  ),
                ],
              ],
            ),
          ),
        ),
      ],
    );
  }

  void _showBindGuide(BuildContext context) {
    showDialog<void>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('需要先绑定设备'),
        content: const Text('云服务需要绑定一台登记在服务器设备清单中的设备。\n请联系管理员在你的账号下添加设备，然后重新登录以选择绑定。'),
        actions: [
          TextButton(onPressed: () => Navigator.pop(ctx), child: const Text('知道了')),
        ],
      ),
    );
  }
}

String _modeText(ServiceMode mode) => switch (mode) {
      ServiceMode.cloud => '云服务运行中',
      ServiceMode.localOnly => '本地服务运行中',
      ServiceMode.off => '已停止',
    };

class _ModeDot extends StatelessWidget {
  const _ModeDot({required this.mode});
  final ServiceMode mode;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final color = switch (mode) {
      ServiceMode.cloud || ServiceMode.localOnly => Colors.green.shade600,
      ServiceMode.off => scheme.outline,
    };
    return Container(
      width: 10,
      height: 10,
      decoration: BoxDecoration(color: color, shape: BoxShape.circle),
    );
  }
}

class _SwitchTile extends StatelessWidget {
  const _SwitchTile({
    required this.title,
    required this.subtitle,
    required this.icon,
    required this.active,
    required this.busy,
    required this.enabled,
    required this.onChanged,
    this.onDisabledTap,
  });

  final String title;
  final String subtitle;
  final IconData icon;
  final bool active;
  final bool busy;
  final bool enabled;
  final ValueChanged<bool> onChanged;
  final VoidCallback? onDisabledTap;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Opacity(
      opacity: enabled ? 1 : 0.45,
      child: ListTile(
        leading: Icon(icon, color: active ? scheme.primary : scheme.onSurfaceVariant),
        title: Text(title),
        subtitle: Text(subtitle, style: const TextStyle(fontSize: 12)),
        trailing: busy
            ? const SizedBox(
                width: 22, height: 22, child: CircularProgressIndicator(strokeWidth: 2))
            : Switch(
                value: active,
                // 门控：未启用时不响应（三层门控之 UI 层）
                onChanged: enabled ? onChanged : null,
              ),
        onTap: !enabled ? onDisabledTap : (busy ? null : () => onChanged(!active)),
      ),
    );
  }
}

/// 启动动作 provider（便于测试替换）。
final rustStartLocalProvider = FutureProvider<void>((ref) => rust.startLocal());
final rustStartCloudProvider = FutureProvider<void>((ref) => rust.startCloud());
