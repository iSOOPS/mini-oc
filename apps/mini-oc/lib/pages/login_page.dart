import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../stores.dart';
import '../src/rust/account.dart' show DeviceBrief;

/// 登录页：仅用户密钥输入 + 记住密码 + 登录；成功后进入绑定流程（多设备选择）。
/// 服务器地址在右上角设置图标进入的独立页面填写（保存到 env）。
class LoginPage extends ConsumerStatefulWidget {
  const LoginPage({super.key});

  @override
  ConsumerState<LoginPage> createState() => _LoginPageState();
}

class _LoginPageState extends ConsumerState<LoginPage> {
  final _key = TextEditingController();
  bool _obscure = true;
  bool _remember = false;
  bool _busy = false;
  String? _error;

  @override
  void initState() {
    super.initState();
    // 预填：记住密码（env 勾选与密码）
    final env = ref.read(appEnvProvider);
    _remember = env.rememberPassword;
    if (_remember) _key.text = env.password;
  }

  @override
  void dispose() {
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
      // 地址从 env 读取（设置页保存；server_url_effective 含默认值兜底）
      final serverUrl = ref.read(serverUrlEffectiveProvider);
      final info = await ref
          .read(authProvider.notifier)
          .login(serverUrl, _key.text.trim(), remember: _remember);
      if (!mounted) return;
      // 绑定恢复（I16）：config 已有绑定且清单仍含该设备 → 跳过选择弹层
      // （绑定是持久事实；此前每次登录强制重选，取消即丢绑定 → 云服务置灰）
      final boundInCfg = ref.read(rustCfgProvider).boundDevice;
      final stillBound = boundInCfg != null &&
          info.devices.any((d) => d.name == boundInCfg);
      if (!stillBound && info.devices.isNotEmpty) {
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
      body: Stack(
        children: [
          Center(
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
                      controller: _key,
                      obscureText: _obscure,
                      onSubmitted: (_) => _submit(),
                      decoration: InputDecoration(
                        hintText: '用户密钥',
                        suffixIcon: IconButton(
                          icon: Icon(_obscure ? Icons.visibility_outlined : Icons.visibility_off_outlined),
                          tooltip: _obscure ? '显示密码' : '隐藏密码',
                          onPressed: () => setState(() => _obscure = !_obscure),
                        ),
                      ),
                    ),
                    const SizedBox(height: 4),
                    GestureDetector(
                      behavior: HitTestBehavior.opaque,
                      onTap: () => setState(() => _remember = !_remember),
                      child: Row(
                        children: [
                          Checkbox(
                            value: _remember,
                            onChanged: (v) => setState(() => _remember = v ?? false),
                          ),
                          const Text('记住密码'),
                        ],
                      ),
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
          // 右上角：服务器地址设置入口
          Positioned(
            top: 12,
            right: 12,
            child: IconButton(
              icon: const Icon(Icons.settings_outlined),
              tooltip: '服务器设置',
              onPressed: () => context.push('/login/server'),
            ),
          ),
        ],
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
