import 'dart:convert';
import 'dart:ffi';
import 'dart:io';

import 'package:device_info_plus/device_info_plus.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter_hbb/consts.dart';
import 'package:flutter_hbb/main.dart';
import 'package:package_info_plus/package_info_plus.dart';
import 'package:path_provider/path_provider.dart';

import '../common.dart';
import '../generated_bridge.dart';

final class RgbaFrame extends Struct {
  @Uint32()
  external int len;
  external Pointer<Uint8> data;
}

typedef HandleEvent = Future<void> Function(Map<String, dynamic> evt);

/// FFI wrapper around the native Rust core.
class PlatformFFI {
  String _dir = '';
  final _eventHandlers = <String, Map<String, HandleEvent>>{};
  late RustdeskImpl _ffiBind;
  late String _appType;
  StreamEventHandler? _eventCallback;

  PlatformFFI._();

  static final PlatformFFI instance = PlatformFFI._();

  RustdeskImpl get ffiBind => _ffiBind;
  @visibleForTesting
  void setBindingForTesting(RustdeskImpl binding) => _ffiBind = binding;

  static get localeName => Platform.localeName;

  static get isMain => instance._appType == kAppTypeMain;

  static Future<String> getVersion() async {
    PackageInfo packageInfo = await PackageInfo.fromPlatform();
    return packageInfo.version;
  }

  bool registerEventHandler(
      String eventName, String handlerName, HandleEvent handler, {bool replace = false}) {
    debugPrint('registerEventHandler $eventName $handlerName');
    var handlers = _eventHandlers[eventName];
    if (handlers == null) {
      _eventHandlers[eventName] = {handlerName: handler};
      return true;
    } else {
      if (!replace && handlers.containsKey(handlerName)) {
        return false;
      } else {
        handlers[handlerName] = handler;
        return true;
      }
    }
  }

  void unregisterEventHandler(String eventName, String handlerName) {
    debugPrint('unregisterEventHandler $eventName $handlerName');
    var handlers = _eventHandlers[eventName];
    if (handlers != null) {
      handlers.remove(handlerName);
    }
  }

  String translate(String name, String locale) =>
      _ffiBind.translate(name: name, locale: locale);

  void registerPixelbufferTexture(SessionID sessionId, int display, int ptr) =>
      _ffiBind.sessionRegisterPixelbufferTexture(
          sessionId: sessionId, display: display, ptr: ptr);

  /// Init the FFI class, loads the native Rust core library.
  Future<void> init(String appType) async {
    _appType = appType;
    final dylib = DynamicLibrary.open('librustdesk.dll');
    debugPrint('initializing FFI $_appType');
    try {
      try {
        // SYSTEM user failed
        _dir = (await getApplicationDocumentsDirectory()).path;
      } catch (e) {
        debugPrint('Failed to get documents directory: $e');
      }
      _ffiBind = RustdeskImpl(dylib);

      _startListenEvent(_ffiBind); // global event
      String id = 'NA';
      String name = 'Flutter';
      DeviceInfoPlugin deviceInfo = DeviceInfoPlugin();
      try {
        // request windows build number to fix overflow on win7
        windowsBuildNumber = getWindowsTargetBuildNumber();
        WindowsDeviceInfo winInfo = await deviceInfo.windowsInfo;
        name = winInfo.computerName;
        id = winInfo.computerName;
      } catch (e) {
        debugPrintStack(label: "get windows device info failed: $e");
        name = "unknown";
        id = "unknown";
      }
      debugPrint(
          '_appType:$_appType,info1-id:$id,info2-name:$name,dir:$_dir');
      if (desktopType == DesktopType.cm) {
        await _ffiBind.cmInit();
      }
      await _ffiBind.mainDeviceId(id: id);
      await _ffiBind.mainDeviceName(name: name);
      await _ffiBind.mainInit(
        appDir: _dir,
        customClientConfig: '',
      );
    } catch (e) {
      debugPrintStack(label: 'initialize failed: $e');
    }
    version = await getVersion();
  }

  Future<bool> tryHandle(Map<String, dynamic> evt) async {
    final name = evt['name'];
    if (name != null) {
      final handlers = _eventHandlers[name];
      if (handlers != null) {
        if (handlers.isNotEmpty) {
          for (var handler in handlers.values) {
            await handler(evt);
          }
          return true;
        }
      }
    }
    return false;
  }

  /// Start listening to the Rust core's events and frames.
  void _startListenEvent(RustdeskImpl rustdeskImpl) {
    final appType =
        _appType == kAppTypeDesktopRemote ? '$_appType,$kWindowId' : _appType;
    var sink = rustdeskImpl.startGlobalEventStream(appType: appType);
    sink.listen((message) {
      () async {
        try {
          Map<String, dynamic> event = json.decode(message);
          // _tryHandle here may be more flexible than _eventCallback
          if (!await tryHandle(event)) {
            if (_eventCallback != null) {
              await _eventCallback!(event);
            }
          }
        } catch (e) {
          debugPrint('json.decode fail(): $e');
        }
      }();
    });
  }

  void setEventCallback(StreamEventHandler fun) async {
    _eventCallback = fun;
  }
}
