import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../stores.dart';
import '../src/rust/account.dart' show DeviceBrief;

/// 登录页：服务器地址 + 密钥；成功后进入绑定流程（多设备选择）。
class LoginPage extends ConsumerStatefulWidget {
  const LoginPage({super.key});

  @override
  ConsumerState<LoginPage> createState() => _LoginPageState();
}

class _LoginPageState extends ConsumerState<LoginPage> {
  final _server = TextEditingController();
  final _key = TextEditingController();
  bool _busy = false;
  String? _error;

  @override
  void initState() {
    super.initState();
    // 预填已保存的服务器地址
    final cfg = ref.read(rustCfgProvider);
    _server.text = cfg.serverUrl;
  }

  @override
  void dispose() {
    _server.dispose();
    _key.dispose();
    super.dispose();
  }

  Future<void> _submit() async {
    if (_busy) return;
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final info =
          await ref.read(authProvider.notifier).login(_server.text.trim(), _key.text.trim());
      if (!mounted) return;
      if (info.devices.isNotEmpty) {
        // 绑定流程（I5 修复）：设备选择弹层——云服务的前置
        final picked = await showDialog<String>(
          context: context,
          barrierDismissible: false,
          builder: (ctx) => _DevicePickerDialog(devices: info.devices),
        );
        if (picked != null) {
          await ref.read(authProvider.notifier).bindDevice(picked);
        }
      }
      if (mounted) context.go('/home');
    } catch (e) {
      setState(() => _error = e.toString());
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Scaffold(
      body: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 400),
          child: Padding(
            padding: const EdgeInsets.all(24),
            child: Column(
              mainAxisAlignment: MainAxisAlignment.center,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Text('mini·oc', style: Theme.of(context).textTheme.displaySmall?.copyWith(
                  color: scheme.primary, fontWeight: FontWeight.w800)),
                const SizedBox(height: 4),
                Text('AI 编程工作站', textAlign: TextAlign.center,
                    style: TextStyle(color: scheme.onSurfaceVariant)),
                const SizedBox(height: 32),
                TextField(
                  controller: _server,
                  decoration: const InputDecoration(hintText: '服务器地址（https://…）'),
                ),
                const SizedBox(height: 12),
                TextField(
                  controller: _key,
                  obscureText: true,
                  onSubmitted: (_) => _submit(),
                  decoration: const InputDecoration(hintText: '用户密钥'),
                ),
                if (_error != null) ...[
                  const SizedBox(height: 12),
                  Text(_error!, style: TextStyle(color: scheme.error)),
                ],
                const SizedBox(height: 20),
                FilledButton(
                  onPressed: _busy ? null : _submit,
                  child: _busy
                      ? const SizedBox(height: 18, width: 18, child: CircularProgressIndicator(strokeWidth: 2))
                      : const Text('登录'),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}


/// 设备选择弹层（绑定流程入口，I5）。
class _DevicePickerDialog extends StatelessWidget {
  const _DevicePickerDialog({required this.devices});
  final List<DeviceBrief> devices;

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: const Text('选择要绑定的设备'),
      content: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          for (final d in devices)
            ListTile(
              leading: Icon(d.pctype == 'windows' ? Icons.desktop_windows : Icons.laptop_mac),
              title: Text(d.name),
              subtitle: Text('端口 ${d.port} · ${d.deviceName.isEmpty ? "未绑定" : d.deviceName}'),
              onTap: () => Navigator.pop(context, d.name),
            ),
        ],
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context), // 取消 = 暂不绑定（本地服务仍可用）
          child: const Text('暂不绑定'),
        ),
      ],
    );
  }
}
