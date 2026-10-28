import 'dart:convert';

import 'package:desktop_multi_window/desktop_multi_window.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_hbb/consts.dart';
import 'package:flutter_hbb/common.dart';
import 'package:flutter_hbb/main.dart';
import 'package:flutter_hbb/models/input_model.dart';

/// must keep the order
// ignore: constant_identifier_names
enum WindowType {
  Main,
  RemoteDesktop,
  FileTransfer,
  Unknown
}

extension Index on int {
  WindowType get windowType {
    switch (this) {
      case 0:
        return WindowType.Main;
      case 1:
        return WindowType.RemoteDesktop;
      case 2:
        return WindowType.FileTransfer;
      default:
        return WindowType.Unknown;
    }
  }
}

class MultiWindowCallResult {
  int windowId;
  dynamic result;

  MultiWindowCallResult(this.windowId, this.result);
}

/// Window Manager
/// mainly use it in `Main Window`
/// use it in sub window is not recommended
class RustDeskMultiWindowManager {
  RustDeskMultiWindowManager._();

  static final instance = RustDeskMultiWindowManager._();

  final Set<int> _inactiveWindows = {};
  final Set<int> _activeWindows = {};
  final List<AsyncCallback> _windowActiveCallbacks = List.empty(growable: true);
  final List<int> _remoteDesktopWindows = List.empty(growable: true);
  final List<int> _fileTransferWindows = List.empty(growable: true);

  Future<int> newSessionWindow(
    WindowType type,
    String remoteId,
    String msg,
    List<int> windows,
  ) async {
    final windowController = await DesktopMultiWindow.createWindow(msg);
    windowController.setInitBackgroundColor(Colors.black);
    final windowId = windowController.windowId;
    windowController
      ..setFrame(const Offset(0, 0) &
          Size(1280 + windowId * 20, 720 + windowId * 20))
      ..center()
      ..setTitle(getWindowNameWithId(
        remoteId,
        overrideType: type,
      ));
    registerActiveWindow(windowId);
    windows.add(windowId);
    return windowId;
  }

  // Sessions open as tabs: a new window only when none of this type exists.
  Future<MultiWindowCallResult> _newSession(
    WindowType type,
    String methodName,
    String remoteId,
    List<int> windows,
    String msg,
  ) async {
    if (windows.isEmpty) {
      final windowId = await newSessionWindow(type, remoteId, msg, windows);
      return MultiWindowCallResult(windowId, null);
    }
    return call(type, methodName, msg);
  }

  Future<MultiWindowCallResult> newSession(
    WindowType type,
    String methodName,
    String remoteId,
    List<int> windows, {
    String? password,
    String? connToken,
  }) async {
    var params = {
      "type": type.index,
      "id": remoteId,
      "password": password,
    };
    if (connToken != null) {
      params['connToken'] = connToken;
    }
    final msg = jsonEncode(params);

    if (windows.length > 1) {
      for (final windowId in windows) {
        if (await DesktopMultiWindow.invokeMethod(
            windowId, kWindowEventActiveSession, remoteId)) {
          return MultiWindowCallResult(windowId, null);
        }
      }
    }

    return _newSession(type, methodName, remoteId, windows, msg);
  }

  Future<MultiWindowCallResult> newRemoteDesktop(
    String remoteId, {
    String? password,
  }) async {
    return await newSession(
      WindowType.RemoteDesktop,
      kWindowEventNewRemoteDesktop,
      remoteId,
      _remoteDesktopWindows,
      password: password,
    );
  }

  Future<MultiWindowCallResult> newFileTransfer(
    String remoteId, {
    String? password,
    String? connToken,
  }) async {
    return await newSession(
      WindowType.FileTransfer,
      kWindowEventNewFileTransfer,
      remoteId,
      _fileTransferWindows,
      password: password,
      connToken: connToken,
    );
  }

  Future<MultiWindowCallResult> call(
      WindowType type, String methodName, dynamic args) async {
    final wnds = _findWindowsByType(type);
    if (wnds.isEmpty) {
      return MultiWindowCallResult(kInvalidWindowId, null);
    }
    for (final windowId in wnds) {
      if (_activeWindows.contains(windowId)) {
        final res =
            await DesktopMultiWindow.invokeMethod(windowId, methodName, args);
        return MultiWindowCallResult(windowId, res);
      }
    }
    final res =
        await DesktopMultiWindow.invokeMethod(wnds[0], methodName, args);
    return MultiWindowCallResult(wnds[0], res);
  }

