import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../stores.dart';
import '../src/rust/config.dart' show ClientConfig;

/// 由表单值 + 当前配置构造新 ClientConfig（保存与测试共用，保证字段透传一致）。
///
/// ⚠️ 设备身份透传：deviceId / user / boundDevice 必须从 [current] 原样带入，
/// 否则点一次「保存」就丢设备身份（builder check 高危项）。
/// theme 透传 [ClientConfig.theme]（外观设置 UI 已移除，I19）。
ClientConfig buildSavedConfig({
  required String serverText,
  required String portText,
  required ClientConfig current,
}) {
  return ClientConfig(
    serverUrl: serverText.trim(),
    user: current.user,
    boundDevice: current.boundDevice,
    port: int.tryParse(portText.trim()) ?? current.port,
    opencodeBin: current.opencodeBin,
    deviceId: current.deviceId,
    theme: current.theme,
  );
}

/// 设置页：服务器地址、端口偏好、设备上报、主题、关于。
class SettingsPage extends ConsumerStatefulWidget {
  const SettingsPage({super.key});

  @override
  ConsumerState<SettingsPage> createState() => _SettingsPageState();
}

class _SettingsPageState extends ConsumerState<SettingsPage> {
  // 表单控制器在 initState 由最新配置初始化（保存成功/Rust 侧写盘后重进页面
  // 显示最新值；late final + provider 缓存会永远显示首次的旧值——I11）
  late final TextEditingController _server;
  late final TextEditingController _port;
  // 进入页面时的服务器地址快照（保存时对比判定是否需提示重新登录，I19）
  late final String _initialServer;
  bool _dirty = false;

  @override
  void initState() {
    super.initState();
    // 强制重算（同 _save）：绕过 provider 缓存拿 config.json 最新值
    final cfg = ref.refresh(rustCfgProvider);
    _initialServer = cfg.serverUrl;
    _server = TextEditingController(text: cfg.serverUrl);
    _port = TextEditingController(text: cfg.port.toString());
    // pending 态进页面 → 自动静默重报一次（重报幂等，是感知管理员批准的唯一途径；
    // 批准后状态更新为已绑定并自动完成绑定，主页云服务开关随即可用）
    WidgetsBinding.instance.addPostFrameCallback((_) {
      // 进入设置页 → 从 web 端拉真实上报状态（I12：内存态 App 重启即丢，
      // 不能让已上报设备显示「尚未上报」；只读无副作用）
      ref.read(registrationProvider.notifier).restoreFromServer();
      // pending 态 → 自动静默重报一次（重报幂等，是感知管理员批准的唯一途径；
      // 批准后状态更新为已绑定并自动完成绑定，主页云服务开关随即可用）
      final outcome = ref.read(registrationProvider);
      if (outcome?.ok == true && outcome?.status == 'pending') {
        ref.read(registrationProvider.notifier).report();
      }
    });
  }

  @override
  void dispose() {
    _server.dispose();
    _port.dispose();
    super.dispose();
  }

  Future<void> _save() async {
    // 强制重算（= rust.loadConfig()），防止用陈旧快照覆盖 Rust 侧已写回的
    // deviceId / user / boundDevice（如上报后 config.json 已更新而缓存未刷）。
    // 测试 overrideWithValue / overrideWith 注入的值不受影响（Riverpod 对
    // override 的 provider 跳过重算，refresh 返回注入值本身）。
    final cfg = ref.refresh(rustCfgProvider);
    final newServer = _server.text.trim();
    final newCfg = buildSavedConfig(
      serverText: _server.text,
      portText: _port.text,
      current: cfg,
    );
    try {
      await ref.read(saveConfigFnProvider)(cfg: newCfg);
      // 保存成功 → invalidate 配置快照 provider（下次任何页面读取都拿最新值）
      ref.invalidate(rustCfgProvider);
      if (mounted) {
        setState(() => _dirty = false);
        // 提示按实际改动区分（I19）：仅改端口等无需重启；服务器地址变更
        // 才提示重新登录（旧提示对任何改动都带重启字样，误导）
        final serverChanged = newServer != _initialServer;
        ScaffoldMessenger.maybeOf(context)?.showSnackBar(SnackBar(
            content: Text(serverChanged
                ? '已保存（服务器地址修改需重新登录生效）'
                : '已保存 ✓')));
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
      // pending 时按钮保持可点：重报幂等且是感知管理员批准的唯一途径
      // （批准后重报 → approved+enabled → 自动绑定）。禁用会造成死锁。
      statusText = '待审核：管理员批准后点击「上报」刷新状态';
      enabled = true;
    } else if (outcome.status == 'approved' && outcome.enabled && outcome.bound) {
      statusText = '已绑定：可在主页启动云服务';
      enabled = false;
    } else if (outcome.status == 'approved' && outcome.enabled && !outcome.bound) {
      // 批准但自动绑定未成功（如清单暂未同步）→ 允许重新上报以重试绑定
      statusText = '已批准，未绑定：点击「上报」重试';
      statusColor = scheme.error;
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
