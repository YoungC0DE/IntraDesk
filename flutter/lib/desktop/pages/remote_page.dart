import 'dart:async';

import 'package:desktop_multi_window/desktop_multi_window.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter/scheduler.dart';
import 'package:get/get.dart';
import 'package:provider/provider.dart';
import 'package:flutter_hbb/models/state_model.dart';

import '../../consts.dart';
import '../../common/widgets/overlay.dart';
import '../../common/widgets/remote_input.dart';
import '../../common.dart';
import '../../common/widgets/dialog.dart';
import '../../common/widgets/toolbar.dart';
import '../../models/model.dart';
import '../../models/input_model.dart';
import '../../models/platform_model.dart';
import '../../models/shortcut_model.dart';
import '../../common/shared_state.dart';
import '../widgets/remote_toolbar.dart';
import '../widgets/tabbar_widget.dart';
import 'macos_full_screen_focus_recovery.dart';

import 'package:flutter_hbb/native/custom_cursor.dart';

final SimpleWrapper<bool> _firstEnterImage = SimpleWrapper(false);

// Used to skip session close if "move to new window" is clicked.
final Map<String, bool> closeSessionOnDispose = {};

class RemotePage extends StatefulWidget {
  RemotePage({
    Key? key,
    required this.id,
    required this.toolbarState,
    this.sessionId,
    this.tabWindowId,
    this.password,
    this.display,
    this.displays,
    this.tabController,
  }) : super(key: key) {
    initSharedStates(id);
  }

  final String id;
  final SessionID? sessionId;
  final int? tabWindowId;
  final int? display;
  final List<int>? displays;
  final String? password;
  final ToolbarState toolbarState;
  final SimpleWrapper<State<RemotePage>?> _lastState = SimpleWrapper(null);
  final DesktopTabController? tabController;

  FFI get ffi => (_lastState.value! as _RemotePageState)._ffi;

  void releaseMacOSInputForTabTransfer() {
    return;
  }

  @override
  State<RemotePage> createState() {
    final state = _RemotePageState(id);
    _lastState.value = state;
    return state;
  }
}