  List<int> _findWindowsByType(WindowType type) {
    switch (type) {
      case WindowType.Main:
        return [kMainWindowId];
      case WindowType.RemoteDesktop:
        return _remoteDesktopWindows;
      case WindowType.FileTransfer:
        return _fileTransferWindows;
      case WindowType.Unknown:
        break;
    }
    return [];
  }

  void clearWindowType(WindowType type) {
    switch (type) {
      case WindowType.Main:
        return;
      case WindowType.RemoteDesktop:
        _remoteDesktopWindows.clear();
        break;
      case WindowType.FileTransfer:
        _fileTransferWindows.clear();
        break;
      case WindowType.Unknown:
        break;
    }
  }

  void setMethodHandler(
      Future<dynamic> Function(MethodCall call, int fromWindowId)? handler) {
    DesktopMultiWindow.setMethodHandler(handler);
  }

  Future<void> closeAllSubWindows() async {
    await Future.wait(WindowType.values.map((e) => _closeWindows(e)));
  }

  Future<void> _closeWindows(WindowType type) async {
    if (type == WindowType.Main) {
      // skip main window, use window manager instead
      return;
    }

    List<int> windows = [];
    try {
      windows = _findWindowsByType(type);
    } catch (e) {
      debugPrint('Failed to getAllSubWindowIds of $type, $e');
      return;
    }

    if (windows.isEmpty) {
      return;
    }
    for (int i = 0; i < windows.length; i++) {
      final wId = windows[i];
      debugPrint("closing multi window, type: ${type.toString()} id: $wId");
      try {
        await saveWindowPosition(type, windowId: wId);
      } catch (e) {
        debugPrint('Failed to save window position of $wId, $e');
      }
      try {
        await WindowController.fromWindowId(wId).setPreventClose(false);
        await WindowController.fromWindowId(wId).close();
        _activeWindows.remove(wId);
      } catch (e) {
        debugPrint("$e");
        return;
      }
    }
    clearWindowType(type);
  }

  Future<List<int>> getAllSubWindowIds() async {
    try {
      final windows = await DesktopMultiWindow.getAllSubWindowIds();
      return windows;
    } catch (err) {
      if (err is AssertionError) {
        return [];
      } else {
        rethrow;
      }
    }
  }

  Set<int> getActiveWindows() {
    return _activeWindows;
  }

  Future<void> _notifyActiveWindow() async {
    for (final callback in _windowActiveCallbacks) {
      await callback.call();
    }
  }

  Future<void> registerActiveWindow(int windowId) async {
    _activeWindows.add(windowId);
    _inactiveWindows.remove(windowId);
    await _notifyActiveWindow();
  }

  /// Remove active window which has [`windowId`]
  ///
  /// [Availability]
  /// This function should only be called from main window.
  /// For other windows, please post a unregister(hide) event to main window handler:
  /// `rustDeskWinManager.call(WindowType.Main, kWindowEventHide, {"id": windowId!});`
  Future<void> unregisterActiveWindow(int windowId) async {
    _activeWindows.remove(windowId);
    if (windowId != kMainWindowId) {
      _inactiveWindows.add(windowId);
    }
    await _notifyActiveWindow();
  }

  void registerActiveWindowListener(AsyncCallback callback) {
    _windowActiveCallbacks.add(callback);
  }

  void unregisterActiveWindowListener(AsyncCallback callback) {
    _windowActiveCallbacks.remove(callback);
  }

  // This function is called from the main window.
  // It will query the active remote windows to get their coords.
  Future<List<String>> getOtherRemoteWindowCoords(int wId) async {
    List<String> coords = [];
    for (final windowId in _remoteDesktopWindows) {
      if (windowId != wId) {
        if (_activeWindows.contains(windowId)) {
          final res = await DesktopMultiWindow.invokeMethod(
              windowId, kWindowEventRemoteWindowCoords, '');
          if (res != null) {
            coords.add(res);
          }
        }
      }
    }
    return coords;
  }

  // This function is called from one remote window.
  // Only the main window knows `_remoteDesktopWindows` and `_activeWindows`.
  // So we need to call the main window to get the other remote windows' coords.
  Future<List<RemoteWindowCoords>> getOtherRemoteWindowCoordsFromMain() async {
    List<RemoteWindowCoords> coords = [];
    // Call the main window to get the coords of other remote windows.
    String res = await DesktopMultiWindow.invokeMethod(
        kMainWindowId, kWindowEventRemoteWindowCoords, kWindowId.toString());
    List<dynamic> list = jsonDecode(res);
    for (var item in list) {
      coords.add(RemoteWindowCoords.fromJson(jsonDecode(item)));
    }
    return coords;
  }
}

final rustDeskWinManager = RustDeskMultiWindowManager.instance;
