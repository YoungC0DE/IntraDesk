import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter/gestures.dart';

import 'package:flutter_hbb/models/platform_model.dart';
import 'package:flutter_hbb/common.dart';
import 'package:flutter_hbb/consts.dart';
import 'package:flutter_hbb/models/model.dart';
import 'package:flutter_hbb/models/input_model.dart';

import './gestures.dart';

class RawKeyFocusScope extends StatelessWidget {
  final FocusNode? focusNode;
  final ValueChanged<bool>? onFocusChange;
  final InputModel inputModel;
  final Widget child;

  RawKeyFocusScope({
    this.focusNode,
    this.onFocusChange,
    required this.inputModel,
    required this.child,
  });

  @override
  Widget build(BuildContext context) {
    // https://github.com/flutter/flutter/issues/154053
    // FIXME: On Windows, `AltGr` will generate `Alt` and `Control` key events,
    // while `Alt` and `Control` are separated key events for en-US input method.
    return FocusScope(
        autofocus: true,
        child: Focus(
            autofocus: true,
            canRequestFocus: true,
            focusNode: focusNode,
            onFocusChange: onFocusChange,
            onKey: null,
            onKeyEvent: (FocusNode node, KeyEvent event) =>
                    inputModel.handleKeyEvent(event),
            child: child));
  }
}

class RawTouchGestureDetectorRegion extends StatefulWidget {
  final Widget child;
  final FFI ffi;
  late final InputModel inputModel = ffi.inputModel;
  late final FfiModel ffiModel = ffi.ffiModel;

  RawTouchGestureDetectorRegion({
    required this.child,
    required this.ffi,
  });

  @override
  State<RawTouchGestureDetectorRegion> createState() =>
      _RawTouchGestureDetectorRegionState();
}