class _RemotePageState extends State<RemotePage>
    with
        AutomaticKeepAliveClientMixin,
        MultiWindowListener,
        WidgetsBindingObserver,
        TickerProviderStateMixin {
  Timer? _timer;
  String keyboardMode = "legacy";
  bool _isWindowBlur = false;
  // Known macOS remote-input trade-offs (kept simple intentionally):
  // 1. Dialogs rely on FocusNode loss plus middleBlocked, not mirrored dialog
  //    state. Reproduce: activate remote input, open a dialog, then type.
  // 2. Delayed fullscreen recovery can race a local-control focus change; no
  //    owner state is added. Reproduce: focus the toolbar during a Space switch.
  // 3. Input-source switching releases native input without updating this
  //    page's cache. Reproduce: switch sources, then type before and after
  //    clicking the remote image; the click reasserts input.
  // These latches compensate for out-of-order macOS focus events. Treat them
  // as coupled when changing a transition or _syncMacOSKeyboardGrab().
  AppLifecycleState? _macOSLifecycleState;
  bool _macOSInputActive = false;
  final _macOSFullScreenFocusRecovery = MacOSFullScreenFocusRecovery();
  bool _macOSExplicitFocusRequestPending = false;
  final _cursorOverImage = false.obs;
  late RxBool _remoteCursorMoved;
  late RxBool _keyboardEnabled;
  final _uniqueKey = UniqueKey();

  var _blockableOverlayState = BlockableOverlayState();

  final FocusNode _rawKeyFocusNode = FocusNode(debugLabel: "rawkeyFocusNode");

  // Debounce timer for pointer lock center updates during window events.
  // Uses kDefaultPointerLockCenterThrottleMs from consts.dart for the duration.
  Timer? _pointerLockCenterDebounceTimer;

  // We need `_instanceIdOnEnterOrLeaveImage4Toolbar` together with `_onEnterOrLeaveImage4Toolbar`
  // to identify the toolbar instance and its callback function.
  int? _instanceIdOnEnterOrLeaveImage4Toolbar;
  Function(bool)? _onEnterOrLeaveImage4Toolbar;

  late FFI _ffi;

  SessionID get sessionId => _ffi.sessionId;

  _RemotePageState(String id) {
    _initStates(id);
  }

  void _initStates(String id) {
    _keyboardEnabled = KeyboardEnabledState.find(id);
    _remoteCursorMoved = RemoteCursorMovedState.find(id);
  }

  @override
  void initState() {
    super.initState();
    _ffi = FFI(widget.sessionId);
    Get.put<FFI>(_ffi, tag: widget.id);
    _ffi.imageModel.addCallbackOnFirstImage((String peerId) {
      _ffi.canvasModel.activateLocalCursor();
      // Seed shortcut action callbacks once the session is ready, so that
      // global keyboard shortcuts work even if the user never opens the
      // toolbar menu. The returned list is intentionally discarded — the
      // side effect of registering callbacks (inside toolbarControls) is
      // what we want here.
      if (mounted) {
        toolbarControls(context, widget.id, _ffi);
        registerSessionShortcutActions(_ffi,
            tabController: widget.tabController,
            toolbarState: widget.toolbarState);
        registerToolbarShortcuts(context, widget.id, _ffi);
      }
    });
    _ffi.start(
      widget.id,
      password: widget.password,
      tabWindowId: widget.tabWindowId,
      display: widget.display,
      displays: widget.displays,
    );
    WidgetsBinding.instance.addPostFrameCallback((_) {
      _ffi.dialogManager
          .showLoading(translate('Connecting...'), onCancel: closeConnection);
    });
    WakelockManager.enable(_uniqueKey);

    _ffi.ffiModel.updateEventListener(sessionId, widget.id);
    DesktopMultiWindow.addListener(this);
    // if (!_isCustomCursorInited) {
    //   customCursorController.registerNeedUpdateCursorCallback(
    //       (String? lastKey, String? currentKey) async {
    //     if (_firstEnterImage.value) {
    //       _firstEnterImage.value = false;
    //       return true;
    //     }
    //     return lastKey == null || lastKey != currentKey;
    //   });
    //   _isCustomCursorInited = true;
    // }

    _blockableOverlayState.applyFfi(_ffi);
    // Call onSelected in post frame callback, since we cannot guarantee that the callback will not call setState.
    WidgetsBinding.instance.addPostFrameCallback((_) {
      widget.tabController?.onSelected?.call(widget.id);
    });

    // Register callback to cancel debounce timer when relative mouse mode is disabled
    _ffi.inputModel.onRelativeMouseModeDisabled =
        _cancelPointerLockCenterDebounceTimer;
  }

  /// Cancel the pointer lock center debounce timer
  void _cancelPointerLockCenterDebounceTimer() {
    _pointerLockCenterDebounceTimer?.cancel();
    _pointerLockCenterDebounceTimer = null;
  }

  bool get _isSelectedTab {
    final controller = widget.tabController;
    if (controller == null) return true;
    final tabState = controller.state.value;
    final selected = tabState.selected;
    return selected >= 0 &&
        selected < tabState.tabs.length &&
        tabState.tabs[selected].key == widget.id;
  }

  // Every Windows requestFocus() must pass this, or a blocking dialog or an
  // inactive tab could hand remote input to this page.
  bool get _windowsCanFocusRemoteInput =>
      _isSelectedTab && _blockableOverlayState.middleBlocked.isFalse;

  bool get _isMacOSKeyboardContextActive {
    return stateGlobal.isFocused.value && !_isWindowBlur && _isSelectedTab;
  }

  void _onMacOSTabStateChanged(DesktopTabState _) {
    if (!_isSelectedTab) {
      _macOSFullScreenFocusRecovery.cancel();
      _syncMacOSKeyboardGrab();
      return;
    }
    // Tab listeners run synchronously. Defer the selected page so the previous
    // page releases first; a late leave from it can disable the new session.
    scheduleMicrotask(() {
      if (mounted) {
        _syncMacOSKeyboardGrab(reassert: true);
      }
    });
  }

  void _releaseMacOSRemoteInput() {
    _macOSFullScreenFocusRecovery.cancel();
    _macOSExplicitFocusRequestPending = false;
    _ffi.inputModel.enterOrLeave(false);
    _macOSInputActive = false;
    _rawKeyFocusNode.unfocus();
  }

  void _onMacOSFocusChange() {
    // requestFocus() notifies later; only a recorded explicit request may clear
    // the local-focus-loss latch.
    if (_rawKeyFocusNode.hasPrimaryFocus) {
      final explicitRequest = _macOSExplicitFocusRequestPending;
      _macOSExplicitFocusRequestPending = false;
      if (explicitRequest && _isMacOSKeyboardContextActive) {
      }
      _syncMacOSKeyboardGrab(allowInactiveLifecycle: explicitRequest);
    } else {
      if (_macOSInputActive) {
        _ffi.inputModel.enterOrLeave(false);
        _macOSInputActive = false;
      }
      if (_isMacOSKeyboardContextActive) {
      }
    }
  }

  // 1. Sync the keyboard grab state with the current context.
  // 2. Call enterOrLeave() to update the input state in the FFI layer.
  // 3. Request or unfocus the raw key focus node based on the current context.
  // Flutter focus and native input are separate; native input activates only
  // after the FocusNode has primary focus.
  void _syncMacOSKeyboardGrab({
    bool reassert = false,
    bool allowInactiveLifecycle = false,
  }) {
    return;
  }

  void _restoreMacOSKeyboardAfterFullScreen({
    required int generation,
    bool allowHiddenLifecycle = false,
  }) {
    // Fullscreen callbacks preserve recovery while hidden. Native window focus
    // may bypass a stale hidden lifecycle for the newly visible Space.
    if (!_macOSFullScreenFocusRecovery.isCurrent(generation) ||
        (!allowHiddenLifecycle &&
            _macOSLifecycleState == AppLifecycleState.hidden)) {
      return;
    }
    final contextActive =
        stateGlobal.isFocused.value && !_isWindowBlur && _isSelectedTab;
    // macOS can focus a fullscreen Space without sending PointerEnter. Native
    // window focus is authoritative here; a later blur cancels this generation
    // before an off-screen window can restore input.
    final shouldInferPointerInside = !_cursorOverImage.value &&
        allowHiddenLifecycle &&
        stateGlobal.fullscreen.isTrue &&
        contextActive;
    final canRestore = contextActive &&
        _blockableOverlayState.middleBlocked.isFalse &&
        (_cursorOverImage.value || shouldInferPointerInside);
    if (!_macOSFullScreenFocusRecovery.consume(generation)) return;
    if (!canRestore) {
      // Consuming recovery here requires a later pointer/window/tab event.
      return;
    }
    if (shouldInferPointerInside) {
      _cursorOverImage.value = true;
    }
    stateGlobal.getInputSource(force: true);
    _syncMacOSKeyboardGrab(reassert: true, allowInactiveLifecycle: true);
  }

  void _scheduleMacOSKeyboardAfterFullScreen({
    required int generation,
    bool allowHiddenLifecycle = false,
  }) {
    // Fullscreen can deliver FocusNode loss after its callback; wait for frame
    // completion and then advance one event-loop turn before restoring.
    WidgetsBinding.instance.addPostFrameCallback((_) {
      Timer.run(() {
        if (mounted) {
          _restoreMacOSKeyboardAfterFullScreen(
            generation: generation,
            allowHiddenLifecycle: allowHiddenLifecycle,
          );
        }
      });
    });
    WidgetsBinding.instance.ensureVisualUpdate();
  }

  void _queueMacOSKeyboardAfterFullScreen({
    bool allowHiddenLifecycle = false,
  }) {
    final generation = _macOSFullScreenFocusRecovery.queue();
    if (_macOSLifecycleState == AppLifecycleState.paused ||
        _macOSLifecycleState == AppLifecycleState.detached) {
      _macOSFullScreenFocusRecovery.cancel();
      return;
    }
    _scheduleMacOSKeyboardAfterFullScreen(
      generation: generation,
      allowHiddenLifecycle: allowHiddenLifecycle,
    );
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    super.didChangeAppLifecycleState(state);
    return;
  }

  @override
  void onWindowBlur() {
    super.onWindowBlur();
    // On windows, we use `focus` way to handle keyboard better.
    // Now on Linux, there's some rdev issues which will break the input.
    // We disable the `focus` way for Linux temporarily.
    _isWindowBlur = true;
    // unfocus the primary-focus when the whole window is lost focus,
    // and let OS to handle events instead.
    _rawKeyFocusNode.unfocus();
    stateGlobal.isFocused.value = false;
    _syncMacOSKeyboardGrab();

    // When window loses focus, temporarily release relative mouse mode constraints
    // to allow user to interact with other applications normally.
    // The cursor will be re-hidden and re-centered when window regains focus.
    if (_ffi.inputModel.relativeMouseMode.value) {
      _ffi.inputModel.onWindowBlur();
    }
  }

  @override
  void onWindowFocus() {
    super.onWindowFocus();
    // See [onWindowBlur].
    _isWindowBlur = false;
    stateGlobal.isFocused.value = true;

    // Normal macOS windows wait for PointerEnter or PointerDown. A focused
    // fullscreen Space queues delayed recovery; if this window blurs again, the
    // pending recovery is cancelled before native input can reactivate.
    // Regression: switch directly between fullscreen remote Spaces without
    // moving or clicking; only the newly focused session may receive input.

    // Refocus without PointerEnter: the cursor already hovers the image when
    // focus returns (Alt+Tab, taskbar), so enterView() never fires again.
    if (_cursorOverImage.value && _windowsCanFocusRemoteInput && !_rawKeyFocusNode.hasFocus) {
      _rawKeyFocusNode.requestFocus();
    }

    // Restore relative mouse mode constraints when window regains focus.
    if (_ffi.inputModel.relativeMouseMode.value) {
      if (_windowsCanFocusRemoteInput) {
        _rawKeyFocusNode.requestFocus();
      }
      _ffi.inputModel.onWindowFocus();
    }
    _syncMacOSKeyboardGrab(reassert: true, allowInactiveLifecycle: true);
  }

  @override
  void onWindowRestore() {
    super.onWindowRestore();
    // On windows, we use `onWindowRestore` way to handle window restore from
    // a minimized state.
    _isWindowBlur = false;
    WakelockManager.enable(_uniqueKey);
    // Update pointer lock center when window is restored
    _updatePointerLockCenterIfNeeded();
  }

  // When the window is unminimized, onWindowMaximize or onWindowRestore can be called when the old state was maximized or not.
  @override
  void onWindowMaximize() {
    super.onWindowMaximize();
    WakelockManager.enable(_uniqueKey);
    // Update pointer lock center when window is maximized
    _updatePointerLockCenterIfNeeded();
  }

  @override
  void onWindowResize() {
    super.onWindowResize();
    // Update pointer lock center when window is resized
    _updatePointerLockCenterIfNeeded();
  }

  @override
  void onWindowMove() {
    super.onWindowMove();
    // Update pointer lock center when window is moved
    _updatePointerLockCenterIfNeeded();
  }

  /// Update pointer lock center with debouncing to avoid excessive updates
  /// during rapid window move/resize events.
  void _updatePointerLockCenterIfNeeded() {
    if (!_ffi.inputModel.relativeMouseMode.value) return;

    // Cancel any pending update and schedule a new one (debounce pattern)
    _pointerLockCenterDebounceTimer?.cancel();
    _pointerLockCenterDebounceTimer = Timer(
      const Duration(milliseconds: kDefaultPointerLockCenterThrottleMs),
      () {
        if (!mounted) return;
        if (_ffi.inputModel.relativeMouseMode.value) {
          _ffi.inputModel.updatePointerLockCenter();
        }
      },
    );
  }

  @override
  void onWindowMinimize() {
    super.onWindowMinimize();
    WakelockManager.disable(_uniqueKey);
    // Release cursor constraints when minimized
    if (_ffi.inputModel.relativeMouseMode.value) {
      _ffi.inputModel.onWindowBlur();
    }
  }

  @override
  void onWindowEnterFullScreen() {
    super.onWindowEnterFullScreen();
  }

  @override
  void onWindowLeaveFullScreen() {
    super.onWindowLeaveFullScreen();
  }

  @override
  Future<void> dispose() async {
    final closeSession = closeSessionOnDispose.remove(widget.id) ?? true;

    // https://github.com/flutter/flutter/issues/64935
    super.dispose();
    debugPrint("REMOTE PAGE dispose session $sessionId ${widget.id}");

    // Defensive cleanup: ensure host system-key propagation is reset even if
    // MouseRegion.onExit never fired (e.g., tab closed while cursor inside).
    bind.hostStopSystemKeyPropagate(stopped: true);

    _pointerLockCenterDebounceTimer?.cancel();
    _pointerLockCenterDebounceTimer = null;
    // Clear callback reference to prevent memory leaks and stale references
    _ffi.inputModel.onRelativeMouseModeDisabled = null;
    // Relative mouse mode cleanup is centralized in FFI.close(closeSession: ...).
    _ffi.textureModel.onRemotePageDispose(closeSession);
    if (closeSession) {
      // ensure we leave this session, this is a double check
      // enterOrLeave() is already called previously in _releaseMacOSRemoteInput() for macOS.
      _ffi.inputModel.enterOrLeave(false);
    }
    DesktopMultiWindow.removeListener(this);
    _ffi.cursorModel.disposeImages();
    _rawKeyFocusNode.dispose();
    await _ffi.close(closeSession: closeSession);
    _timer?.cancel();
    _ffi.dialogManager.dismissAll();
    WakelockManager.disable(_uniqueKey);
    await Get.delete<FFI>(tag: widget.id);
    removeSharedStates(widget.id);
  }

  Widget emptyOverlay() => BlockableOverlay(
        /// the Overlay key will be set with _blockableOverlayState in BlockableOverlay
        /// see override build() in [BlockableOverlay]
        state: _blockableOverlayState,
        underlying: Container(
          color: Colors.transparent,
        ),
      );

  Widget buildBody(BuildContext context) {
    remoteToolbar(BuildContext context) => RemoteToolbar(
          id: widget.id,
          ffi: _ffi,
          state: widget.toolbarState,
          onEnterOrLeaveImageSetter: (id, func) {
            _instanceIdOnEnterOrLeaveImage4Toolbar = id;
            _onEnterOrLeaveImage4Toolbar = func;
          },
          onEnterOrLeaveImageCleaner: (id) {
            // If _instanceIdOnEnterOrLeaveImage4Toolbar != id
            // it means `_onEnterOrLeaveImage4Toolbar` is not set or it has been changed to another toolbar.
            if (_instanceIdOnEnterOrLeaveImage4Toolbar == id) {
              _instanceIdOnEnterOrLeaveImage4Toolbar = null;
              _onEnterOrLeaveImage4Toolbar = null;
            }
          },
          setRemoteState: setState,
        );

    bodyWidget() {
      return Stack(
        children: [
          Container(
              color: kColorCanvas,
              child: RawKeyFocusScope(
                  focusNode: _rawKeyFocusNode,
                  onFocusChange: (bool imageFocused) {
                    debugPrint(
                        "onFocusChange(window active:${!_isWindowBlur}) $imageFocused");
                    // See [onWindowBlur].
                    if (_isWindowBlur) {
                      imageFocused = false;
                      Future.delayed(Duration.zero, () {
                        _rawKeyFocusNode.unfocus();
                      });
                    }
                    if (imageFocused) {
                      _ffi.inputModel.enterOrLeave(true);
                    } else {
                      _ffi.inputModel.enterOrLeave(false);
                    }
                  },
                  inputModel: _ffi.inputModel,
                  child: getBodyForDesktop(context))),
          Stack(
            children: [
              _ffi.ffiModel.pi.isSet.isTrue &&
                      _ffi.ffiModel.waitForFirstImage.isTrue
                  ? emptyOverlay()
                  : Offstage(),
              // Use Overlay to enable rebuild every time on menu button click.
              // Hide toolbar when relative mouse mode is active to prevent
              // cursor from escaping to toolbar area.
              Obx(() => _ffi.inputModel.relativeMouseMode.value
                  ? const Offstage()
                  : _ffi.ffiModel.pi.isSet.isTrue
                      ? Overlay(initialEntries: [
                          OverlayEntry(builder: remoteToolbar)
                        ])
                      : remoteToolbar(context)),
              _ffi.ffiModel.pi.isSet.isFalse ? emptyOverlay() : Offstage(),
            ],
          ),
        ],
      );
    }

    return Scaffold(
      backgroundColor: Theme.of(context).colorScheme.background,
      body: Obx(() {
        final imageReady = _ffi.ffiModel.pi.isSet.isTrue &&
            _ffi.ffiModel.waitForFirstImage.isFalse;
        if (imageReady) {
          // `dismissAll()` is to ensure that the state is clean.
          // It's ok to call dismissAll() here.
          _ffi.dialogManager.dismissAll();
          // Recreate the block state to refresh the state.
          _blockableOverlayState = BlockableOverlayState();
          _blockableOverlayState.applyFfi(_ffi);
          // Block the whole `bodyWidget()` when dialog shows.
          return BlockableOverlay(
            underlying: bodyWidget(),
            state: _blockableOverlayState,
          );
        } else {
          // `_blockableOverlayState` is not recreated here.
          // The toolbar's block state won't work properly when reconnecting, but that's okay.
          return bodyWidget();
        }
      }),
    );
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    return WillPopScope(
        onWillPop: () async {
          clientClose(sessionId, _ffi);
          return false;
        },
        child: MultiProvider(providers: [
          ChangeNotifierProvider.value(value: _ffi.ffiModel),
          ChangeNotifierProvider.value(value: _ffi.cursorModel),
          ChangeNotifierProvider.value(value: _ffi.canvasModel),
        ], child: buildBody(context)));
  }

  void enterView(PointerEnterEvent evt) {
    _cursorOverImage.value = true;
    _firstEnterImage.value = true;
    if (_onEnterOrLeaveImage4Toolbar != null) {
      try {
        _onEnterOrLeaveImage4Toolbar!(true);
      } catch (e) {
        //
      }
    }

    // See [onWindowBlur].
    // Blur unfocuses this node and nothing restores it, so the keyboard stayed
    // dead until a click. Focus only while the window is really active, or a
    // background window would grab system keys. onFocusChange does enterOrLeave.
    if (!_isWindowBlur &&
        _windowsCanFocusRemoteInput &&
        !_rawKeyFocusNode.hasFocus) {
      _rawKeyFocusNode.requestFocus();
    }
  }

  void leaveView(PointerExitEvent evt) {

    if (_ffi.ffiModel.keyboard) {
      _ffi.inputModel.tryMoveEdgeOnExit(evt.position);
    }

    _cursorOverImage.value = false;
    _firstEnterImage.value = false;
    if (_onEnterOrLeaveImage4Toolbar != null) {
      try {
        _onEnterOrLeaveImage4Toolbar!(false);
      } catch (e) {
        //
      }
    }

    // See [onWindowBlur].
  }

  Widget _buildRawTouchAndPointerRegion(
    Widget child,
    PointerEnterEventListener? onEnter,
    PointerExitEventListener? onExit,
  ) {
    return RawTouchGestureDetectorRegion(
      child: _buildRawPointerMouseRegion(child, onEnter, onExit),
      ffi: _ffi,
    );
  }

  Widget _buildRawPointerMouseRegion(
    Widget child,
    PointerEnterEventListener? onEnter,
    PointerExitEventListener? onExit,
  ) {
    return RawPointerMouseRegion(
      onEnter: onEnter,
      onExit: onExit,
      onPointerDown: (event) {
        // A double check for blur status on Windows and macOS.
        // Note: If there's an `onPointerDown` event is triggered, `_isWindowBlur` is expected being false.
        // Sometimes the system does not send the necessary focus event to flutter. We should manually
        // handle this inconsistent status by setting `_isWindowBlur` to false. So we can
        // ensure the grab-key thread is running when our users are clicking the remote canvas.
        if (_isWindowBlur) {
          debugPrint(
              "Unexpected status: onPointerDown is triggered while the remote window is in blur status");
          _isWindowBlur = false;
        }
        if (!_rawKeyFocusNode.hasFocus) {
          _rawKeyFocusNode.requestFocus();
        }
      },
      inputModel: _ffi.inputModel,
      child: child,
    );
  }

  Widget getBodyForDesktop(BuildContext context) {
    var paints = <Widget>[
      MouseRegion(
        onEnter: (evt) {
          bind.hostStopSystemKeyPropagate(stopped: false);
        },
        onExit: (evt) {
          bind.hostStopSystemKeyPropagate(stopped: true);
        },
        child: _ViewStyleUpdater(
          canvasModel: _ffi.canvasModel,
          inputModel: _ffi.inputModel,
          child: Builder(builder: (context) {
            final peerDisplay = CurrentDisplayState.find(widget.id);
            return Obx(
              () => _ffi.ffiModel.pi.isSet.isFalse
                  ? Container(color: Colors.transparent)
                  : Obx(() {
                      _ffi.textureModel.updateCurrentDisplay(peerDisplay.value);
                      return ImagePaint(
                        id: widget.id,
                        cursorOverImage: _cursorOverImage,
                        keyboardEnabled: _keyboardEnabled,
                        remoteCursorMoved: _remoteCursorMoved,
                        listenerBuilder: (child) =>
                            _buildRawTouchAndPointerRegion(
                                child, enterView, leaveView),
                        ffi: _ffi,
                      );
                    }),
            );
          }),
        ),
      )
    ];

    return Stack(
      children: paints,
    );
  }

  @override
  bool get wantKeepAlive => true;
}

