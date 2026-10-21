import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../stores.dart';
import '../src/rust/api.dart' as rust;

/// 设置页：服务器地址、opencode 路径、端口偏好、主题、关于。
class SettingsPage extends ConsumerStatefulWidget {
  const SettingsPage({super.key});

  @override
  ConsumerState<SettingsPage> createState() => _SettingsPageState();
}

class _SettingsPageState extends ConsumerState<SettingsPage> {
  late final _server = TextEditingController(text: rust.loadConfig().serverUrl);
  late final _bin = TextEditingController(text: rust.loadConfig().opencodeBin ?? '');
  late final _port =
      TextEditingController(text: rust.loadConfig().port.toString());
  String _theme = 'system';
  bool _dirty = false;

  @override
  void dispose() {
    _server.dispose();
    _bin.dispose();
    _port.dispose();
    super.dispose();
  }

  Future<void> _save() async {
    final cfg = rust.loadConfig();
    final newCfg = rust.ClientConfig(
      serverUrl: _server.text.trim(),
      user: cfg.user,
      boundDevice: cfg.boundDevice,
      port: int.tryParse(_port.text.trim()) ?? cfg.port,
      opencodeBin: _bin.text.trim().isEmpty ? null : _bin.text.trim(),
      theme: _theme,
    );
    try {
      await rust.saveConfig(cfg: newCfg);
      if (mounted) {
        setState(() => _dirty = false);
        ScaffoldMessenger.maybeOf(context)
            ?.showSnackBar(const SnackBar(content: Text('已保存（服务器地址修改需重新登录生效）')));
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.maybeOf(context).showSnackBar(SnackBar(content: Text('保存失败：$e')));
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Scaffold(
      appBar: AppBar(title: const Text('设置'), scrolledUnderElevation: 0),
      body: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          _section(context, '连接', [
            _field('服务器地址（mini-oc-web）', _server,
                hint: 'https://oc.example.com'),
          ]),
          _section(context, '本地', [
            _field('opencode 二进制路径（留空自动探测）', _bin, hint: 'opencode'),
            _field('本地端口偏好（绑定设备后以清单为准）', _port, hint: '9464'),
          ]),
          _section(context, '外观', [
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 8),
              child: SegmentedButton<String>(
                segments: const [
                  ButtonSegment(value: 'system', label: Text('跟随系统')),
                  ButtonSegment(value: 'light', label: Text('亮色')),
                  ButtonSegment(value: 'dark', label: Text('暗色')),
                ],
                selected: {_theme},
                onSelectionChanged: (s) => setState(() {
                  _theme = s.first;
                  _dirty = true;
                }),
              ),
            ),
          ]),
          const SizedBox(height: 8),
          FilledButton(
            onPressed: _dirty ? _save : null,
            child: const Text('保存设置'),
          ),
          const SizedBox(height: 16),
          Card(
            child: Padding(
              padding: const EdgeInsets.all(16),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  const Text('关于',
                      style: TextStyle(fontWeight: FontWeight.w600, fontSize: 15)),
                  const SizedBox(height: 6),
                  Text('mini-oc v2.0.0',
                      style: TextStyle(color: scheme.onSurfaceVariant, fontSize: 13)),
                  Text('opencode 远程工作站客户端',
                      style: TextStyle(color: scheme.onSurfaceVariant, fontSize: 13)),
                ],
              ),
            ),
          ),
        ],
      ),
    );
  }

  Widget _section(BuildContext context, String title, List<Widget> children) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(4, 12, 0, 6),
          child: Text(title,
              style: TextStyle(
                  color: Theme.of(context).colorScheme.primary,
                  fontWeight: FontWeight.w600,
                  fontSize: 13)),
        ),
        Card(child: Padding(padding: const EdgeInsets.all(8), child: Column(children: children))),
      ],
    );
  }

  Widget _field(String label, TextEditingController ctrl, {String? hint}) {
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 6),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(label, style: const TextStyle(fontSize: 13)),
          const SizedBox(height: 6),
          TextField(
            controller: ctrl,
            decoration: InputDecoration(hintText: hint, isDense: true),
            onChanged: (_) => setState(() => _dirty = true),
          ),
        ],
      ),
    );
  }
}
