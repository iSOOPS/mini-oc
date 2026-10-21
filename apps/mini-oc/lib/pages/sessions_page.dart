import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../stores.dart';
import '../src/rust/api.dart' show ProjectSummary, SessionSummary;
import '../src/rust/supervisor.dart' show ServiceMode;

/// 项目 → 会话浏览器：检索、点击开终端。
class SessionsPage extends ConsumerStatefulWidget {
  const SessionsPage({super.key});

  @override
  ConsumerState<SessionsPage> createState() => _SessionsPageState();
}

class _SessionsPageState extends ConsumerState<SessionsPage> {
  final _search = TextEditingController();
  String? _expandedProjectPath;
  final Set<String> _loadedPaths = {};

  @override
  void initState() {
    super.initState();
    _search.addListener(() => setState(() {}));
  }

  @override
  void dispose() {
    _search.dispose();
    super.dispose();
  }

  void _toggleProject(ProjectSummary p) {
    setState(() => _expandedProjectPath = _expandedProjectPath == p.path ? null : p.path);
  }

  Future<void> _openTerminal(String directory, String? sessionId) async {
    final messenger = ScaffoldMessenger.maybeOf(context);
    try {
      await ref.read(openInTerminalProvider)(directory, sessionId);
      messenger?.showSnackBar(const SnackBar(content: Text('已在系统终端打开 ✓')));
    } catch (e) {
      messenger?.showSnackBar(SnackBar(content: Text('打开失败：$e')));
    }
  }

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final serviceAsync = ref.watch(serviceStateProvider);
    final serveRunning =
        serviceAsync.value?.mode != null && serviceAsync.value!.mode != ServiceMode.off;

    final projectsAsync = ref.watch(listProjectsProvider);
    final query = _search.text.trim().toLowerCase();
    final projects = (projectsAsync.value ?? const <ProjectSummary>[])
        .where((p) => query.isEmpty
            ? true
            : p.path.toLowerCase().contains(query) || p.name.toLowerCase().contains(query))
        .toList();

    return Scaffold(
      body: !serveRunning
          ? Center(
              child: ConstrainedBox(
                constraints: const BoxConstraints(maxWidth: 420),
                child: Card(
                  child: Padding(
                    padding: const EdgeInsets.all(24),
                    child: Column(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        Icon(Icons.power_off_outlined, size: 44, color: scheme.outline),
                        const SizedBox(height: 12),
                        const Text('服务未启动',
                            style: TextStyle(fontWeight: FontWeight.w600, fontSize: 16)),
                        const SizedBox(height: 8),
                        Text('项目与会话数据来自本地 opencode serve。\n请先在「概览」页启动服务。',
                            textAlign: TextAlign.center,
                            style: TextStyle(color: scheme.onSurfaceVariant, fontSize: 13)),
                      ],
                    ),
                  ),
                ),
              ),
            )
          : Column(
              children: [
                Padding(
                  padding: const EdgeInsets.fromLTRB(16, 16, 16, 8),
                  child: Row(
                    children: [
                      Expanded(
                        child: TextField(
                          controller: _search,
                          decoration: const InputDecoration(
                              hintText: '🔍 检索项目路径或名称', isDense: true),
                        ),
                      ),
                      const SizedBox(width: 8),
                      IconButton(
                        tooltip: '刷新',
                        onPressed: () => ref.invalidate(listProjectsProvider),
                        icon: const Icon(Icons.refresh),
                      ),
                    ],
                  ),
                ),
                if (projectsAsync.isLoading) const LinearProgressIndicator(minHeight: 2),
                if (projectsAsync.hasError)
                  Padding(
                    padding: const EdgeInsets.symmetric(horizontal: 16),
                    child: Card(
                      child: Padding(
                        padding: const EdgeInsets.all(12),
                        child: Text('${projectsAsync.error}',
                            style: TextStyle(color: scheme.error)),
                      ),
                    ),
                  ),
                Expanded(
                  child:
                      projects.isEmpty && !projectsAsync.isLoading && !projectsAsync.hasError
                          ? const Center(
                              child: Text('暂无项目——用 opencode 打开过的项目会出现在这里'))
                          : ListView.builder(
                              padding: const EdgeInsets.fromLTRB(16, 8, 16, 16),
                              itemCount: projects.length,
                              itemBuilder: (_, i) {
                                final p = projects[i];
                                final expanded = _expandedProjectPath == p.path;
                                return _projectTile(context, p, expanded);
                              },
                            ),
                ),
              ],
            ),
    );
  }

  Widget _projectTile(BuildContext context, ProjectSummary p, bool expanded) {
    final sessionsAsync =
        expanded && _loadedPaths.contains(p.path) ? ref.watch(listSessionsProvider(p.path)) : null;
    return Column(
      children: [
        Card(
          child: ListTile(
            leading: p.vcs != null
                ? const Icon(Icons.folder_special_outlined)
                : const Icon(Icons.folder_outlined),
            title: Text(p.name, maxLines: 1, overflow: TextOverflow.ellipsis),
            subtitle: Text(p.path,
                maxLines: 1, overflow: TextOverflow.ellipsis, style: const TextStyle(fontSize: 12)),
            trailing: Icon(expanded ? Icons.expand_less : Icons.expand_more),
            onTap: () {
              _loadedPaths.add(p.path);
              _toggleProject(p);
            },
          ),
        ),
        if (expanded) ..._sessionTiles(sessionsAsync?.value ?? const [], p.path),
        const SizedBox(height: 8),
      ],
    );
  }

  List<Widget> _sessionTiles(List<SessionSummary> sessions, String projectPath) {
    if (sessions.isEmpty) {
      return const [Padding(padding: EdgeInsets.all(12), child: Text('暂无会话', style: TextStyle(fontSize: 13)))];
    }
    final query = _search.text.trim().toLowerCase();
    return sessions
        .where((s) => query.isEmpty
            ? true
            : (s.title ?? '').toLowerCase().contains(query) || s.directory.toLowerCase().contains(query))
        .map((s) => Padding(
              padding: const EdgeInsets.only(left: 16),
              child: Card(
                child: ListTile(
                  dense: true,
                  leading: const Icon(Icons.chat_bubble_outline, size: 20),
                  title: Text(s.title ?? '（无标题会话）',
                      maxLines: 1, overflow: TextOverflow.ellipsis),
                  subtitle: Text(_fmtUpdated(s.updatedMs), style: const TextStyle(fontSize: 11)),
                  trailing: IconButton(
                    tooltip: '在终端中打开',
                    icon: const Icon(Icons.open_in_new, size: 18),
                    onPressed: () => _openTerminal(s.directory, s.id),
                  ),
                  onTap: () => _openTerminal(s.directory, s.id),
                ),
              ),
            ))
        .toList();
  }

  String _fmtUpdated(int ms) {
    final diff = DateTime.now().millisecondsSinceEpoch - ms;
    if (diff < 3600_000) return '${(diff / 60_000).round().clamp(1, 59)} 分钟前';
    if (diff < 86400_000) return '${(diff / 3600_000).round()} 小时前';
    return DateTime.fromMillisecondsSinceEpoch(ms).toLocal().toString().substring(0, 10);
  }
}