/// Tracks view size and DPR to update CanvasModel.updateViewStyle()
/// and InputModel.updateImageWidgetSize() only when either changes.
/// This avoids scheduling post-frame callbacks on every LayoutBuilder rebuild.
class _ViewStyleUpdater extends StatefulWidget {
  final CanvasModel canvasModel;
  final InputModel inputModel;
  final Widget child;

  const _ViewStyleUpdater({
    Key? key,
    required this.canvasModel,
    required this.inputModel,
    required this.child,
  }) : super(key: key);

  @override
  State<_ViewStyleUpdater> createState() => _ViewStyleUpdaterState();
}

class _ViewStyleUpdaterState extends State<_ViewStyleUpdater> {
  Size? _lastSize;
  double? _lastDevicePixelRatio;
  bool _callbackScheduled = false;

  @override
  Widget build(BuildContext context) {
    final devicePixelRatio = MediaQuery.devicePixelRatioOf(context);
    return LayoutBuilder(
      builder: (context, constraints) {
        final maxWidth = constraints.maxWidth;
        final maxHeight = constraints.maxHeight;
        // Guard against infinite constraints (e.g., unconstrained ancestor).
        if (!maxWidth.isFinite || !maxHeight.isFinite) {
          return widget.child;
        }
        final newSize = Size(maxWidth, maxHeight);
        if (_lastSize != newSize || _lastDevicePixelRatio != devicePixelRatio) {
          _lastSize = newSize;
          _lastDevicePixelRatio = devicePixelRatio;
          // Schedule the update for after the current frame to avoid setState during build.
          // Use _callbackScheduled flag to prevent accumulating multiple callbacks
          // when size changes rapidly before any callback executes.
          if (!_callbackScheduled) {
            _callbackScheduled = true;
            SchedulerBinding.instance.addPostFrameCallback((_) {
              _callbackScheduled = false;
              final currentSize = _lastSize;
              if (mounted && currentSize != null) {
                widget.canvasModel.updateViewStyle();
                widget.inputModel.updateImageWidgetSize(currentSize);
              }
            });
          }
        }
        return widget.child;
      },
    );
  }
}

