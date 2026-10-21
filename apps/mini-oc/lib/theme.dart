import 'package:flutter/material.dart';

/// mini-oc 主题：FlClash 式 Material 3——动态种子色、亮暗跟随、连续圆角。
class AppTheme {
  static const seed = Color(0xFF4F6EF7);

  static ThemeData light() => _base(ColorScheme.fromSeed(seedColor: seed));

  static ThemeData dark() => _base(ColorScheme.fromSeed(
        seedColor: seed,
        brightness: Brightness.dark,
      ));

  static ThemeData _base(ColorScheme scheme) {
    final radiusLarge = BorderRadius.circular(20);
    final radiusMedium = BorderRadius.circular(14);
    return ThemeData(
      useMaterial3: true,
      colorScheme: scheme,
      scaffoldBackgroundColor: scheme.surface,
      navigationBarTheme: NavigationBarThemeData(
        backgroundColor: scheme.surface,
        indicatorColor: scheme.primaryContainer,
        labelTextStyle: WidgetStatePropertyAll(
          TextStyle(fontSize: 11, color: scheme.onSurface),
        ),
      ),
      navigationRailTheme: NavigationRailThemeData(
        backgroundColor: scheme.surface,
        indicatorColor: scheme.primaryContainer,
        selectedIconTheme: IconThemeData(color: scheme.onPrimaryContainer),
        selectedLabelTextStyle: TextStyle(
          color: scheme.onPrimaryContainer,
          fontWeight: FontWeight.w600,
        ),
      ),
      cardTheme: CardThemeData(
        color: scheme.surfaceContainerLow,
        elevation: 0,
        shape: RoundedRectangleBorder(borderRadius: radiusLarge),
        margin: EdgeInsets.zero,
      ),
      filledButtonTheme: FilledButtonThemeData(
        style: FilledButton.styleFrom(
          shape: RoundedRectangleBorder(borderRadius: radiusMedium),
          padding: const EdgeInsets.symmetric(horizontal: 20, vertical: 14),
        ),
      ),
      inputDecorationTheme: InputDecorationTheme(
        filled: true,
        fillColor: scheme.surfaceContainerHighest,
        border: OutlineInputBorder(
          borderRadius: radiusMedium,
          borderSide: BorderSide.none,
        ),
        focusedBorder: OutlineInputBorder(
          borderRadius: radiusMedium,
          borderSide: BorderSide(color: scheme.primary, width: 1.6),
        ),
        contentPadding: const EdgeInsets.symmetric(horizontal: 16, vertical: 14),
      ),
      listTileTheme: ListTileThemeData(shape: RoundedRectangleBorder(borderRadius: radiusMedium)),
      snackBarTheme: SnackBarThemeData(
        behavior: SnackBarBehavior.floating,
        shape: RoundedRectangleBorder(borderRadius: radiusMedium),
      ),
      pageTransitionsTheme: const PageTransitionsTheme(
        builders: {
          TargetPlatform.macOS: FadeUpwardsPageTransitionsBuilder(),
          TargetPlatform.windows: FadeUpwardsPageTransitionsBuilder(),
        },
      ),
    );
  }
}
