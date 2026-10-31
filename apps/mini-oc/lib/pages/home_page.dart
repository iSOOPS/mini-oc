import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../stores.dart';
import '../src/rust/portkill.dart' show PortOccupant;
import '../src/rust/supervisor.dart' show ServiceMode;

/// 服务启停互斥（全局，跨页面存活）：
/// HomePage 是 ShellRoute 子路由，切换页面即 dispose——若把「正在启动」放
/// State 局部变量，切页后互斥丢失（按钮重新可点，可重复触发启动）。
class ServiceActionBusyNotifier extends Notifier<bool> {
  @override
  bool build() => false;

  /// 运行 [action] 期间置 busy（重入直接忽略，防重复点击/双开关并发）。
  Future<T> run<T>(Future<T> Function() action) async {
    if (state) return Future<T>.error('操作进行中');
    state = true;
    try {
      return await action();
    } finally {
      state = false;
    }
  }
}

final serviceActionBusyProvider =
    NotifierProvider<ServiceActionBusyNotifier, bool>(ServiceActionBusyNotifier.new);

/// 仪表盘：服务双开关 + 运行状态 + 账户卡。
class HomePage extends ConsumerStatefulWidget {
  const HomePage({super.key});

  @override
  ConsumerState<HomePage> createState() => _HomePageState();
}

/// 服务启停动作种类（I20）。
enum _StartKind { all, local, tunnel }
enum _StopKind { all, local, tunnel }

class _HomePageState extends ConsumerState<HomePage> {
  /// 从错误中提取被占端口；非端口占用错误返回 null。
  int? _portFromError(Object e) {
    final msg = e.toString();
    final prefix = ref.read(portErrPrefixFnProvider)();
    final i = msg.indexOf(prefix);
    if (i < 0) return null;
    final rest = msg.substring(i + prefix.length);
    final match = RegExp(r'^(\d+)').firstMatch(rest);
    return match == null ? null : int.tryParse(match.group(1)!);
  }

  /// 启动失败统一入口：端口占用且有占用者 → 弹框（杀死并重试）；否则 SnackBar。
  Future<void> _handleStartError(Object e, _StartKind kind) async {
    final messenger = ScaffoldMessenger.maybeOf(context);
    final port = _portFromError(e);
    if (port != null) {
      final occupant = ref.read(queryPortOccupantFnProvider)(port);
      if (occupant != null) {
        final kill = await _showPortInUseDialog(port, occupant);
        if (kill) {
          await _start(kind, retryAfterKill: port);
        }
        return; // 弹框路径：不再叠加 SnackBar
      }
    }
    messenger?.showSnackBar(SnackBar(content: Text('启动失败：$e')));
  }

