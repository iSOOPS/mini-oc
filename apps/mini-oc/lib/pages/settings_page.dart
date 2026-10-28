import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../stores.dart';
import '../src/rust/api.dart' as rust;
import '../src/rust/config.dart' show ClientConfig;

/// 由表单值 + 当前配置构造新 ClientConfig（保存与测试共用，保证字段透传一致）。
///
/// ⚠️ 设备身份透传：deviceId / user / boundDevice 必须从 [current] 原样带入，
/// 否则点一次「保存」就丢设备身份（builder check 高危项）。
ClientConfig buildSavedConfig({
  required String serverText,
  required String portText,
  required String theme,
  required ClientConfig current,
}) {
  return ClientConfig(
    serverUrl: serverText.trim(),
    user: current.user,
    boundDevice: current.boundDevice,
    port: int.tryParse(portText.trim()) ?? current.port,
    opencodeBin: current.opencodeBin,
    deviceId: current.deviceId,
    theme: theme,
  );
}

/// 设置页：服务器地址、端口偏好、设备上报、主题、关于。
class SettingsPage extends ConsumerStatefulWidget {
  const SettingsPage({super.key});

  @override
  ConsumerState<SettingsPage> createState() => _SettingsPageState();
}

class _SettingsPageState extends ConsumerState<SettingsPage> {
  // 经 rustCfgProvider 读初始值（测试可注入；生产等价 rust.loadConfig()）
  late final _server =
      TextEditingController(text: ref.read(rustCfgProvider).serverUrl);
  late final _port =
      TextEditingController(text: ref.read(rustCfgProvider).port.toString());
  String _theme = 'system';
  bool _dirty = false;

  @override
  void dispose() {
    _server.dispose();
    _port.dispose();
    super.dispose();
  }

  Future<void> _save() async {
    final cfg = ref.read(rustCfgProvider);
    final newCfg = buildSavedConfig(
      serverText: _server.text,
      portText: _port.text,
      theme: _theme,
      current: cfg,
    );
    try {
      await rust.saveConfig(cfg: newCfg);
      if (mounted) {
        setState(() => _dirty = false);
        ScaffoldMessenger.maybeOf(context)?.showSnackBar(
            const SnackBar(content: Text('已保存（服务器地址修改需重新登录生效）')));
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.maybeOf(context)?.showSnackBar(SnackBar(content: Text('保存失败：$e')));
      }
    }
  }

  /// 设备上报区：状态文本 + 独立「上报」按钮（状态机见 spec「上报时机」节）。
  Widget _buildReportSection() {
    final outcome = ref.watch(registrationProvider);
    final scheme = Theme.of(context).colorScheme;

    // 状态推导：(按钮可点, 状态文案, 文案颜色)
    String statusText;
    var enabled = true;
    var statusColor = scheme.onSurfaceVariant;
    if (outcome == null) {
      statusText = '尚未上报：点击「上报」把本机信息登记给服务器';
    } else if (outcome.ok != true) {
      statusText = '上报失败：${outcome.error ?? '未知错误'}';
      statusColor = scheme.error;
    } else if (outcome.status == 'pending') {
      // 定夺：不做轮询，重进设置页不重查；上报幂等，审核被拒可重报
      statusText = '待审核';
      enabled = false;
    } else if (outcome.status == 'approved' && outcome.enabled) {
      statusText = '已上报';
      enabled = false;
    } else if (outcome.status == 'approved' && !outcome.enabled) {
      statusText = '已停用';
      statusColor = scheme.error;
    } else {
      statusText = '状态未知（${outcome.status}），可重新上报';
    }

    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 8),
      child: Row(
        children: [
          Expanded(
            child: Text(statusText,
                style: TextStyle(fontSize: 13, color: statusColor)),
          ),
          const SizedBox(width: 12),
          FilledButton.tonal(
            onPressed: enabled
                ? () async {
                    await ref.read(registrationProvider.notifier).report();
                  }
                : null,
            child: const Text('上报'),
          ),
        ],
      ),
    );
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
            _field('opencode 端口（上报给服务端）', _port, hint: '9464'),
          ]),
          _section(context, '设备上报', [_buildReportSection()]),
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
