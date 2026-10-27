import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:flutter_hbb/main.dart';

enum SystemWindowTheme { light, dark }

/// The platform channel for RustDesk.
class RdPlatformChannel {
  RdPlatformChannel._();

  static final RdPlatformChannel _windowUtil = RdPlatformChannel._();

  static RdPlatformChannel get instance => _windowUtil;

  final MethodChannel _hostMethodChannel =
      MethodChannel("org.rustdesk.rustdesk/host");

  /// Change the theme of the system window
  Future<void> changeSystemWindowTheme(SystemWindowTheme theme) {
    assert(false);
    if (kDebugMode) {
      print(
          "[Window ${kWindowId ?? 'Main'}] change system window theme to ${theme.name}");
    }
    return _hostMethodChannel
        .invokeMethod("setWindowTheme", {"themeName": theme.name});
  }

  /// Terminate .app manually.
  Future<void> terminate() {
    assert(false);
    return _hostMethodChannel.invokeMethod("terminate");
  }
}
