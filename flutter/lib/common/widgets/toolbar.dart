import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_hbb/common.dart';
import 'package:flutter_hbb/common/widgets/dialog.dart';
import 'package:flutter_hbb/consts.dart';
import 'package:flutter_hbb/models/model.dart';
import 'package:flutter_hbb/models/platform_model.dart';
import 'package:flutter_hbb/models/shortcut_model.dart';

/// Action IDs that `toolbarControls` is the sole registrar for. Wiped on
/// every call so stale closures don't outlive the menu entry that owned
/// them. Actions registered by `registerSessionShortcutActions` MUST NOT
/// appear here.
const _kToolbarOwnedActionIds = <String>[
  kShortcutActionSendCtrlAltDel,
  kShortcutActionRestartRemote,
  kShortcutActionInsertLock,
  kShortcutActionRefresh,
];

const _kToolbarViewStyleActionIds = <String>[
  kShortcutActionViewModeOriginal,
  kShortcutActionViewModeAdaptive,
  kShortcutActionViewModeCustom,
];

const _kToolbarImageQualityActionIds = <String>[
  kShortcutActionImageQualityBest,
  kShortcutActionImageQualityBalanced,
  kShortcutActionImageQualityLow,
];

const _kToolbarDisplayToggleActionIds = <String>[
  kShortcutActionToggleMute,
  kShortcutActionToggleEnableFileCopyPaste,
  kShortcutActionToggleDisableClipboard,
];

class TTextMenu {
  final Widget child;
  final VoidCallback? onPressed;
  Widget? trailingIcon;
  bool divider;
  final String? actionId;
  TTextMenu(
      {required this.child,
      required this.onPressed,
      this.trailingIcon,
      this.divider = false,
      this.actionId});

  Widget getChild() {
    if (trailingIcon != null) {
      return Row(
        mainAxisAlignment: MainAxisAlignment.spaceBetween,
        children: [
          child,
          trailingIcon!,
        ],
      );
    } else {
      return child;
    }
  }
}

class TRadioMenu<T> {
  final Widget child;
  final T value;
  final T groupValue;
  final ValueChanged<T?>? onChanged;
  final String? actionId;

  TRadioMenu(
      {required this.child,
      required this.value,
      required this.groupValue,
      required this.onChanged,
      this.actionId});
}

class TToggleMenu {
  final Widget child;
  final bool value;
  final ValueChanged<bool?>? onChanged;
  final String? actionId;
  TToggleMenu(
      {required this.child,
      required this.value,
      required this.onChanged,
      this.actionId});
}

/// Register each tagged entry's `onChanged` with the session [ShortcutModel].
/// Passthrough — returns [menus] so a caller can wrap `return [...]` directly.
List<TToggleMenu> _registerToggleMenuShortcuts(
  FFI ffi,
  List<TToggleMenu> menus, {
  List<String> ownedActionIds = const [],
}) {
  for (final actionId in ownedActionIds) {
    ffi.shortcutModel.unregister(actionId);
  }
  for (final menu in menus) {
    final actionId = menu.actionId;
    if (actionId == null) continue;
    final onChanged = menu.onChanged;
    if (onChanged == null) {
      ffi.shortcutModel.unregister(actionId);
    } else {
      final value = menu.value;
      ffi.shortcutModel.register(actionId, () => onChanged(!value));
    }
  }
  return menus;
}

/// Radio variant of [_registerToggleMenuShortcuts].
List<TRadioMenu<T>> _registerRadioMenuShortcuts<T>(
  FFI ffi,
  List<TRadioMenu<T>> menus, {
  List<String> ownedActionIds = const [],
}) {
  for (final actionId in ownedActionIds) {
    ffi.shortcutModel.unregister(actionId);
  }
  for (final menu in menus) {
    final actionId = menu.actionId;
    if (actionId == null) continue;
    final onChanged = menu.onChanged;
    if (onChanged == null) {
      ffi.shortcutModel.unregister(actionId);
    } else {
      final value = menu.value;
      ffi.shortcutModel.register(actionId, () => onChanged(value));
    }
  }
  return menus;
}