class ImagePaint extends StatefulWidget {
  final FFI ffi;
  final String id;
  final RxBool cursorOverImage;
  final RxBool keyboardEnabled;
  final RxBool remoteCursorMoved;
  final Widget Function(Widget)? listenerBuilder;

  ImagePaint(
      {Key? key,
      required this.ffi,
      required this.id,
      required this.cursorOverImage,
      required this.keyboardEnabled,
      required this.remoteCursorMoved,
      this.listenerBuilder})
      : super(key: key);

  @override
  State<StatefulWidget> createState() => _ImagePaintState();
}

class _ImagePaintState extends State<ImagePaint> {
  bool _lastRemoteCursorMoved = false;
  final _cursorDisplayScale = 1.0.obs;

  String get id => widget.id;
  RxBool get cursorOverImage => widget.cursorOverImage;
  RxBool get keyboardEnabled => widget.keyboardEnabled;
  RxBool get remoteCursorMoved => widget.remoteCursorMoved;
  Widget Function(Widget)? get listenerBuilder => widget.listenerBuilder;

  @override
  Widget build(BuildContext context) {
    var c = Provider.of<CanvasModel>(context);
    final s = c.scale;
    // Read the live DPR while the canvas's post-frame update is pending.
    final dpr = MediaQuery.devicePixelRatioOf(context);

    bool isViewOriginal() => c.viewStyle.style == kRemoteViewStyleOriginal;

    mouseRegion({child}) => Obx(() {
          double getCursorScale() {
            var c = Provider.of<CanvasModel>(context);
            return _getDesktopCursorScale(c, dpr);
          }

          return MouseRegion(
              cursor: cursorOverImage.isTrue
                  ? c.cursorEmbedded
                      ? SystemMouseCursors.none
                      // Hide cursor when relative mouse mode is active
                      : widget.ffi.inputModel.relativeMouseMode.value
                          ? SystemMouseCursors.none
                          : keyboardEnabled.isTrue
                              ? (() {
                                  if (remoteCursorMoved.isTrue) {
                                    _lastRemoteCursorMoved = true;
                                    return SystemMouseCursors.none;
                                  } else {
                                    if (_lastRemoteCursorMoved) {
                                      _lastRemoteCursorMoved = false;
                                      _firstEnterImage.value = true;
                                    }
                                    return _buildCustomCursor(
                                        context, getCursorScale());
                                  }
                                }())
                              : _buildDisabledCursor(context, getCursorScale())
                  : MouseCursor.defer,
              onHover: (evt) {},
              child: child);
        });
    if (c.size.width > 0 && c.size.height > 0) {
      final paintWidget = _BuildPaintTextureRender(
          c, s, Offset(c.x, c.y), c.size, isViewOriginal());
      return mouseRegion(child: _buildListener(paintWidget));
    } else {
      return Container();
    }
  }

