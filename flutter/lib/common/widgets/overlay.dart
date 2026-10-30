import 'package:flutter/material.dart';
import 'package:flutter_hbb/common.dart';
import 'package:get/get.dart';

import '../../models/model.dart';

class BlockableOverlayState extends OverlayKeyState {
  final _middleBlocked = false.obs;

  VoidCallback? onMiddleBlockedClick; // to-do use listener

  RxBool get middleBlocked => _middleBlocked;

  void addMiddleBlockedListener(void Function(bool) cb) {
    _middleBlocked.listen(cb);
  }

  void setMiddleBlocked(bool blocked) {
    if (blocked != _middleBlocked.value) {
      _middleBlocked.value = blocked;
    }
  }

  void applyFfi(FFI ffi) {
    ffi.dialogManager.setOverlayState(this);
    // make remote page penetrable automatically
    onMiddleBlockedClick = () {
      setMiddleBlocked(false);
    };
  }
}

class BlockableOverlay extends StatelessWidget {
  final Widget underlying;
  final List<OverlayEntry>? upperLayer;

  final BlockableOverlayState state;

  BlockableOverlay(
      {required this.underlying, required this.state, this.upperLayer});

  @override
  Widget build(BuildContext context) {
    final initialEntries = [
      OverlayEntry(builder: (_) => underlying),

      /// middle layer
      OverlayEntry(
          builder: (context) => Obx(() => Listener(
              onPointerDown: (_) {
                state.onMiddleBlockedClick?.call();
              },
              child: Container(
                  color:
                      state.middleBlocked.value ? Colors.transparent : null)))),
    ];

    if (upperLayer != null) {
      initialEntries.addAll(upperLayer!);
    }

    /// set key
    return Overlay(key: state.key, initialEntries: initialEntries);
  }
}
