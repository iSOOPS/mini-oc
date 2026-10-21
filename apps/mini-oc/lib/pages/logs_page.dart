import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart' show ScrollDirection;
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../src/rust/api.dart' as rust;
import '../src/rust/logbuf.dart' show LogLine;

/// 日志流 provider（批量缓冲，可测试注入）。
final logsProvider = StreamProvider<List<LogLine>>((ref) async* {
  final buffer = <LogLine>[];
  await for (final line in rust.subscribeLogs()) {
    buffer.add(line);
    if (buffer.length > 2000) buffer.removeRange(0, buffer.length - 2000);
    yield List.of(buffer);
  }
});

/// 剪贴板代理（测试可替换/计数）。
class ClipboardProxy {
  static Future<void> Function(String) setter = (text) async {
    await Clipboard.setData(ClipboardData(text: text));
  };
  static Future<void> set(String text) => setter(text);
}

/// 日志页：实时滚动 + 来源过滤 + 全屏 + 选择复制。
class LogsPage extends ConsumerStatefulWidget {
  const LogsPage({super.key});

  @override
  ConsumerState<LogsPage> createState() => _LogsPageState();
}

class _LogsPageState extends ConsumerState<LogsPage> {
  final _scroll = ScrollController();
  String _filter = 'all'; // all | opencode | rathole | system
  bool _fullscreen = false;
  bool _autoFollow = true;

  @override
  void dispose() {
    _scroll.dispose();
    super.dispose();
  }

  void _followBottom() {
    if (_autoFollow && _scroll.hasClients) {
      _scroll.jumpTo(_scroll.position.maxScrollExtent);
    }
  }

  Future<void> _copyAll(List<LogLine> lines) async {
    final text = lines.map((l) => '[${l.source}] ${l.message}').join('\n');
    await ClipboardProxy.set(text);
    if (mounted) {
      ScaffoldMessenger.maybeOf(context)
          ?.showSnackBar(SnackBar(content: Text('已复制 ${lines.length} 行')));
    }
  }

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final logsAsync = ref.watch(logsProvider);
    final all = logsAsync.value ?? const <LogLine>[];
    final lines = _filter == 'all' ? all : all.where((l) => l.source == _filter).toList();

    WidgetsBinding.instance.addPostFrameCallback((_) => _followBottom());

    final body = Column(
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(12, 8, 12, 4),
          child: Row(
            children: [
              Expanded(
                child: SingleChildScrollView(
                  scrollDirection: Axis.horizontal,
                  child: Row(
                    children: [
                      for (final f in [
                        ('all', '全部'),
                        ('opencode', 'opencode'),
                        ('rathole', 'rathole'),
                        ('system', '系统'),
                      ])
                        Padding(
                          padding: const EdgeInsets.only(right: 6),
                          child: FilterChip(
                            label: Text(f.$2),
                            selected: _filter == f.$1,
                            onSelected: (_) => setState(() => _filter = f.$1),
                            visualDensity: VisualDensity.compact,
                          ),
                        ),
                    ],
                  ),
                ),
              ),
              IconButton(
                tooltip: '复制全部',
                icon: const Icon(Icons.copy_all_outlined, size: 20),
                onPressed: lines.isEmpty ? null : () => _copyAll(lines),
              ),
              IconButton(
                tooltip: _fullscreen ? '退出全屏' : '全屏',
                icon: Icon(_fullscreen ? Icons.fullscreen_exit : Icons.fullscreen, size: 20),
                onPressed: () => setState(() => _fullscreen = !_fullscreen),
              ),
            ],
          ),
        ),
        Expanded(
          child: lines.isEmpty
              ? Center(child: Text('暂无日志', style: TextStyle(color: scheme.onSurfaceVariant)))
              : SelectionArea(
                  child: ListView.builder(
                    controller: _scroll,
                    padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
                    itemCount: lines.length,
                    itemBuilder: (_, i) {
                      final l = lines[i];
                      final color = switch (l.level) {
                        'warn' || 'error' => scheme.error,
                        'debug' => scheme.outline,
                        _ => null,
                      };
                      return Padding(
                        padding: const EdgeInsets.symmetric(vertical: 1),
                        child: Text(
                          '[${_fmtTs(l.tsMs)}] [${l.source}] ${l.message}',
                          style: TextStyle(fontSize: 12, fontFamily: 'Menlo', color: color),
                        ),
                      );
                    },
                  ),
                ),
        ),
      ],
    );

    // 全屏：隐藏 AppBar，仅保留工具条
    if (_fullscreen) return Scaffold(body: body);
    return Scaffold(
      appBar: AppBar(title: const Text('日志'), scrolledUnderElevation: 0),
      body: NotificationListener<UserScrollNotification>(
        onNotification: (n) {
          if (n.direction == ScrollDirection.reverse && _autoFollow) {
            setState(() => _autoFollow = false);
          } else if (n.metrics.pixels >= n.metrics.maxScrollExtent - 40 && !_autoFollow) {
            setState(() => _autoFollow = true);
          }
          return false;
        },
        child: body,
      ),
    );
  }

  String _fmtTs(int ms) {
    final t = DateTime.fromMillisecondsSinceEpoch(ms);
    final two = (int v) => v.toString().padLeft(2, '0');
    return '${two(t.hour)}:${two(t.minute)}:${two(t.second)}';
  }
}