/// Touch screens drive the remote cursor directly:
///   LongPress -> right click
///   OneFingerPan -> start/end -> left down start/end
///   onDoubleTapDown -> move to
///   onLongPressDown => move to
///   DoubleFiner -> right click
class _RawTouchGestureDetectorRegionState
    extends State<RawTouchGestureDetectorRegion> {
  Offset _cacheLongPressPosition = Offset(0, 0);
  // Timestamp of the last long press event.
  int _cacheLongPressPositionTs = 0;
  double _mouseScrollIntegral = 0; // mouse scroll speed controller
  double _scale = 1;

  // Workaround tap down event when two fingers are used to scale
  TapDownDetails? _lastTapDownDetails;

  PointerDeviceKind? lastDeviceKind;

  // `onDoubleTap()` does not provide the position of the tap event.
  Offset _lastPosOfDoubleTapDown = Offset.zero;
  bool _touchModePanStarted = false;
  Offset _doubleFinerTapPosition = Offset.zero;

  FFI get ffi => widget.ffi;
  FfiModel get ffiModel => widget.ffiModel;
  InputModel get inputModel => widget.inputModel;
  SessionID get sessionId => ffi.sessionId;

  @override
  Widget build(BuildContext context) {
    return RawGestureDetector(
      child: widget.child,
      gestures: makeGestures(context),
    );
  }

  bool isNotTouchBasedDevice() {
    return !kTouchBasedDeviceKinds.contains(lastDeviceKind);
  }

  onTapDown(TapDownDetails d) async {
    lastDeviceKind = d.kind;
    if (isNotTouchBasedDevice()) {
      return;
    }
    _lastPosOfDoubleTapDown = d.localPosition;
    _lastTapDownDetails = d;
  }

  onTapUp(TapUpDetails d) async {
    final TapDownDetails? lastTapDownDetails = _lastTapDownDetails;
    _lastTapDownDetails = null;
    if (isNotTouchBasedDevice()) {
      return;
    }
    final isMoved =
        await ffi.cursorModel.move(d.localPosition.dx, d.localPosition.dy);
    if (isMoved) {
      // If pan already handled 'down', don't send it again.
      if (lastTapDownDetails != null && !_touchModePanStarted) {
        await inputModel.tapDown(MouseButtons.left);
      }
      await inputModel.tapUp(MouseButtons.left);
    }
  }

  onDoubleTapDown(TapDownDetails d) async {
    lastDeviceKind = d.kind;
    if (isNotTouchBasedDevice()) {
      return;
    }
    _lastPosOfDoubleTapDown = d.localPosition;
    await ffi.cursorModel.move(d.localPosition.dx, d.localPosition.dy);
  }

  onDoubleTap() async {
    if (isNotTouchBasedDevice()) {
      return;
    }
    if (!ffi.cursorModel.isInRemoteRect(_lastPosOfDoubleTapDown)) {
      return;
    }
    await inputModel.tap(MouseButtons.left);
    await inputModel.tap(MouseButtons.left);
  }

  onLongPressDown(LongPressDownDetails d) async {
    lastDeviceKind = d.kind;
    if (isNotTouchBasedDevice()) {
      return;
    }
    _lastPosOfDoubleTapDown = d.localPosition;
    _cacheLongPressPosition = d.localPosition;
    if (!ffi.cursorModel.isInRemoteRect(d.localPosition)) {
      return;
    }
    _cacheLongPressPositionTs = DateTime.now().millisecondsSinceEpoch;
  }

  onLongPressUp() async {
    if (isNotTouchBasedDevice()) {
      return;
    }
    await inputModel.tapUp(MouseButtons.left);
  }

  onLongPress() async {
    if (isNotTouchBasedDevice()) {
      return;
    }
    final isMoved = await ffi.cursorModel
        .move(_cacheLongPressPosition.dx, _cacheLongPressPosition.dy);
    if (!isMoved) {
      return;
    }
    await inputModel.tap(MouseButtons.right);
  }

  onDoubleFinerTapDown(TapDownDetails d) async {
    lastDeviceKind = d.kind;
    if (isNotTouchBasedDevice()) {
      return;
    }
    _doubleFinerTapPosition = d.localPosition;
    // ignore for desktop and mobile
  }

  onDoubleFinerTap(TapDownDetails d) async {
    lastDeviceKind = d.kind;
    if (isNotTouchBasedDevice()) {
      return;
    }

    // We can't use `d.localPosition` here because it's always (0, 0) on desktop.
    final isDesktopInRemoteRect = ffi.cursorModel.isInRemoteRect(_doubleFinerTapPosition);
    if (isDesktopInRemoteRect) {
      await inputModel.tap(MouseButtons.right);
    }
  }

  onOneFingerPanStart(BuildContext context, DragStartDetails d) async {
    final TapDownDetails? lastTapDownDetails = _lastTapDownDetails;
    _lastTapDownDetails = null;
    lastDeviceKind = d.kind ?? lastDeviceKind;
    if (isNotTouchBasedDevice()) {
      return;
    }
    if (lastTapDownDetails != null) {
      await ffi.cursorModel.move(lastTapDownDetails.localPosition.dx,
          lastTapDownDetails.localPosition.dy);
    }
    if (!ffi.cursorModel.isInRemoteRect(d.localPosition)) {
      return;
    }

    _touchModePanStarted = true;
    ffi.cursorModel.trySetRemoteWindowCoords();

    // Workaround for the issue that the first pan event is sent a long time after the start event.
    // If the time interval between the start event and the first pan event is less than 500ms,
    // we consider to use the long press position as the start position.
    //
    // TODO: We should find a better way to send the first pan event as soon as possible.
    if (DateTime.now().millisecondsSinceEpoch - _cacheLongPressPositionTs <
        500) {
      await ffi.cursorModel
          .move(_cacheLongPressPosition.dx, _cacheLongPressPosition.dy);
    }
    // In relative mouse mode, skip mouse down - only send movement via sendMobileRelativeMouseMove
    if (!inputModel.relativeMouseMode.value) {
      await inputModel.sendMouse('down', MouseButtons.left);
    }
    await ffi.cursorModel.move(d.localPosition.dx, d.localPosition.dy);
  }

  onOneFingerPanUpdate(DragUpdateDetails d) async {
    if (isNotTouchBasedDevice()) {
      return;
    }
    if (!_touchModePanStarted) {
      return;
    }
    // In relative mouse mode, send delta directly without position tracking.
    if (inputModel.relativeMouseMode.value) {
      await inputModel.sendMobileRelativeMouseMove(d.delta.dx, d.delta.dy);
    } else {
      await ffi.cursorModel.updatePan(d.delta, d.localPosition);
    }
  }

  onOneFingerPanEnd(DragEndDetails d) async {
    _touchModePanStarted = false;
    if (isNotTouchBasedDevice()) {
      return;
    }
    ffi.cursorModel.clearRemoteWindowCoords();
    // In relative mouse mode, skip mouse up - matches the skipped mouse down in onOneFingerPanStart
    if (!inputModel.relativeMouseMode.value) {
      await inputModel.sendMouse('up', MouseButtons.left);
    }
  }

  // Reset `_touchModePanStarted` if the one-finger pan gesture is cancelled
  // or rejected by the gesture arena. Without this, the flag can remain
  // stuck in the "started" state.
  onOneFingerPanCancel() {
    _touchModePanStarted = false;
  }

  // scale + pan event
  onTwoFingerScaleStart(ScaleStartDetails d) {
    _lastTapDownDetails = null;
  }

  onTwoFingerScaleUpdate(ScaleUpdateDetails d) async {
    if (isNotTouchBasedDevice()) {
      return;
    }

    final scale = ((d.scale - _scale) * 1000).toInt();
    _scale = d.scale;

    if (scale != 0) {
      await bind.sessionSendPointer(
          sessionId: sessionId,
          msg: json.encode(
              PointerEventToRust(kPointerEventKindTouch, 'scale', scale)
                  .toJson()));
    }
  }

  onTwoFingerScaleEnd(ScaleEndDetails d) async {
    if (isNotTouchBasedDevice()) {
      return;
    }
    await bind.sessionSendPointer(
        sessionId: sessionId,
        msg: json.encode(
            PointerEventToRust(kPointerEventKindTouch, 'scale', 0).toJson()));
    await inputModel.sendMouse('up', MouseButtons.left);
  }

  onThreeFingerVerticalDragUpdate(DragUpdateDetails d) {
    _mouseScrollIntegral += d.delta.dy / 4;
    if (_mouseScrollIntegral > 1) {
      inputModel.scroll(1);
      _mouseScrollIntegral = 0;
    } else if (_mouseScrollIntegral < -1) {
      inputModel.scroll(-1);
      _mouseScrollIntegral = 0;
    }
  }

  makeGestures(BuildContext context) {
    return <Type, GestureRecognizerFactory>{
      // Official
      TapGestureRecognizer:
          GestureRecognizerFactoryWithHandlers<TapGestureRecognizer>(
              () => TapGestureRecognizer(), (instance) {
        instance
          ..onTapDown = onTapDown
          ..onTapUp = onTapUp;
      }),
      DoubleTapGestureRecognizer:
          GestureRecognizerFactoryWithHandlers<DoubleTapGestureRecognizer>(
              () => DoubleTapGestureRecognizer(), (instance) {
        instance
          ..onDoubleTapDown = onDoubleTapDown
          ..onDoubleTap = onDoubleTap;
      }),
      LongPressGestureRecognizer:
          GestureRecognizerFactoryWithHandlers<LongPressGestureRecognizer>(
              () => LongPressGestureRecognizer(), (instance) {
        instance
          ..onLongPressDown = onLongPressDown
          ..onLongPressUp = onLongPressUp
          ..onLongPress = onLongPress;
      }),
      // Customized
      DoubleFinerTapGestureRecognizer:
          GestureRecognizerFactoryWithHandlers<DoubleFinerTapGestureRecognizer>(
              () => DoubleFinerTapGestureRecognizer(), (instance) {
        instance
          ..onDoubleFinerTap = onDoubleFinerTap
          ..onDoubleFinerTapDown = onDoubleFinerTapDown;
      }),
      CustomTouchGestureRecognizer:
          GestureRecognizerFactoryWithHandlers<CustomTouchGestureRecognizer>(
              () => CustomTouchGestureRecognizer(), (instance) {
        instance.onOneFingerPanStart =
            (DragStartDetails d) => onOneFingerPanStart(context, d);
        instance
          ..onOneFingerPanUpdate = onOneFingerPanUpdate
          ..onOneFingerPanEnd = onOneFingerPanEnd
          ..onOneFingerPanCancel = onOneFingerPanCancel
          ..onTwoFingerScaleStart = onTwoFingerScaleStart
          ..onTwoFingerScaleUpdate = onTwoFingerScaleUpdate
          ..onTwoFingerScaleEnd = onTwoFingerScaleEnd
          ..onThreeFingerVerticalDragUpdate = onThreeFingerVerticalDragUpdate;
      }),
    };
  }
}