List<TTextMenu> toolbarControls(BuildContext context, String id, FFI ffi) {
  final ffiModel = ffi.ffiModel;
  final pi = ffiModel.pi;
  final perms = ffiModel.permissions;
  final sessionId = ffi.sessionId;
  final isDefaultConn = ffi.connType == ConnType.defaultConn;

  // Wipe stale registrations from previous menu builds before re-registering
  // below; runs unconditionally so mid-session enable works without reconnect.
  for (final actionId in _kToolbarOwnedActionIds) {
    ffi.shortcutModel.unregister(actionId);
  }

  List<TTextMenu> v = [];
  // elevation
  if (isDefaultConn &&
      perms['keyboard'] != false &&
      ffi.elevationModel.showRequestMenu) {
    v.add(
      TTextMenu(
          child: Text(translate('Request Elevation')),
          onPressed: () =>
              showRequestElevationDialog(sessionId, ffi.dialogManager)),
    );
  }
  // https://github.com/rustdesk/rustdesk/pull/9731
  // Does not work for connection established by "accept".
  connectWithToken({bool isFileTransfer = false}) {
    final connToken = bind.sessionGetConnToken(sessionId: ffi.sessionId);
    connect(context, id, isFileTransfer: isFileTransfer, connToken: connToken);
  }

  if (isDefaultConn) {
    v.add(
      TTextMenu(
          child: Text(translate('Transfer file')),
          onPressed: () => connectWithToken(isFileTransfer: true)),
    );
  }
  // divider
  if (isDefaultConn) {
    v.add(TTextMenu(child: Offstage(), onPressed: () {}, divider: true));
  }
  // ctrlAltDel
  if (isDefaultConn &&
      ffiModel.keyboard &&
      (pi.platform == kPeerPlatformLinux || pi.sasEnabled)) {
    v.add(
      TTextMenu(
          child: Text('${translate("Insert Ctrl + Alt + Del")}'),
          onPressed: () => bind.sessionCtrlAltDel(sessionId: sessionId),
          actionId: kShortcutActionSendCtrlAltDel),
    );
  }
  // restart
  if (isDefaultConn &&
      perms['restart'] != false &&
      (pi.platform == kPeerPlatformLinux ||
          pi.platform == kPeerPlatformWindows ||
          pi.platform == kPeerPlatformMacOS)) {
    v.add(
      TTextMenu(
          child: Text(translate('Restart remote device')),
          onPressed: () =>
              showRestartRemoteDevice(pi, id, sessionId, ffi.dialogManager),
          actionId: kShortcutActionRestartRemote),
    );
  }
  // insertLock
  if (isDefaultConn && ffi.ffiModel.keyboard) {
    v.add(
      TTextMenu(
          child: Text(translate('Insert Lock')),
          onPressed: () => bind.sessionLockScreen(sessionId: sessionId),
          actionId: kShortcutActionInsertLock),
    );
  }
  // refresh
  if (pi.version.isNotEmpty) {
    v.add(TTextMenu(
      child: Text(translate('Refresh')),
      onPressed: () => sessionRefreshVideo(sessionId, pi),
      actionId: kShortcutActionRefresh,
    ));
  }
  // Register tagged TTextMenu callbacks. The else-unregister is defense in
  // depth for actionIds tagged but missing from `_kToolbarOwnedActionIds`.
  for (final menu in v) {
    final actionId = menu.actionId;
    if (actionId == null) continue;
    if (menu.onPressed != null) {
      ffi.shortcutModel.register(actionId, menu.onPressed!);
    } else {
      ffi.shortcutModel.unregister(actionId);
    }
  }
  return v;
}

Future<List<TRadioMenu<String>>> toolbarViewStyle(
    BuildContext context, String id, FFI ffi) async {
  final groupValue =
      await bind.sessionGetViewStyle(sessionId: ffi.sessionId) ?? '';
  void onChanged(String? value) async {
    if (value == null) return;
    bind
        .sessionSetViewStyle(sessionId: ffi.sessionId, value: value)
        .then((_) => ffi.canvasModel.updateViewStyle());
  }

  return _registerRadioMenuShortcuts(ffi, [
    TRadioMenu<String>(
        child: Text(translate('Scale original')),
        value: kRemoteViewStyleOriginal,
        groupValue: groupValue,
        onChanged: onChanged,
        actionId: kShortcutActionViewModeOriginal),
    TRadioMenu<String>(
        child: Text(translate('Scale adaptive')),
        value: kRemoteViewStyleAdaptive,
        groupValue: groupValue,
        onChanged: onChanged,
        actionId: kShortcutActionViewModeAdaptive),
    TRadioMenu<String>(
        child: Text(translate('Scale custom')),
        value: kRemoteViewStyleCustom,
        groupValue: groupValue,
        onChanged: onChanged,
        actionId: kShortcutActionViewModeCustom)
  ], ownedActionIds: _kToolbarViewStyleActionIds);
}