  /// Matches desktop cursors to the rendered image and native pixel units.
  ///
  /// Windows cursor pixels are physical; NSCursor/GdkCursor use logical pixels.
  /// The cursor is never zoomed with the canvas; Adaptive/Custom views
  /// preserve macOS cursors' point dimensions.
  double _getDesktopCursorScale(CanvasModel c, double dpr) {
    final peer = widget.ffi.ffiModel;
    if (peer.pi.platform == kPeerPlatformMacOS &&
        (c.viewStyle.style == kRemoteViewStyleAdaptive ||
            c.viewStyle.style == kRemoteViewStyleCustom)) {
      return dpr;
    }
    if (peer.isPeerLinux && peer.pi.currentDisplay == kAllDisplayValue) {
      // Remove the host output's density without applying canvas zoom.
      return 1.0 / _cursorDisplayScale.value;
    }
    return _getCursorScaleForDisplay(1.0);
  }

  double _getCursorScaleForDisplay(double scale) {
    final peer = widget.ffi.ffiModel;
    // All Displays can mix densities; no single display scale applies.
    if (peer.pi.currentDisplay == kAllDisplayValue) return scale;
    final displays = peer.pi.getCurDisplays();
    if (displays.isEmpty) return scale;
    if (peer.pi.platform == kPeerPlatformMacOS) {
      // macOS sends NSImage.size in points (src/platform/macos.rs:640,682),
      // while HiDPI screen frames use backing pixels (libs/scrap/src/quartz/display.rs:37).
      return scale * displays.first.scale;
    }
    if (!peer.isPeerLinux) return scale;
    // Match rendering when a Wayland host
    // with multiple outputs reports a display scale > 1.
    // A single-output host keeps scale at 1.0 to preserve physical uinput
    // coordinates, even with OS scaling enabled. This counts host outputs,
    // not the selected displays.length.
    // See src/server/display_service.rs:650 (update_sync_displays) and
    // src/server/drm_capturer.rs:1523 (DRM's matching convention).
    return scale / displays.first.scale;
  }