class RawPointerMouseRegion extends StatelessWidget {
  final InputModel inputModel;
  final Widget child;
  final MouseCursor? cursor;
  final PointerEnterEventListener? onEnter;
  final PointerExitEventListener? onExit;
  final PointerDownEventListener? onPointerDown;
  final PointerUpEventListener? onPointerUp;

  RawPointerMouseRegion({
    this.onEnter,
    this.onExit,
    this.cursor,
    this.onPointerDown,
    this.onPointerUp,
    required this.inputModel,
    required this.child,
  });

  @override
  Widget build(BuildContext context) {
    return Listener(
      onPointerHover: inputModel.onPointHoverImage,
      onPointerDown: (evt) {
        onPointerDown?.call(evt);
        inputModel.onPointDownImage(evt);
      },
      onPointerUp: (evt) {
        onPointerUp?.call(evt);
        inputModel.onPointUpImage(evt);
      },
      onPointerMove: inputModel.onPointMoveImage,
      onPointerSignal: inputModel.onPointerSignalImage,
      onPointerPanZoomStart: inputModel.onPointerPanZoomStart,
      onPointerPanZoomUpdate: inputModel.onPointerPanZoomUpdate,
      onPointerPanZoomEnd: inputModel.onPointerPanZoomEnd,
      child: MouseRegion(
        cursor: cursor ?? MouseCursor.defer,
        onEnter: onEnter,
        onExit: onExit,
        child: child,
      ),
    );
  }
}