Future<List<TRadioMenu<String>>> toolbarImageQuality(
    BuildContext context, String id, FFI ffi) async {
  final groupValue =
      await bind.sessionGetImageQuality(sessionId: ffi.sessionId) ?? '';
  onChanged(String? value) async {
    if (value == null) return;
    await bind.sessionSetImageQuality(sessionId: ffi.sessionId, value: value);
  }

  return _registerRadioMenuShortcuts(ffi, [
    TRadioMenu<String>(
        child: Text(translate('Good image quality')),
        value: kRemoteImageQualityBest,
        groupValue: groupValue,
        onChanged: onChanged,
        actionId: kShortcutActionImageQualityBest),
    TRadioMenu<String>(
        child: Text(translate('Balanced')),
        value: kRemoteImageQualityBalanced,
        groupValue: groupValue,
        onChanged: onChanged,
        actionId: kShortcutActionImageQualityBalanced),
    TRadioMenu<String>(
        child: Text(translate('Optimize reaction time')),
        value: kRemoteImageQualityLow,
        groupValue: groupValue,
        onChanged: onChanged,
        actionId: kShortcutActionImageQualityLow),
    TRadioMenu<String>(
      child: Text(translate('Custom')),
      value: kRemoteImageQualityCustom,
      groupValue: groupValue,
      onChanged: (value) {
        onChanged(value);
        customImageQualityDialog(ffi.sessionId, id, ffi);
      },
    ),
  ], ownedActionIds: _kToolbarImageQualityActionIds);
}

Future<List<TToggleMenu>> toolbarDisplayToggle(
    BuildContext context, String id, FFI ffi) async {
  List<TToggleMenu> v = [];
  final ffiModel = ffi.ffiModel;
  final pi = ffiModel.pi;
  final perms = ffiModel.permissions;
  final sessionId = ffi.sessionId;
  final isDefaultConn = ffi.connType == ConnType.defaultConn;

  // mute
  if (isDefaultConn && perms['audio'] != false) {
    final option = 'disable-audio';
    final value =
        bind.sessionGetToggleOptionSync(sessionId: sessionId, arg: option);
    v.add(TToggleMenu(
        value: value,
        actionId: kShortcutActionToggleMute,
        onChanged: (value) {
          if (value == null) return;
          bind.sessionToggleOption(sessionId: sessionId, value: option);
        },
        child: Text(translate('Mute'))));
  }
  // file copy and paste
  // If the version is less than 1.2.4, file copy and paste is supported on Windows only.
  final isSupportIfPeer_1_2_3 = versionCmp(pi.version, '1.2.4') < 0 && pi.platform == kPeerPlatformWindows;
  // If the version is 1.2.4 or later, file copy and paste is supported when kPlatformAdditionsHasFileClipboard is set.
  final isSupportIfPeer_1_2_4 = versionCmp(pi.version, '1.2.4') >= 0 &&
      bind.mainHasFileClipboard() &&
      pi.platformAdditions.containsKey(kPlatformAdditionsHasFileClipboard);
  if (isDefaultConn &&
      ffiModel.keyboard &&
      perms['file'] != false &&
      (isSupportIfPeer_1_2_3 || isSupportIfPeer_1_2_4)) {
    final value = bind.sessionGetToggleOptionSync(
        sessionId: sessionId, arg: kOptionEnableFileCopyPaste);
    v.add(TToggleMenu(
        value: value,
        actionId: kShortcutActionToggleEnableFileCopyPaste,
        onChanged: (value) {
          if (value == null) return;
          bind.sessionToggleOption(
              sessionId: sessionId, value: kOptionEnableFileCopyPaste);
        },
        child: Text(translate('Enable file copy and paste'))));
  }
  // disable clipboard
  if (isDefaultConn && ffiModel.keyboard && perms['clipboard'] != false) {
    final option = 'disable-clipboard';
    final value =
        bind.sessionGetToggleOptionSync(sessionId: sessionId, arg: option);
    v.add(TToggleMenu(
        value: value,
        actionId: kShortcutActionToggleDisableClipboard,
        onChanged: (value) {
          if (value == null) return;
          bind.sessionToggleOption(sessionId: sessionId, value: option);
        },
        child: Text(translate('Disable clipboard'))));
  }
  return _registerToggleMenuShortcuts(ffi, v,
      ownedActionIds: _kToolbarDisplayToggleActionIds);
}

/// Drive each toolbar helper for its registration side effect, so a shortcut
/// fires from the first keystroke without needing the user to open the
/// matching submenu.
void registerToolbarShortcuts(BuildContext context, String id, FFI ffi) {
  // Rebuild only the triggered action's menu, so permission/capability changes
  // and temporary disabled states take effect without opening the toolbar.
  void register(List<String> actions, FutureOr<Object> Function() refresh) {
    ffi.shortcutModel.registerRefresher(actions, () async {
      if (!context.mounted || ffi.closed) {
        ffi.shortcutModel.clear();
        return;
      }
      await refresh();
    });
  }

  register(_kToolbarOwnedActionIds, () => toolbarControls(context, id, ffi));
  register(_kToolbarDisplayToggleActionIds,
      () => toolbarDisplayToggle(context, id, ffi));
  register(_kToolbarViewStyleActionIds, () => toolbarViewStyle(context, id, ffi));
  register(_kToolbarImageQualityActionIds,
      () => toolbarImageQuality(context, id, ffi));

  unawaited(toolbarDisplayToggle(context, id, ffi));
  unawaited(toolbarViewStyle(context, id, ffi));
  unawaited(toolbarImageQuality(context, id, ffi));
}