  /// 端口占用确认框。返回 true = 用户选择「结束进程并重试」。
  Future<bool> _showPortInUseDialog(int port, PortOccupant occupant) async {
    final result = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('端口被占用'),
        content: Text(
          '端口 $port 被 ${occupant.name}（PID ${occupant.pid}）占用。\n'
          '结束该进程后自动重试启动？',
        ),
        actions: [
          TextButton(onPressed: () => Navigator.pop(ctx, false), child: const Text('取消')),
          FilledButton(
            onPressed: () => Navigator.pop(ctx, true),
            child: const Text('结束进程并重试'),
          ),
        ],
      ),
    );
    return result ?? false;
  }

  Future<void> _start(_StartKind kind, {int? retryAfterKill}) async {
    final messenger = ScaffoldMessenger.maybeOf(context);
    final busy = ref.read(serviceActionBusyProvider.notifier);
    try {
      await busy.run(() async {
        // 杀死占用者后重试（I10）：先杀再启
        if (retryAfterKill != null) {
          await ref.read(killPortOccupantFnProvider)(retryAfterKill);
        }
        // 每次真实发起（不能用缓存的 future——重复调用只会重放旧结果，
        // 服务根本不会再次启动）
        switch (kind) {
          case _StartKind.all:
            await ref.read(startAllFnProvider)();
          case _StartKind.local:
            await ref.read(startLocalFnProvider)();
          case _StartKind.tunnel:
            await ref.read(startTunnelFnProvider)();
        }
      });
      if (!mounted) return;
      messenger?.showSnackBar(const SnackBar(content: Text('服务已启动 ✓')));
    } catch (e) {
      if (!mounted) return;
      await _handleStartError(e, kind);
    }
  }

  Future<void> _stop(_StopKind kind) async {
    final messenger = ScaffoldMessenger.maybeOf(context);
    final busy = ref.read(serviceActionBusyProvider.notifier);
    try {
      await busy.run(() async {
        switch (kind) {
          case _StopKind.all:
            await ref.read(stopAllFnProvider)();
          case _StopKind.local:
            await ref.read(stopLocalFnProvider)();
          case _StopKind.tunnel:
            await ref.read(stopTunnelFnProvider)();
        }
      });
    } catch (e) {
      if (!mounted) return;
      messenger?.showSnackBar(SnackBar(content: Text('停止失败：$e')));
    }
  }

  @override
  Widget build(BuildContext context) {
    final auth = ref.watch(authProvider);
    final stateAsync = ref.watch(serviceStateProvider);
    final starting = ref.watch(serviceActionBusyProvider);
    final scheme = Theme.of(context).colorScheme;

    final st = stateAsync.value;
    final running = st != null && st.mode != ServiceMode.off;
    // 双开关独立状态（I20）：隧道在跑 = rathole 存在；本地在跑 = serve 存在
    final tunnelOn =
        st != null && (st.mode == ServiceMode.cloud || st.mode == ServiceMode.tunnelOnly);
    final serveOn =
        st != null && (st.mode == ServiceMode.cloud || st.mode == ServiceMode.localOnly);
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

        // 服务开关卡（I20 重设计：隧道 + 本地 双开关，标题右侧「启动全部/停止全部」）
        Card(
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  children: [
                    const Text('服务', style: TextStyle(fontWeight: FontWeight.w600, fontSize: 16)),
                    const Spacer(),
                    // 任一在跑 →「停止全部」；全停 →「启动全部」
                    FilledButton.tonal(
                      onPressed: starting
                          ? null
                          : () => running
                              ? _stop(_StopKind.all)
                              : _start(_StartKind.all),
                      child: Text(running ? '停止全部' : '启动全部'),
                    ),
                  ],
                ),
                const SizedBox(height: 12),
                _SwitchTile(
                  title: '启动隧道',
                  subtitle: '仅 rathole（远程访问已运行的本地服务）',
                  icon: Icons.vpn_lock_outlined,
                  active: tunnelOn,
                  busy: starting,
                  // 门控：未绑定设备置灰 + 引导
                  enabled: auth.boundDevice != null,
                  onDisabledTap: () => _showBindGuide(context),
                  // toggle 语义：开 → 停隧道保本地；关 → 仅起隧道
                  onChanged: (_) => tunnelOn ? _stop(_StopKind.tunnel) : _start(_StartKind.tunnel),
                ),
                const SizedBox(height: 8),
                _SwitchTile(
                  title: '启动本地服务',
                  subtitle: '仅 opencode serve（本机使用）',
                  icon: Icons.computer_outlined,
                  active: serveOn,
                  busy: starting,
                  enabled: true,
                  // toggle 语义：开 → 停 serve 保隧道；关 → 仅起 serve
                  onChanged: (_) => serveOn ? _stop(_StopKind.local) : _start(_StartKind.local),
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
                          child: Text(st?.lastError ?? '',
                              style: TextStyle(color: scheme.onErrorContainer, fontSize: 13)),
                        ),
                        TextButton(
                          onPressed: starting ? null : () => _start(_StartKind.local),
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
      ServiceMode.cloud => '全部服务运行中',
      ServiceMode.localOnly => '本地服务运行中',
      ServiceMode.tunnelOnly => '隧道运行中',
      ServiceMode.off => '已停止',
    };

class _ModeDot extends StatelessWidget {
  const _ModeDot({required this.mode});
  final ServiceMode mode;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final color = switch (mode) {
      ServiceMode.cloud ||
      ServiceMode.localOnly ||
      ServiceMode.tunnelOnly => Colors.green.shade600,
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
