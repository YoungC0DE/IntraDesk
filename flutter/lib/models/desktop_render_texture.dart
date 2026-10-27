import 'package:flutter/material.dart';
import 'package:flutter_hbb/consts.dart';
import 'package:flutter_hbb/models/model.dart';
import 'package:get/get.dart';

import '../../common.dart';
import './platform_model.dart';

import 'package:texture_rgba_renderer/texture_rgba_renderer.dart'
    if (dart.library.html) 'package:flutter_hbb/web/texture_rgba_renderer.dart';

class _PixelbufferTexture {
  int _textureKey = -1;
  int _display = 0;
  SessionID? _sessionId;
  bool _destroying = false;
  int? _id;

  final textureRenderer = TextureRgbaRenderer();

  int get display => _display;

  create(int d, SessionID sessionId, FFI ffi) {
    _display = d;
    _textureKey = bind.getNextTextureKey();
    _sessionId = sessionId;

    textureRenderer.createTexture(_textureKey).then((id) async {
      _id = id;
      if (id != -1) {
        ffi.textureModel.setRgbaTextureId(display: d, id: id);
        final ptr = await textureRenderer.getTexturePtr(_textureKey);
        platformFFI.registerPixelbufferTexture(sessionId, display, ptr);
        debugPrint(
            "create pixelbuffer texture: peerId: ${ffi.id} display:$_display, textureId:$id, texturePtr:$ptr");
      }
    });
  }

  destroy(bool unregisterTexture, FFI ffi) async {
    if (!_destroying && _textureKey != -1 && _sessionId != null) {
      _destroying = true;
      if (unregisterTexture) {
        platformFFI.registerPixelbufferTexture(_sessionId!, display, 0);
        // sleep for a while to avoid the texture is used after it's unregistered.
        await Future.delayed(Duration(milliseconds: 100));
      }
      await textureRenderer.closeTexture(_textureKey);
      _textureKey = -1;
      _destroying = false;
      debugPrint(
          "destroy pixelbuffer texture: peerId: ${ffi.id} display:$_display, textureId:$_id");
    }
  }
}

class _Control {
  RxInt textureID = (-1).obs;
}

class TextureModel {
  final WeakReference<FFI> parent;
  final Map<int, _Control> _control = {};
  final Map<int, _PixelbufferTexture> _pixelbufferRenderTextures = {};

  TextureModel(this.parent);

  setRgbaTextureId({required int display, required int id}) {
    ensureControl(display);
    _control[display]?.textureID.value = id;
  }

  RxInt getTextureId(int display) {
    ensureControl(display);
    return _control[display]!.textureID;
  }

  updateCurrentDisplay(int curDisplay) {
    final ffi = parent.target;
    if (ffi == null) return;
    tryCreateTexture(int idx) {
      if (!_pixelbufferRenderTextures.containsKey(idx)) {
        final renderTexture = _PixelbufferTexture();
        _pixelbufferRenderTextures[idx] = renderTexture;
        renderTexture.create(idx, ffi.sessionId, ffi);
      }
    }

    tryRemoveTexture(int idx) {
      _control.remove(idx);
      if (_pixelbufferRenderTextures.containsKey(idx)) {
        _pixelbufferRenderTextures[idx]!.destroy(true, ffi);
        _pixelbufferRenderTextures.remove(idx);
      }
    }

    if (curDisplay == kAllDisplayValue) {
      final displays = ffi.ffiModel.pi.getCurDisplays();
      for (var i = 0; i < displays.length; i++) {
        tryCreateTexture(i);
      }
    } else {
      tryCreateTexture(curDisplay);
      for (var i = 0; i < ffi.ffiModel.pi.displays.length; i++) {
        if (i != curDisplay) {
          tryRemoveTexture(i);
        }
      }
    }
  }

  onRemotePageDispose(bool closeSession) async {
    final ffi = parent.target;
    if (ffi == null) return;
    for (final texture in _pixelbufferRenderTextures.values) {
      await texture.destroy(closeSession, ffi);
    }
  }

  ensureControl(int display) {
    var ctl = _control[display];
    if (ctl == null) {
      ctl = _Control();
      _control[display] = ctl;
    }
  }
}