  Widget _BuildPaintTextureRender(
      CanvasModel c, double s, Offset offset, Size size, bool isViewOriginal) {
    final ffiModel = c.parent.target!.ffiModel;
    final displays = ffiModel.pi.getCurDisplays();
    final children = <Widget>[];
    final rect = ffiModel.rect;
    if (rect == null) {
      return Container();
    }
    final isPeerLinux = ffiModel.isPeerLinux;
    final curDisplay = ffiModel.pi.currentDisplay;
    for (var i = 0; i < displays.length; i++) {
      final textureId = widget.ffi.textureModel
          .getTextureId(curDisplay == kAllDisplayValue ? i : curDisplay);
      if (true) {
        // both "textureId.value != -1" and "true" seems ok
        final sizeScale = isPeerLinux ? s / displays[i].scale : s;
        children.add(Positioned(
          left: (displays[i].x - rect.left) * s + offset.dx,
          top: (displays[i].y - rect.top) * s + offset.dy,
          width: displays[i].width * sizeScale,
          height: displays[i].height * sizeScale,
          child: _trackCursorDisplay(
            Obx(() => Texture(
                  textureId: textureId.value,
                  filterQuality:
                      isViewOriginal ? FilterQuality.none : FilterQuality.low,
                )),
            displays[i],
          ),
        ));
      }
    }
    return SizedBox(
      width: size.width,
      height: size.height,
      child: Stack(children: children),
    );
  }

  Widget _trackCursorDisplay(Widget child, Display display) {
    final peer = widget.ffi.ffiModel;
    if (!peer.isPeerLinux || peer.pi.currentDisplay != kAllDisplayValue) {
      return child;
    }
    return MouseRegion(
      onEnter: (_) => _cursorDisplayScale.value = display.scale,
      onHover: (_) => _cursorDisplayScale.value = display.scale,
      child: child,
    );
  }

  MouseCursor _buildCustomCursor(BuildContext context, double scale) {
    final cursor = Provider.of<CursorModel>(context);
    final cache = cursor.cache ?? preDefaultCursor.cache;
    return buildCursorOfCache(cursor, scale, cache);
  }

  MouseCursor _buildDisabledCursor(BuildContext context, double scale) {
    final cursor = Provider.of<CursorModel>(context);
    final cache = preForbiddenCursor.cache;
    return buildCursorOfCache(cursor, scale, cache);
  }

  Widget _buildListener(Widget child) {
    if (listenerBuilder != null) {
      return listenerBuilder!(child);
    } else {
      return child;
    }
  }
}

