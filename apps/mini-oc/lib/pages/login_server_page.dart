import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../stores.dart';

/// 登录页·服务器地址设置：修改访问地址并保存到 env 文件。
/// 默认值 https://oc.isoops.com；docker 注入 MINI_OC_* 时保存自动跳过落盘。
class LoginServerPage extends ConsumerStatefulWidget {
  const LoginServerPage({super.key});

  @override
  ConsumerState<LoginServerPage> createState() => _LoginServerPageState();
}

class _LoginServerPageState extends ConsumerState<LoginServerPage> {
  late final TextEditingController _server;
  bool _saving = false;

  @override
  void initState() {
    super.initState();
    _server = TextEditingController(text: ref.read(serverUrlEffectiveProvider));
  }

  @override
  void dispose() {
    _server.dispose();
    super.dispose();
  }

  Future<void> _save() async {
    if (_saving) return;
    setState(() => _saving = true);
    try {
      final url = _server.text.trim();
      await ref.read(authProvider.notifier).saveServerUrl(url);
      if (!mounted) return;
      ScaffoldMessenger.maybeOf(context)
          ?.showSnackBar(const SnackBar(content: Text('已保存')));
      if (context.canPop()) {
        context.pop();
      } else {
        context.go('/login');
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.maybeOf(context)?.showSnackBar(SnackBar(content: Text('保存失败：$e')));
      }
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Scaffold(
      appBar: AppBar(title: const Text('服务器设置'), scrolledUnderElevation: 0),
      body: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 400),
          child: Padding(
            padding: const EdgeInsets.all(24),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Text('访问地址',
                    style: TextStyle(
                        color: scheme.onSurfaceVariant, fontSize: 13)),
                const SizedBox(height: 8),
                TextField(
                  controller: _server,
                  autofocus: true,
                  decoration: const InputDecoration(
                    hintText: 'https://oc.isoops.com',
                  ),
                  onSubmitted: (_) => _save(),
                ),
                const SizedBox(height: 8),
                Text('默认 https://oc.isoops.com，可修改；保存后立即生效。',
                    style: TextStyle(
                        color: scheme.onSurfaceVariant, fontSize: 12)),
                const SizedBox(height: 24),
                FilledButton(
                  onPressed: _saving ? null : _save,
                  child: _saving
                      ? const SizedBox(height: 18, width: 18, child: CircularProgressIndicator(strokeWidth: 2))
                      : const Text('保存'),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
