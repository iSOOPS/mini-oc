import 'dart:io';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated_io.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mini_oc/src/rust/api/simple.dart' as simple;
import 'package:mini_oc/src/rust/frb_generated.dart';
import 'package:mini_oc/src/rust/api.dart' as rust;

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  // flutter test 的 Dart VM 不触发 cargokit 构建——直接加载预构建 dylib。
  // （cargo build 在 apps/mini-oc/rust 产出 target/debug/librust_lib_mini_oc.dylib）
  setUpAll(() async {
    // 按平台选动态库扩展名：macOS .dylib / Windows .dll / Linux .so
    final libName = Platform.isWindows
        ? 'rust_lib_mini_oc.dll'
        : (Platform.isMacOS ? 'librust_lib_mini_oc.dylib' : 'librust_lib_mini_oc.so');
    final candidates = [
      '../rust/target/debug/$libName',
      'rust/target/debug/$libName',
    ];
    final path = candidates.firstWhere(
      (c) => File(c).existsSync(),
      orElse: () => candidates.first,
    );
    await RustLib.init(
      externalLibrary: ExternalLibrary.open(path),
    );
  });

  test('rust bridge ping returns pong', () async {
    expect(rust.ping(), 'pong');
  });

  test('rust bridge greet (frb 生成验证)', () async {
    expect(simple.greet(name: 'mini-oc'), 'Hello, mini-oc!');
  });
}
