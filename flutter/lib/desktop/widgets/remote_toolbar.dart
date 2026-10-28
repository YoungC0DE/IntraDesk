import 'dart:convert';
import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_hbb/common/widgets/keyboard_shortcuts/display.dart';
import 'package:flutter_hbb/common/widgets/toolbar.dart';
import 'package:flutter_hbb/models/state_model.dart';
import 'package:flutter_hbb/consts.dart';
import 'package:flutter_svg/flutter_svg.dart';
import 'package:get/get.dart';
import 'package:debounce_throttle/debounce_throttle.dart';
import 'package:desktop_multi_window/desktop_multi_window.dart';
import 'package:window_size/window_size.dart' as window_size;

import '../../common.dart';
import '../../models/model.dart';
import '../../models/platform_model.dart';
import '../../common/shared_state.dart';
import './popup_menu.dart';
import 'package:flutter_hbb/utils/scale.dart';
import 'package:flutter_hbb/common/widgets/custom_scale_base.dart';

// Per peer: where along the top edge the toolbar sits (0 = left, 1 = right).
const _kRemoteMenubarDragX = 'remote-menubar-drag-x';

const _toolbarFallbackSize = Size(360, 40);

double _clampToolbarFraction(double fraction, double left, double right) {
  if (fraction < left) fraction = left;
  if (fraction > right) fraction = right;
  return fraction;
}

double _fractionForAlignedDrag({
  required double cursor,
  required double grabOffset,
  required double parentExtent,
  required double toolbarExtent,
  required double left,
  required double right,
}) {
  final travelExtent = parentExtent - toolbarExtent;
  if (travelExtent <= 0) {
    return _clampToolbarFraction(0.5, left, right);
  }
  return _clampToolbarFraction(
      (cursor - grabOffset) / travelExtent, left, right);
}

// Alignment uses [-1, 1] coordinates (0 = center).
Alignment _toolbarAlignment(double fraction) => Alignment(fraction * 2 - 1, -1);

// Positions of toolbars already shown, so a rebuilt toolbar (e.g. after a tab
// switch) appears in place without waiting for the saved option.
final _toolbarFractionBySession = <String, double>{};

class ToolbarState {
  late RxBool _pin;

  RxBool collapse = false.obs;
  RxBool hide = false.obs;

  // Track initialization state to prevent flickering
  final RxBool initialized = false.obs;
  bool _isInitializing = false;

  ToolbarState() {
    _pin = RxBool(false);
    final s = bind.getLocalFlutterOption(k: kOptionRemoteMenubarState);
    if (s.isEmpty) {
      return;
    }

    try {
      final m = jsonDecode(s);
      if (m != null) {
        _pin = RxBool(m['pin'] ?? false);
      }
    } catch (e) {
      debugPrint('Failed to decode toolbar state ${e.toString()}');
    }
  }

  bool get pin => _pin.value;

  /// Initialize all toolbar states from session options.
  /// This should be called once when the toolbar is first created.
  Future<void> init(SessionID sessionId) async {
    if (initialized.value || _isInitializing) return;
    _isInitializing = true;

    try {
      // Load both states in parallel for better performance
      final results = await Future.wait([
        bind.sessionGetToggleOption(
            sessionId: sessionId, arg: kOptionCollapseToolbar),
        bind.sessionGetToggleOption(
            sessionId: sessionId, arg: kOptionHideToolbar),
      ]);

      collapse.value = results[0] ?? false;
      hide.value = results[1] ?? false;
    } finally {
      _isInitializing = false;
      initialized.value = true;
    }
  }

  switchCollapse(SessionID sessionId) async {
    bind.sessionToggleOption(
        sessionId: sessionId, value: kOptionCollapseToolbar);
    collapse.value = !collapse.value;
  }

  // Switch hide state for entire toolbar visibility
  switchHide(SessionID sessionId) async {
    bind.sessionToggleOption(sessionId: sessionId, value: kOptionHideToolbar);
    hide.value = !hide.value;
  }

  switchPin() async {
    _pin.value = !_pin.value;
    // Save everytime changed, as this func will not be called frequently
    await _savePin();
  }

  setPin(bool v) async {
    if (_pin.value != v) {
      _pin.value = v;
      // Save everytime changed, as this func will not be called frequently
      await _savePin();
    }
  }

  _savePin() async {
    bind.setLocalFlutterOption(
        k: kOptionRemoteMenubarState, v: jsonEncode({'pin': _pin.value}));
  }
}

class _ToolbarTheme {
  static Color accentColor(BuildContext context) =>
      MyTheme.color(context).accent;
  static Color hoverAccentColor(BuildContext context) =>
      MyTheme.color(context).accentHover;
  static Color inactiveColor = Colors.grey[800]!;
  static Color hoverInactiveColor = Colors.grey[850]!;

  static const Color redColor = Colors.redAccent;
  static const Color hoverRedColor = Colors.red;
  // kMinInteractiveDimension
  static const double height = 20.0;

  static const double buttonSize = 32;
  static const double buttonHMargin = 2;
  static const double buttonVMargin = 6;
  static const double iconRadius = 8;
  static const double elevation = 3;

  static double dividerSpaceToAction = 8;

  static double menuBorderRadius = 5.0;
  static EdgeInsets menuPadding = EdgeInsets.fromLTRB(4, 12, 4, 12);
  static const double menuButtonBorderRadius = 3.0;

  static Color borderColor(BuildContext context) =>
      MyTheme.color(context).border3 ?? MyTheme.border;

  static Color? dividerColor(BuildContext context) =>
      MyTheme.color(context).divider;

  static MenuStyle defaultMenuStyle(BuildContext context) => MenuStyle(
        side: MaterialStateProperty.all(BorderSide(
          width: 1,
          color: borderColor(context),
        )),
        shape: MaterialStatePropertyAll(RoundedRectangleBorder(
            borderRadius:
                BorderRadius.circular(_ToolbarTheme.menuBorderRadius))),
        padding: MaterialStateProperty.all(_ToolbarTheme.menuPadding),
      );
  static final defaultMenuButtonStyle = ButtonStyle(
    backgroundColor: MaterialStatePropertyAll(Colors.transparent),
    padding: MaterialStatePropertyAll(EdgeInsets.zero),
    overlayColor: MaterialStatePropertyAll(Colors.transparent),
  );

  static Widget borderWrapper(
      BuildContext context, Widget child, BorderRadius borderRadius) {
    return Container(
      decoration: BoxDecoration(
        border: Border.all(
          color: borderColor(context),
          width: 1,
        ),
        borderRadius: borderRadius,
      ),
      child: child,
    );
  }
}

typedef DismissFunc = void Function();

class RemoteMenuEntry {
  static MenuEntryButton<String> insertLock(
    SessionID sessionId,
    EdgeInsets? padding, {
    DismissFunc? dismissFunc,
    DismissCallback? dismissCallback,
  }) {
    return MenuEntryButton<String>(
      childBuilder: (TextStyle? style) => Text(
        translate('Insert Lock'),
        style: style,
      ),
      proc: () {
        bind.sessionLockScreen(sessionId: sessionId);
        if (dismissFunc != null) {
          dismissFunc();
        }
      },
      padding: padding,
      dismissOnClicked: true,
      dismissCallback: dismissCallback,
    );
  }

  static insertCtrlAltDel(
    SessionID sessionId,
    EdgeInsets? padding, {
    DismissFunc? dismissFunc,
    DismissCallback? dismissCallback,
  }) {
    return MenuEntryButton<String>(
      childBuilder: (TextStyle? style) => Text(
        translate("Insert Ctrl + Alt + Del"),
        style: style,
      ),
      proc: () {
        bind.sessionCtrlAltDel(sessionId: sessionId);
        if (dismissFunc != null) {
          dismissFunc();
        }
      },
      padding: padding,
      dismissOnClicked: true,
      dismissCallback: dismissCallback,
    );
  }
}

class RemoteToolbar extends StatefulWidget {
  final String id;
  final FFI ffi;
  final ToolbarState state;
  final Function(int, Function(bool)) onEnterOrLeaveImageSetter;
  final Function(int) onEnterOrLeaveImageCleaner;
  final Function(VoidCallback) setRemoteState;

  RemoteToolbar({
    Key? key,
    required this.id,
    required this.ffi,
    required this.state,
    required this.onEnterOrLeaveImageSetter,
    required this.onEnterOrLeaveImageCleaner,
    required this.setRemoteState,
  }) : super(key: key);

  @override
  State<RemoteToolbar> createState() => _RemoteToolbarState();
}

class _RemoteToolbarState extends State<RemoteToolbar> {
  late Debouncer<int> _debouncerHide;
  bool _isCursorOverImage = false;
  final _fraction = 0.5.obs;
  final _fractionLoaded = false.obs;
  final _dragging = false.obs;
  // Live drag preview: where the toolbar would sit if the user dropped now.
  final _previewFraction = Rxn<double>();
  // Measured size of the live toolbar, so the preview ghost matches reality
  // (collapsed handle vs expanded toolbar). Updated after every layout pass.
  final _toolbarSize = Rxn<Size>();
  final _toolbarKey = GlobalKey(debugLabel: 'remote_toolbar_root');

  int get windowId => stateGlobal.windowId;

  void _setFullscreen(bool v) {
    stateGlobal.setFullscreen(v);
    // stateGlobal.fullscreen is RxBool now, no need to call setState.
    // setState(() {});
  }

  RxBool get collapse => widget.state.collapse;
  RxBool get hide => widget.state.hide;
  bool get pin => widget.state.pin;

  PeerInfo get pi => widget.ffi.ffiModel.pi;
  FfiModel get ffiModel => widget.ffi.ffiModel;

  triggerAutoHide() => _debouncerHide.value = _debouncerHide.value + 1;

  void _minimize() async =>
      await WindowController.fromWindowId(windowId).minimize();

  Future<void> _loadFraction() async {
    // The peer options are only loaded once the peer info arrives.
    if (pi.isSet.isFalse) return;
    final saved = await bind.sessionGetOption(
        sessionId: widget.ffi.sessionId, arg: _kRemoteMenubarDragX);
    if (!mounted) return;
    // Clamp to the saved drag bounds so a corrupted or out-of-range saved
    // value can't bypass them until the user drags again.
    final dragLeft = double.tryParse(
            bind.mainGetLocalOption(key: kOptionRemoteMenubarDragLeft)) ??
        0.0;
    final dragRight = double.tryParse(
            bind.mainGetLocalOption(key: kOptionRemoteMenubarDragRight)) ??
        1.0;
    final fraction = _clampToolbarFraction(
        double.tryParse(saved ?? '') ?? 0.5, dragLeft, dragRight);
    _fraction.value = fraction;
    _toolbarFractionBySession[widget.ffi.sessionId.toString()] = fraction;
    _fractionLoaded.value = true;
  }

  @override
  initState() {
    super.initState();

    final cached = _toolbarFractionBySession[widget.ffi.sessionId.toString()];
    if (cached != null) {
      _fraction.value = cached;
      _fractionLoaded.value = true;
    }

    WidgetsBinding.instance.addPostFrameCallback((_) async {
      if (_fractionLoaded.isFalse) {
        await _loadFraction();
      }
      // Initialize toolbar states (collapse, hide) from session options
      widget.state.init(widget.ffi.sessionId);
    });

    _debouncerHide = Debouncer<int>(
      Duration(milliseconds: 5000),
      onChanged: _debouncerHideProc,
      initialValue: 0,
    );

    widget.onEnterOrLeaveImageSetter(identityHashCode(this), (enter) {
      if (enter) {
        triggerAutoHide();
        _isCursorOverImage = true;
      } else {
        _isCursorOverImage = false;
      }
    });
  }

  @override
  void didUpdateWidget(covariant RemoteToolbar oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (_fractionLoaded.isFalse) {
      WidgetsBinding.instance.addPostFrameCallback((_) => _loadFraction());
    }
  }

  _debouncerHideProc(int v) {
    if (!pin && collapse.isFalse && _isCursorOverImage && _dragging.isFalse) {
      collapse.value = true;
    }
  }

  @override
  dispose() {
    widget.onEnterOrLeaveImageCleaner(identityHashCode(this));
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return Obx(() {
      // Wait for initialization to complete to prevent flickering
      if (!widget.state.initialized.value || _fractionLoaded.isFalse) {
        return const SizedBox.shrink();
      }
      // If toolbar is hidden, return empty widget
      if (hide.value) {
        return const SizedBox.shrink();
      }

      // Measure the live toolbar after every layout so the preview ghost can
      // match its actual footprint (collapsed handle vs expanded toolbar).
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (_dragging.isTrue) return;
        final ro = _toolbarKey.currentContext?.findRenderObject();
        if (ro is RenderBox && ro.hasSize) {
          final s = ro.size;
          if (_toolbarSize.value != s) _toolbarSize.value = s;
        }
      });

      final toolbar = Align(
        alignment: _toolbarAlignment(_fraction.value),
        child: KeyedSubtree(
          key: _toolbarKey,
          child: collapse.isFalse
              ? _buildToolbar(context)
              : _buildDraggableCollapse(context),
        ),
      );

      // Always return the Stack — even when not dragging — so the toolbar's
      // position in the Element tree stays stable. Wrapping/unwrapping it
      // mid-drag was killing the Draggable's gesture state.
      return Stack(
        fit: StackFit.expand,
        children: [
          IgnorePointer(
            child: Obx(() {
              final pf = _previewFraction.value;
              if (!_dragging.isTrue || pf == null) {
                return const SizedBox.shrink();
              }
              return _buildDragPreview(context, pf, _toolbarSize.value);
            }),
          ),
          toolbar,
        ],
      );
    });
  }

  Widget _buildDragPreview(
      BuildContext context, double fraction, Size? measured) {
    final color = Theme.of(context).colorScheme.primary;
    // Use the measured live toolbar size so collapsed vs expanded looks right.
    final previewSize = measured ?? _toolbarFallbackSize;
    return Align(
      alignment: _toolbarAlignment(fraction),
      child: Container(
        width: previewSize.width,
        height: previewSize.height,
        decoration: BoxDecoration(
          color: color.withOpacity(0.10),
          borderRadius: BorderRadius.circular(6),
          border: Border.all(color: color.withOpacity(0.55), width: 1.5),
        ),
      ),
    );
  }

  Widget _buildDraggableCollapse(BuildContext context) {
    return Obx(() {
      if (collapse.isFalse && _dragging.isFalse) {
        triggerAutoHide();
      }
      const borderRadius = BorderRadius.vertical(bottom: Radius.circular(5));
      return Offstage(
        offstage: _dragging.isTrue,
        child: Material(
          elevation: _ToolbarTheme.elevation,
          shadowColor: MyTheme.color(context).shadow,
          borderRadius: borderRadius,
          child: _DraggableShowHide(
            id: widget.id,
            ffi: widget.ffi,
            sessionId: widget.ffi.sessionId,
            dragging: _dragging,
            fraction: _fraction,
            previewFraction: _previewFraction,
            toolbarSize: _toolbarSize,
            toolbarState: widget.state,
            setFullscreen: _setFullscreen,
            setMinimize: _minimize,
            borderRadius: borderRadius,
          ),
        ),
      );
    });
  }

  Widget _buildToolbar(BuildContext context) {
    final List<Widget> toolbarItems = [];
    toolbarItems.add(_PinMenu(state: widget.state));
    toolbarItems.add(Obx(() {
      if (pi.displaysCount.value > 1) {
        return _MonitorMenu(
            id: widget.id,
            ffi: widget.ffi,
            setRemoteState: widget.setRemoteState);
      } else {
        return Offstage();
      }
    }));

    toolbarItems
        .add(_ControlMenu(id: widget.id, ffi: widget.ffi, state: widget.state));
    toolbarItems.add(_DisplayMenu(
      id: widget.id,
      ffi: widget.ffi,
      state: widget.state,
      setFullscreen: _setFullscreen,
    ));
    toolbarItems.add(_CloseMenu(id: widget.id, ffi: widget.ffi));
    final toolbarBorderRadius = BorderRadius.all(Radius.circular(4.0));
    final spacer = SizedBox(width: _ToolbarTheme.buttonHMargin * 2);
    final toolbarMaterial = Material(
      elevation: _ToolbarTheme.elevation,
      shadowColor: MyTheme.color(context).shadow,
      borderRadius: toolbarBorderRadius,
      color: Theme.of(context)
          .menuBarTheme
          .style
          ?.backgroundColor
          ?.resolve(MaterialState.values.toSet()),
      child: SingleChildScrollView(
        scrollDirection: Axis.horizontal,
        child: Theme(
          data: themeData(),
          child: _ToolbarTheme.borderWrapper(
              context,
              Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  spacer,
                  ...toolbarItems,
                  spacer,
                ],
              ),
              toolbarBorderRadius),
        ),
      ),
    );
    // The handle hangs below the toolbar, centered.
    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [toolbarMaterial, _buildDraggableCollapse(context)],
    );
  }

  ThemeData themeData() {
    return Theme.of(context).copyWith(
      menuButtonTheme: MenuButtonThemeData(
        style: ButtonStyle(
          minimumSize: MaterialStatePropertyAll(Size(64, 32)),
          textStyle: MaterialStatePropertyAll(
            TextStyle(fontWeight: FontWeight.normal),
          ),
          shape: MaterialStatePropertyAll(RoundedRectangleBorder(
              borderRadius:
                  BorderRadius.circular(_ToolbarTheme.menuButtonBorderRadius))),
        ),
      ),
      dividerTheme: DividerThemeData(
        space: _ToolbarTheme.dividerSpaceToAction,
        color: _ToolbarTheme.dividerColor(context),
      ),
      menuBarTheme: MenuBarThemeData(
          style: MenuStyle(
        padding: MaterialStatePropertyAll(EdgeInsets.zero),
        elevation: MaterialStatePropertyAll(0),
        shape: MaterialStatePropertyAll(BeveledRectangleBorder()),
      ).copyWith(
              backgroundColor:
                  Theme.of(context).menuBarTheme.style?.backgroundColor)),
    );
  }
}

class _PinMenu extends StatelessWidget {
  final ToolbarState state;
  const _PinMenu({Key? key, required this.state}) : super(key: key);

  @override
  Widget build(BuildContext context) {
    return Obx(
      () => _IconMenuButton(
        assetName: state.pin ? "assets/pinned.svg" : "assets/unpinned.svg",
        tooltip: state.pin ? 'Unpin Toolbar' : 'Pin Toolbar',
        onPressed: state.switchPin,
        color:
            state.pin ? _ToolbarTheme.accentColor(context) : _ToolbarTheme.inactiveColor,
        hoverColor: state.pin
            ? _ToolbarTheme.hoverAccentColor(context)
            : _ToolbarTheme.hoverInactiveColor,
      ),
    );
  }
}

class _MonitorMenu extends StatelessWidget {
  final String id;
  final FFI ffi;
  final Function(VoidCallback) setRemoteState;
  const _MonitorMenu({
    Key? key,
    required this.id,
    required this.ffi,
    required this.setRemoteState,
  }) : super(key: key);

  bool get supportIndividualWindows =>
      ffi.ffiModel.pi.isSupportMultiDisplay;

  @override
  Widget build(BuildContext context) => Obx(() => buildMonitorMenu(context));

  Widget buildMonitorMenu(BuildContext context) {
    final width = SimpleWrapper<double>(0);
    final monitorsIcon =
        globalMonitorsWidget(width, Colors.white, Colors.black38);
    return _IconSubmenuButton(
        tooltip: 'Select Monitor',
        icon: monitorsIcon,
        ffi: ffi,
        width: width.value,
        color: _ToolbarTheme.accentColor(context),
        hoverColor: _ToolbarTheme.hoverAccentColor(context),
        menuStyle: MenuStyle(
            padding:
                MaterialStatePropertyAll(EdgeInsets.symmetric(horizontal: 6))),
        menuChildrenGetter: (_) => [buildMonitorSubmenuWidget(context)]);
  }

  Widget buildMonitorSubmenuWidget(BuildContext context) {
    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        Row(children: buildMonitorList(context)),
      ],
    );
  }

  buildOneMonitorButton(BuildContext context, i, curDisplay) => Text(
        '${i + 1}',
        style: TextStyle(
          color: i == curDisplay
              ? _ToolbarTheme.accentColor(context)
              : _ToolbarTheme.inactiveColor,
          fontSize: 12,
          fontWeight: FontWeight.bold,
        ),
      );

  List<Widget> buildMonitorList(BuildContext context) {
    final List<Widget> monitorList = [];
    final pi = ffi.ffiModel.pi;

    buildMonitorButton(int i) => Obx(() {
          RxInt display = CurrentDisplayState.find(id);

          final isAllMonitors = i == kAllDisplayValue;
          final width = SimpleWrapper<double>(0);
          Widget? monitorsIcon;
          if (isAllMonitors) {
            monitorsIcon = globalMonitorsWidget(
                width, Colors.white, _ToolbarTheme.accentColor(context));
          }
          return _IconMenuButton(
            tooltip: isAllMonitors ? 'All monitors' : '#{${i + 1}} monitor',
            hMargin: 6,
            vMargin: 12,
            topLevel: false,
            color: i == display.value
                ? _ToolbarTheme.accentColor(context)
                : _ToolbarTheme.inactiveColor,
            hoverColor: i == display.value
                ? _ToolbarTheme.hoverAccentColor(context)
                : _ToolbarTheme.hoverInactiveColor,
            width: isAllMonitors ? width.value : null,
            icon: isAllMonitors
                ? monitorsIcon
                : Container(
                    alignment: AlignmentDirectional.center,
                    constraints:
                        const BoxConstraints(minHeight: _ToolbarTheme.height),
                    child: Stack(
                      alignment: Alignment.center,
                      children: [
                        SvgPicture.asset(
                          "assets/screen.svg",
                          colorFilter:
                              ColorFilter.mode(Colors.white, BlendMode.srcIn),
                        ),
                        Obx(() => buildOneMonitorButton(context, i, display.value)),
                      ],
                    ),
                  ),
            onPressed: () => onPressed(i, pi),
          );
        });

    for (int i = 0; i < pi.displays.length; i++) {
      monitorList.add(buildMonitorButton(i));
    }
    if (supportIndividualWindows && pi.displays.length > 1) {
      monitorList.add(buildMonitorButton(kAllDisplayValue));
    }
    return monitorList;
  }

  globalMonitorsWidget(
      SimpleWrapper<double> width, Color activeTextColor, Color activeBgColor) {
    getMonitors() {
      final pi = ffi.ffiModel.pi;
      RxInt display = CurrentDisplayState.find(id);
      final rect = ffi.ffiModel.globalDisplaysRect();
      if (rect == null) {
        return Offstage();
      }

      final scale = _ToolbarTheme.buttonSize / rect.height * 0.75;
      final height = rect.height * scale;
      final startY = (_ToolbarTheme.buttonSize - height) * 0.5;
      final startX = startY;

      final children = <Widget>[];
      for (var i = 0; i < pi.displays.length; i++) {
        final d = pi.displays[i];
        double s = d.scale;
        int dWidth = d.width.toDouble() ~/ s;
        int dHeight = d.height.toDouble() ~/ s;
        final fontSize = (dWidth * scale < dHeight * scale
                ? dWidth * scale
                : dHeight * scale) *
            0.65;
        children.add(Positioned(
          left: (d.x - rect.left) * scale + startX,
          top: (d.y - rect.top) * scale + startY,
          width: dWidth * scale,
          height: dHeight * scale,
          child: Container(
            decoration: BoxDecoration(
              border: Border.all(
                color: Colors.grey,
                width: 1.0,
              ),
              color: display.value == i ? activeBgColor : Colors.white,
            ),
            child: Center(
                child: Text(
              '${i + 1}',
              style: TextStyle(
                color: display.value == i
                    ? activeTextColor
                    : _ToolbarTheme.inactiveColor,
                fontSize: fontSize,
                fontWeight: FontWeight.bold,
              ),
            )),
          ),
        ));
      }
      width.value = rect.width * scale + startX * 2;
      return SizedBox(
        width: width.value,
        height: height + startY * 2,
        child: Stack(
          children: children,
        ),
      );
    }

    return Stack(
      alignment: Alignment.center,
      children: [
        SizedBox(height: _ToolbarTheme.buttonSize),
        getMonitors(),
      ],
    );
  }

  onPressed(int i, PeerInfo pi) {
    _menuDismissCallback(ffi);
    RxInt display = CurrentDisplayState.find(id);
    if (display.value != i) {
      openMonitorInTheSameTab(i, ffi, pi);
    }
  }
}

class _ControlMenu extends StatelessWidget {
  final String id;
  final FFI ffi;
  final ToolbarState state;
  _ControlMenu(
      {Key? key, required this.id, required this.ffi, required this.state})
      : super(key: key);

  @override
  Widget build(BuildContext context) {
    return _IconSubmenuButton(
        tooltip: 'Control Actions',
        svg: "assets/actions.svg",
        color: _ToolbarTheme.accentColor(context),
        hoverColor: _ToolbarTheme.hoverAccentColor(context),
        ffi: ffi,
        menuChildrenGetter: (_) => toolbarControls(context, id, ffi).map((e) {
              if (e.divider) {
                return Divider();
              } else {
                final hint = e.actionId == null
                    ? null
                    : ShortcutDisplay.formatFor(e.actionId!);
                final child = hint == null
                    ? e.child
                    : Row(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          Flexible(child: e.child),
                          Flexible(
                            child: Padding(
                              padding: const EdgeInsets.only(left: 16),
                              child: Text(
                                hint,
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                                style: Theme.of(context)
                                    .textTheme
                                    .bodySmall
                                    ?.copyWith(
                                      color: Theme.of(context).hintColor,
                                    ),
                              ),
                            ),
                          ),
                        ],
                      );
                return MenuButton(
                    child: child,
                    onPressed: e.onPressed,
                    ffi: ffi,
                    trailingIcon: e.trailingIcon);
              }
            }).toList());
  }
}

class ScreenAdjustor {
  final String id;
  final FFI ffi;
  final VoidCallback cbExitFullscreen;
  window_size.Screen? _screen;

  ScreenAdjustor({
    required this.id,
    required this.ffi,
    required this.cbExitFullscreen,
  });

  bool get isFullscreen => stateGlobal.fullscreen.isTrue;
  int get windowId => stateGlobal.windowId;

  Future<bool?> isWindowMaximized() async {
    try {
      return await WindowController.fromWindowId(windowId).isMaximized();
    } catch (_) {
      // The delayed resolution callback may run after the window is disposed.
      return null;
    }
  }

  adjustWindow(BuildContext context) {
    return futureBuilder(
        future: isWindowCanBeAdjusted(context),
        hasData: (data) {
          final visible = data as bool;
          if (!visible) return Offstage();
          return Column(
            children: [
              MenuButton(
                  child: Text(translate('Adjust Window')),
                  onPressed: () => doAdjustWindow(context),
                  ffi: ffi),
              Divider(),
            ],
          );
        });
  }

  // Linux screen and work-area coordinates can use different units or become
  // unreliable across Wayland/X11 state changes, so normalize reported frames
  // and cache usable work-area measurements before sizing the window.

  Future<void> _updateLinuxWorkAreaCache({
    required window_size.Screen screen,
    required Rect wndRect,
    required bool isWayland,
    required bool isX11,
    required bool forMenu,
  }) async {
  }

  Future<Rect?> _getEffectiveScreenFrame({
    required window_size.Screen screen,
    required bool isWayland,
    required bool isX11,
    required bool forMenu,
  }) async {
    Rect frameRect = screen.visibleFrame;
    return frameRect;
  }

  Future<Rect?> _getAdjustedWindowFrame(Size mediaSize,
      {bool forMenu = false}) async {
    final screen = _screen;
    if (screen != null) {
      // Windows window frames use physical pixels while Flutter view sizes are
      // logical. macOS and Linux window frames use the same units as Flutter.
      double scale = screen.scaleFactor;
      final Rect wndRect;
      try {
        wndRect = await WindowController.fromWindowId(windowId).getFrame();
      } catch (e) {
        debugPrint("Failed to get frame of window $windowId, it may be hidden");
        return null;
      }
      // On Windows, wndRect is GetWindowRect while mediaSize is GetClientRect.
      // https://stackoverflow.com/a/7561083
      double magicWidth =
          wndRect.right - wndRect.left - mediaSize.width * scale;
      double magicHeight =
          wndRect.bottom - wndRect.top - mediaSize.height * scale;
      final canvasModel = ffi.canvasModel;
      // canvasModel.scale is the rendered scale and already applies kIgnoreDpi.
      // Use it instead of the remote source resolution.
      await _updateLinuxWorkAreaCache(
        screen: screen,
        wndRect: wndRect,
        isWayland: false,
        isX11: false,
        forMenu: forMenu,
      );
      if (forMenu && isFullscreen) {
        // desktop_multi_window's hidden title bar keeps 8 physical pixels on
        // each horizontal edge and at the bottom, plus up to 1px at the top.
        // Fullscreen removes these in WM_NCCALCSIZE, so predict the restored
        // frame's worst-case padding when deciding whether to show the menu.
        magicWidth = 16.0;
        magicHeight = 9.0;
      }
      double horizontalEdges;
      double verticalEdges;
      if (forMenu && isFullscreen) {
        // Linux Adjust Window unmaximizes; macOS and Windows exit fullscreen
        // before resizing. Predict the restored normal-window edges when
        // deciding whether to show the menu item.
        final resizePadding = 0.0;
        final windowEdge = kWindowBorderWidth + resizePadding;
        horizontalEdges = windowEdge * 2;
        verticalEdges = kDesktopRemoteTabBarHeight + windowEdge * 2;
      } else {
        horizontalEdges = CanvasModel.leftToEdge + CanvasModel.rightToEdge;
        verticalEdges = CanvasModel.topToEdge + CanvasModel.bottomToEdge;
      }
      final width = (canvasModel.getDisplayWidth() * canvasModel.scale +
                  horizontalEdges) *
              scale +
          magicWidth;
      final height =
          (canvasModel.getDisplayHeight() * canvasModel.scale + verticalEdges) *
                  scale +
              magicHeight;
      double left = wndRect.left + (wndRect.width - width) / 2;
      double top = wndRect.top + (wndRect.height - height) / 2;

      final frameRect = await _getEffectiveScreenFrame(
        screen: screen,
        isWayland: false,
        isX11: false,
        forMenu: forMenu,
      );
      if (frameRect == null) {
        return null;
      }
      var availableSize = frameRect.size;
      // A window frame cannot be smaller than its client area. Tolerate small
      // floating-point differences; larger negative values mean the native
      // frame and Flutter view metrics are not synchronized.
      if (magicWidth < -0.1 || magicHeight < -0.1) {
        return null;
      }
      // Reject implausibly small targets to avoid hiding the window.
      if (width < 300 || height < 300) {
        return null;
      }
      // The remote size may change after the menu is built. Reject targets
      // that exceed the available area.
      final exceedsScreen =
          width > availableSize.width || height > availableSize.height;
      if (exceedsScreen) {
        return null;
      }
      if (left < frameRect.left) {
        left = frameRect.left;
      }
      if (top < frameRect.top) {
        top = frameRect.top;
      }
      if ((left + width) > frameRect.right) {
        left = frameRect.right - width;
      }
      if ((top + height) > frameRect.bottom) {
        top = frameRect.bottom - height;
      }
      return Rect.fromLTWH(left, top, width, height);
    }
    return null;
  }

  doAdjustWindow([BuildContext? context]) async {
    // A resolution change is adjusted after a delay, when the menu context may
    // already be disposed. Each desktop_multi_window window has its own engine,
    // so that engine's first view is the current window.
    final views = WidgetsBinding.instance.platformDispatcher.views;
    if (context == null && views.isEmpty) {
      return;
    }
    final view = context != null ? View.of(context) : views.first;
    await updateScreen();
    if (_screen != null) {
      final wc = WindowController.fromWindowId(windowId);
      final wasFullscreen = isFullscreen;
      cbExitFullscreen();
      if (wasFullscreen) {
        // Wait for the native fullscreen exit to update the window frame.
        await Future.delayed(Duration(milliseconds: 700));
        await updateScreen();
      }
      final mediaSize = MediaQueryData.fromView(view).size;
      final frame = await _getAdjustedWindowFrame(mediaSize);
      if (frame == null) {
        return;
      }
      try {
        await wc.setFrame(frame);
      } catch (_) {
        return;
      }
      stateGlobal.setMaximized(false);
    }
  }

  updateScreen() async {
    _screen = await _getCurrentScreen();
  }

  Future<window_size.Screen?> _getCurrentScreen() async {
    try {
      return (await window_size.getWindowInfo()).screen;
    } catch (e) {
      debugPrint('Failed to get current window screen: $e');
      return null;
    }
  }

  Future<bool> isWindowCanBeAdjusted([BuildContext? context]) async {
    // Capture the view before awaiting because the menu context may be disposed.
    final views = WidgetsBinding.instance.platformDispatcher.views;
    if (context == null && views.isEmpty) {
      return false;
    }
    final view = context != null ? View.of(context) : views.first;
    final mediaSize = MediaQueryData.fromView(view).size;
    final viewStyle =
        await bind.sessionGetViewStyle(sessionId: ffi.sessionId) ?? '';
    if (viewStyle != kRemoteViewStyleOriginal) {
      return false;
    }
    final remoteCount = RemoteCountState.find().value;
    if (remoteCount != 1) {
      return false;
    }
    await updateScreen();
    if (_screen == null) {
      return false;
    }
    return await _getAdjustedWindowFrame(mediaSize, forMenu: true) != null;
  }
}

class _DisplayMenu extends StatefulWidget {
  final String id;
  final FFI ffi;
  final ToolbarState state;
  final Function(bool) setFullscreen;
  const _DisplayMenu(
      {required this.id,
      required this.ffi,
      required this.state,
      required this.setFullscreen});

  @override
  State<_DisplayMenu> createState() => _DisplayMenuState();
}

class _DisplayMenuState extends State<_DisplayMenu> {
  final RxInt _customPercent = 100.obs;
  late final ScreenAdjustor _screenAdjustor = ScreenAdjustor(
    id: widget.id,
    ffi: widget.ffi,
    cbExitFullscreen: () => widget.setFullscreen(false),
  );

  int get windowId => stateGlobal.windowId;
  Map<String, bool> get perms => widget.ffi.ffiModel.permissions;
  PeerInfo get pi => widget.ffi.ffiModel.pi;
  FfiModel get ffiModel => widget.ffi.ffiModel;
  FFI get ffi => widget.ffi;
  String get id => widget.id;

  @override
  void initState() {
    super.initState();
    // Initialize custom percent from stored option once
    WidgetsBinding.instance.addPostFrameCallback((_) async {
      try {
        final v = await getSessionCustomScalePercent(widget.ffi.sessionId);
        if (_customPercent.value != v) {
          _customPercent.value = v;
        }
      } catch (_) {}
    });
  }

  @override
  Widget build(BuildContext context) {
    menuChildrenGetter(_IconSubmenuButtonState state) {
      final menuChildren = <Widget>[
        _screenAdjustor.adjustWindow(context),
        viewStyle(customPercent: _customPercent),
        imageQuality(),
        if (ffi.connType == ConnType.defaultConn)
          _ResolutionsMenu(
            id: widget.id,
            ffi: widget.ffi,
            screenAdjustor: _screenAdjustor,
          ),
        Divider(),
        toggles(),
      ];
      return menuChildren;
    }

    return _IconSubmenuButton(
      tooltip: 'Display Settings',
      svg: "assets/display.svg",
      ffi: widget.ffi,
      color: _ToolbarTheme.accentColor(context),
      hoverColor: _ToolbarTheme.hoverAccentColor(context),
      menuChildrenGetter: menuChildrenGetter,
    );
  }

  viewStyle({required RxInt customPercent}) {
    return futureBuilder(
        future: toolbarViewStyle(context, widget.id, widget.ffi),
        hasData: (data) {
          final v = data as List<TRadioMenu<String>>;
          final bool isCustomSelected = v.isNotEmpty
              ? v.first.groupValue == kRemoteViewStyleCustom
              : false;
          return Column(children: [
            ...v.map((e) {
              final isCustom = e.value == kRemoteViewStyleCustom;
              final child =
                  isCustom ? Text(translate('Scale custom')) : e.child;
              // Whether the current selection is already custom
              final bool isGroupCustomSelected =
                  e.groupValue == kRemoteViewStyleCustom;
              // Keep menu open when switching INTO custom so the slider is visible immediately
              final bool keepOpenForThisItem =
                  isCustom && !isGroupCustomSelected;
              return RdoMenuButton<String>(
                  value: e.value,
                  groupValue: e.groupValue,
                  onChanged: (value) {
                    // Perform the original change
                    e.onChanged?.call(value);
                    // Only force a rebuild when we keep the menu open to reveal the slider
                    if (keepOpenForThisItem) {
                      setState(() {});
                    }
                  },
                  child: child,
                  ffi: ffi,
                  // When entering custom, keep submenu open to show the slider controls
                  closeOnActivate: !keepOpenForThisItem);
            }).toList(),
            // Only show a divider when custom is NOT selected
            if (!isCustomSelected) Divider(),
            _customControlsIfCustomSelected(
                onChanged: (v) => customPercent.value = v),
          ]);
        });
  }

  Widget _customControlsIfCustomSelected({ValueChanged<int>? onChanged}) {
    return futureBuilder(future: () async {
      final current = await bind.sessionGetViewStyle(sessionId: ffi.sessionId);
      return current == kRemoteViewStyleCustom;
    }(), hasData: (data) {
      final isCustom = data as bool;
      return AnimatedSwitcher(
        duration: Duration(milliseconds: 220),
        switchInCurve: Curves.easeOut,
        switchOutCurve: Curves.easeIn,
        child: isCustom
            ? _CustomScaleMenuControls(ffi: ffi, onChanged: onChanged)
            : SizedBox.shrink(),
      );
    });
  }

  imageQuality() {
    return futureBuilder(
        future: toolbarImageQuality(context, widget.id, widget.ffi),
        hasData: (data) {
          final v = data as List<TRadioMenu<String>>;
          return _SubmenuButton(
            ffi: widget.ffi,
            child: Text(translate('Image Quality')),
            menuChildren: v
                .map((e) => RdoMenuButton<String>(
                    value: e.value,
                    groupValue: e.groupValue,
                    onChanged: e.onChanged,
                    child: e.child,
                    ffi: ffi))
                .toList(),
          );
        });
  }

  toggles() {
    return futureBuilder(
        future: toolbarDisplayToggle(context, id, ffi),
        hasData: (data) {
          final v = data as List<TToggleMenu>;
          if (v.isEmpty) return Offstage();
          return Column(
              children: v
                  .map((e) => CkbMenuButton(
                      value: e.value,
                      onChanged: e.onChanged,
                      child: e.child,
                      ffi: ffi))
                  .toList());
        });
  }
}

class _CustomScaleMenuControls extends StatefulWidget {
  final FFI ffi;
  final ValueChanged<int>? onChanged;
  const _CustomScaleMenuControls({Key? key, required this.ffi, this.onChanged})
      : super(key: key);

  @override
  State<_CustomScaleMenuControls> createState() =>
      _CustomScaleMenuControlsState();
}

class _CustomScaleMenuControlsState
    extends CustomScaleControls<_CustomScaleMenuControls> {
  @override
  FFI get ffi => widget.ffi;

  @override
  ValueChanged<int>? get onScaleChanged => widget.onChanged;

  @override
  Widget build(BuildContext context) {
    final colorScheme = Theme.of(context).colorScheme;
    const smallBtnConstraints = BoxConstraints(minWidth: 28, minHeight: 28);

    final sliderControl = Semantics(
      label: translate('Custom scale slider'),
      value: '$scaleValue%',
      child: SliderTheme(
        data: SliderTheme.of(context).copyWith(
          activeTrackColor: colorScheme.primary,
          thumbColor: colorScheme.primary,
          overlayColor: colorScheme.primary.withOpacity(0.1),
          showValueIndicator: ShowValueIndicator.never,
          thumbShape: _RectValueThumbShape(
            min: CustomScaleControls.minPercent.toDouble(),
            max: CustomScaleControls.maxPercent.toDouble(),
            width: 52,
            height: 24,
            radius: 4,
            displayValueForNormalized: (t) => mapPosToPercent(t),
          ),
        ),
        child: Slider(
          value: scalePos,
          min: 0.0,
          max: 1.0,
          // Use a wide range of divisions (calculated as (CustomScaleControls.maxPercent - CustomScaleControls.minPercent)) to provide ~1% precision increments.
          // This allows users to set precise scale values. Lower values would require more fine-tuning via the +/- buttons, which is undesirable for big ranges.
          divisions:
              (CustomScaleControls.maxPercent - CustomScaleControls.minPercent)
                  .round(),
          onChanged: onSliderChanged,
        ),
      ),
    );

    return Column(children: [
      Padding(
        padding: const EdgeInsets.symmetric(horizontal: 12.0),
        child: Row(children: [
          Tooltip(
            message: translate('Decrease'),
            child: IconButton(
              iconSize: 16,
              padding: EdgeInsets.all(1),
              constraints: smallBtnConstraints,
              icon: const Icon(Icons.remove),
              onPressed: () => nudgeScale(-1),
            ),
          ),
          Expanded(child: sliderControl),
          Tooltip(
            message: translate('Increase'),
            child: IconButton(
              iconSize: 16,
              padding: EdgeInsets.all(1),
              constraints: smallBtnConstraints,
              icon: const Icon(Icons.add),
              onPressed: () => nudgeScale(1),
            ),
          ),
        ]),
      ),
      Divider(),
    ]);
  }
}

// Lightweight rectangular thumb that paints the current percentage.
// Stateless and uses only SliderTheme colors; avoids allocations beyond a TextPainter per frame.
class _RectValueThumbShape extends SliderComponentShape {
  final double min;
  final double max;
  final double width;
  final double height;
  final double radius;
  final String unit;
  // Optional mapper to compute display value from normalized position [0,1]
  // If null, falls back to linear interpolation between min and max.
  final int Function(double normalized)? displayValueForNormalized;

  const _RectValueThumbShape({
    required this.min,
    required this.max,
    required this.width,
    required this.height,
    required this.radius,
    this.displayValueForNormalized,
    this.unit = '%',
  });

  @override
  Size getPreferredSize(bool isEnabled, bool isDiscrete) {
    return Size(width, height);
  }

  @override
  void paint(
    PaintingContext context,
    Offset center, {
    required Animation<double> activationAnimation,
    required Animation<double> enableAnimation,
    required bool isDiscrete,
    required TextPainter labelPainter,
    required RenderBox parentBox,
    required SliderThemeData sliderTheme,
    required TextDirection textDirection,
    required double value,
    required double textScaleFactor,
    required Size sizeWithOverflow,
  }) {
    final Canvas canvas = context.canvas;

    // Resolve color based on enabled/disabled animation, with safe fallbacks.
    final ColorTween colorTween = ColorTween(
      begin: sliderTheme.disabledThumbColor,
      end: sliderTheme.thumbColor,
    );
    final Color? evaluatedColor = colorTween.evaluate(enableAnimation);
    final Color? thumbColor = sliderTheme.thumbColor;
    final Color fillColor = evaluatedColor ?? thumbColor ?? MyTheme.accentLight;

    final RRect rrect = RRect.fromRectAndRadius(
      Rect.fromCenter(center: center, width: width, height: height),
      Radius.circular(radius),
    );
    final Paint paint = Paint()..color = fillColor;
    canvas.drawRRect(rrect, paint);

    // Compute displayed value from normalized slider value.
    final int displayValue = displayValueForNormalized != null
        ? displayValueForNormalized!(value)
        : (min + value * (max - min)).round();
    final TextSpan span = TextSpan(
      text: '$displayValue$unit',
      style: const TextStyle(
        color: Colors.white,
        fontSize: 12,
        fontWeight: FontWeight.w600,
      ),
    );
    final TextPainter tp = TextPainter(
      text: span,
      textAlign: TextAlign.center,
      textDirection: textDirection,
    );
    tp.layout(maxWidth: width - 4);
    tp.paint(
        canvas, Offset(center.dx - tp.width / 2, center.dy - tp.height / 2));
  }
}

class _ResolutionsMenu extends StatefulWidget {
  final String id;
  final FFI ffi;
  final ScreenAdjustor screenAdjustor;

  _ResolutionsMenu({
    Key? key,
    required this.id,
    required this.ffi,
    required this.screenAdjustor,
  }) : super(key: key);

  @override
  State<_ResolutionsMenu> createState() => _ResolutionsMenuState();
}

const double _kCustomResolutionEditingWidth = 42;
const _kCustomResolutionValue = 'custom';

class _ResolutionsMenuState extends State<_ResolutionsMenu> {
  String _groupValue = '';
  Resolution? _localResolution;

  late final TextEditingController _customWidth =
      TextEditingController(text: rect?.width.toInt().toString() ?? '');
  late final TextEditingController _customHeight =
      TextEditingController(text: rect?.height.toInt().toString() ?? '');

  FFI get ffi => widget.ffi;
  PeerInfo get pi => widget.ffi.ffiModel.pi;
  FfiModel get ffiModel => widget.ffi.ffiModel;
  Rect? get rect => scaledRect();
  List<Resolution> get resolutions => pi.resolutions;
  bool get isWayland => bind.mainCurrentIsWayland();

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) {
      _getLocalResolution();
    });
  }

  Rect? scaledRect() {
    final scale = pi.scaleOfDisplay(pi.currentDisplay);
    final rect = ffiModel.rect;
    if (rect == null) {
      return null;
    }
    return Rect.fromLTWH(
      rect.left,
      rect.top,
      rect.width / scale,
      rect.height / scale,
    );
  }

  @override
  Widget build(BuildContext context) {
    final visible = ffiModel.keyboard &&
        resolutions.length > 1 &&
        pi.currentDisplay != kAllDisplayValue;
    if (!visible) return Offstage();
    final showOriginalBtn =
        ffiModel.isOriginalResolutionSet && !ffiModel.isOriginalResolution;
    final showFitLocalBtn = !_isRemoteResolutionFitLocal();
    _setGroupValue();
    return _SubmenuButton(
      ffi: widget.ffi,
      menuChildren: <Widget>[
            _OriginalResolutionMenuButton(context, showOriginalBtn),
            _FitLocalResolutionMenuButton(context, showFitLocalBtn),
            _menuDivider(showOriginalBtn, showFitLocalBtn),
          ] +
          _supportedResolutionMenuButtons(),
      child: Text(translate("Resolution")),
    );
  }

  _setGroupValue() {
    if (pi.currentDisplay == kAllDisplayValue) {
      return;
    }
    final lastGroupValue =
        stateGlobal.getLastResolutionGroupValue(widget.id, pi.currentDisplay);
    if (lastGroupValue == _kCustomResolutionValue) {
      _groupValue = _kCustomResolutionValue;
    } else {
      _groupValue =
          '${(rect?.width ?? 0).toInt()}x${(rect?.height ?? 0).toInt()}';
    }
  }

  _menuDivider(bool showOriginalBtn, bool showFitLocalBtn) {
    return Offstage(
      offstage: !(showOriginalBtn || showFitLocalBtn),
      child: Divider(),
    );
  }

  _getLocalResolution() {
    _localResolution = null;
    final String mainDisplay = bind.mainGetMainDisplay();
    if (mainDisplay.isNotEmpty) {
      try {
        final display = json.decode(mainDisplay);
        if (display['w'] != null && display['h'] != null) {
          _localResolution = Resolution(display['w'], display['h']);
        }
      } catch (e) {
        debugPrint('Failed to decode $mainDisplay, $e');
      }
    }
  }

  // This widget has been unmounted, so the State no longer has a context
  _onChanged(String? value) async {
    if (pi.currentDisplay == kAllDisplayValue) {
      return;
    }
    stateGlobal.setLastResolutionGroupValue(
        widget.id, pi.currentDisplay, value);
    if (value == null) return;

    int? w;
    int? h;
    if (value == _kCustomResolutionValue) {
      w = int.tryParse(_customWidth.text);
      h = int.tryParse(_customHeight.text);
    } else {
      final list = value.split('x');
      if (list.length == 2) {
        w = int.tryParse(list[0]);
        h = int.tryParse(list[1]);
      }
    }

    if (w != null && h != null) {
      if (w != rect?.width.toInt() || h != rect?.height.toInt()) {
        await _changeResolution(w, h);
      }
    }
  }

  _changeResolution(int w, int h) async {
    if (pi.currentDisplay == kAllDisplayValue) {
      return;
    }
    await bind.sessionChangeResolution(
      sessionId: ffi.sessionId,
      display: pi.currentDisplay,
      width: w,
      height: h,
    );
    Future.delayed(Duration(seconds: 3), () async {
      final rect = ffiModel.rect;
      if (rect == null) {
        return;
      }
      if (w == rect.width.toInt() && h == rect.height.toInt()) {
        if (!await widget.screenAdjustor.isWindowCanBeAdjusted()) {
          return;
        }
        if (widget.screenAdjustor.isFullscreen) {
          return;
        }
        if ((await widget.screenAdjustor.isWindowMaximized()) == false) {
          // This delayed callback can outlive the menu State, so its context
          // is unsafe.
          widget.screenAdjustor.doAdjustWindow();
        }
      }
    });
  }

  Widget _OriginalResolutionMenuButton(
      BuildContext context, bool showOriginalBtn) {
    final display = pi.tryGetDisplayIfNotAllDisplay();
    if (display == null) {
      return Offstage();
    }
    if (!resolutions.any((e) =>
        e.width == display.originalWidth &&
        e.height == display.originalHeight)) {
      return Offstage();
    }
    return Offstage(
      offstage: !showOriginalBtn,
      child: MenuButton(
        onPressed: () =>
            _changeResolution(display.originalWidth, display.originalHeight),
        ffi: widget.ffi,
        child: Text(
            '${translate('resolution_original_tip')} ${display.originalWidth}x${display.originalHeight}'),
      ),
    );
  }

  Widget _FitLocalResolutionMenuButton(
      BuildContext context, bool showFitLocalBtn) {
    return Offstage(
      offstage: !showFitLocalBtn,
      child: MenuButton(
        onPressed: () {
          final resolution = _getBestFitResolution();
          if (resolution != null) {
            _changeResolution(resolution.width, resolution.height);
          }
        },
        ffi: widget.ffi,
        child: Text(
            '${translate('resolution_fit_local_tip')} ${_localResolution?.width ?? 0}x${_localResolution?.height ?? 0}'),
      ),
    );
  }

  List<Widget> _supportedResolutionMenuButtons() => resolutions
      .map((e) => RdoMenuButton(
          value: '${e.width}x${e.height}',
          groupValue: _groupValue,
          onChanged: (String? value) => _onChanged(value),
          ffi: widget.ffi,
          child: Text('${e.width}x${e.height}')))
      .toList();

  Resolution? _getBestFitResolution() {
    if (_localResolution == null) {
      return null;
    }

    for (final r in resolutions) {
      if (r.width == _localResolution!.width &&
          r.height == _localResolution!.height) {
        return r;
      }
    }

    return null;
  }

  bool _isRemoteResolutionFitLocal() {
    if (_localResolution == null) {
      return true;
    }
    final bestFitResolution = _getBestFitResolution();
    if (bestFitResolution == null) {
      return true;
    }
    return bestFitResolution.width == rect?.width.toInt() &&
        bestFitResolution.height == rect?.height.toInt();
  }
}

class _CloseMenu extends StatelessWidget {
  final String id;
  final FFI ffi;
  const _CloseMenu({Key? key, required this.id, required this.ffi})
      : super(key: key);

  @override
  Widget build(BuildContext context) {
    return _IconMenuButton(
      assetName: 'assets/close.svg',
      tooltip: 'Close',
      onPressed: () async {
        closeConnection(id: id);
      },
      color: _ToolbarTheme.redColor,
      hoverColor: _ToolbarTheme.hoverRedColor,
    );
  }
}

class _IconMenuButton extends StatefulWidget {
  final String? assetName;
  final Widget? icon;
  final String tooltip;
  final Color color;
  final Color hoverColor;
  final VoidCallback? onPressed;
  final double? hMargin;
  final double? vMargin;
  final bool topLevel;
  final double? width;
  const _IconMenuButton({
    Key? key,
    this.assetName,
    this.icon,
    required this.tooltip,
    required this.color,
    required this.hoverColor,
    required this.onPressed,
    this.hMargin,
    this.vMargin,
    this.topLevel = true,
    this.width,
  }) : super(key: key);

  @override
  State<_IconMenuButton> createState() => _IconMenuButtonState();
}

class _IconMenuButtonState extends State<_IconMenuButton> {
  bool hover = false;

  @override
  Widget build(BuildContext context) {
    assert(widget.assetName != null || widget.icon != null);
    final icon = widget.icon ??
        SvgPicture.asset(
          widget.assetName!,
          colorFilter: ColorFilter.mode(Colors.white, BlendMode.srcIn),
          width: _ToolbarTheme.buttonSize,
          height: _ToolbarTheme.buttonSize,
        );
    var button = SizedBox(
      width: widget.width ?? _ToolbarTheme.buttonSize,
      height: _ToolbarTheme.buttonSize,
      child: MenuItemButton(
          style: ButtonStyle(
              backgroundColor: MaterialStatePropertyAll(Colors.transparent),
              padding: MaterialStatePropertyAll(EdgeInsets.zero),
              overlayColor: MaterialStatePropertyAll(Colors.transparent)),
          onHover: (value) => setState(() {
                hover = value;
              }),
          onPressed: widget.onPressed,
          child: Tooltip(
            message: translate(widget.tooltip),
            child: Material(
                type: MaterialType.transparency,
                child: Ink(
                    decoration: BoxDecoration(
                      borderRadius:
                          BorderRadius.circular(_ToolbarTheme.iconRadius),
                      color: hover ? widget.hoverColor : widget.color,
                    ),
                    child: icon)),
          )),
    ).marginSymmetric(
        horizontal: widget.hMargin ?? _ToolbarTheme.buttonHMargin,
        vertical: widget.vMargin ?? _ToolbarTheme.buttonVMargin);
    button = Tooltip(
      message: translate(widget.tooltip),
      child: button,
    );
    if (widget.topLevel) {
      return MenuBar(children: [button]);
    } else {
      return button;
    }
  }
}

class _IconSubmenuButton extends StatefulWidget {
  final String tooltip;
  final String? svg;
  final Widget? icon;
  final Color color;
  final Color hoverColor;
  final List<Widget> Function(_IconSubmenuButtonState state) menuChildrenGetter;
  final MenuStyle? menuStyle;
  final FFI? ffi;
  final double? width;

  _IconSubmenuButton({
    Key? key,
    this.svg,
    this.icon,
    required this.tooltip,
    required this.color,
    required this.hoverColor,
    required this.menuChildrenGetter,
    this.ffi,
    this.menuStyle,
    this.width,
  }) : super(key: key);

  @override
  State<_IconSubmenuButton> createState() => _IconSubmenuButtonState();
}

class _IconSubmenuButtonState extends State<_IconSubmenuButton> {
  bool hover = false;

  @override // discard @protected
  void setState(VoidCallback fn) {
    super.setState(fn);
  }

  @override
  Widget build(BuildContext context) {
    assert(widget.svg != null || widget.icon != null);
    final icon = widget.icon ??
        SvgPicture.asset(
          widget.svg!,
          colorFilter: ColorFilter.mode(Colors.white, BlendMode.srcIn),
          width: _ToolbarTheme.buttonSize,
          height: _ToolbarTheme.buttonSize,
        );
    final button = SizedBox(
        width: widget.width ?? _ToolbarTheme.buttonSize,
        height: _ToolbarTheme.buttonSize,
        child: SubmenuButton(
            menuStyle:
                widget.menuStyle ?? _ToolbarTheme.defaultMenuStyle(context),
            style: _ToolbarTheme.defaultMenuButtonStyle,
            onHover: (value) => setState(() {
                  hover = value;
                }),
            child: Tooltip(
                message: translate(widget.tooltip),
                child: Material(
                    type: MaterialType.transparency,
                    child: Ink(
                        decoration: BoxDecoration(
                          borderRadius:
                              BorderRadius.circular(_ToolbarTheme.iconRadius),
                          color: hover ? widget.hoverColor : widget.color,
                        ),
                        child: icon))),
            menuChildren: widget
                .menuChildrenGetter(this)
                .map((e) => _buildPointerTrackWidget(e, widget.ffi))
                .toList()));
    return MenuBar(children: [
      button.marginSymmetric(
          horizontal: _ToolbarTheme.buttonHMargin,
          vertical: _ToolbarTheme.buttonVMargin)
    ]);
  }
}

class _SubmenuButton extends StatelessWidget {
  final List<Widget> menuChildren;
  final Widget? child;
  final FFI ffi;
  const _SubmenuButton({
    Key? key,
    required this.menuChildren,
    required this.child,
    required this.ffi,
  }) : super(key: key);

  @override
  Widget build(BuildContext context) {
    return SubmenuButton(
      key: key,
      child: child,
      menuChildren:
          menuChildren.map((e) => _buildPointerTrackWidget(e, ffi)).toList(),
      menuStyle: _ToolbarTheme.defaultMenuStyle(context),
    );
  }
}

class MenuButton extends StatelessWidget {
  final VoidCallback? onPressed;
  final Widget? trailingIcon;
  final Widget? child;
  final FFI? ffi;
  MenuButton(
      {Key? key,
      this.onPressed,
      this.trailingIcon,
      required this.child,
      this.ffi})
      : super(key: key);

  @override
  Widget build(BuildContext context) {
    return MenuItemButton(
        key: key,
        onPressed: onPressed != null
            ? () {
                if (ffi != null) {
                  _menuDismissCallback(ffi!);
                }
                onPressed?.call();
              }
            : null,
        trailingIcon: trailingIcon,
        child: child);
  }
}

class CkbMenuButton extends StatelessWidget {
  final bool? value;
  final ValueChanged<bool?>? onChanged;
  final Widget? child;
  final FFI? ffi;
  const CkbMenuButton(
      {Key? key,
      required this.value,
      required this.onChanged,
      required this.child,
      this.ffi})
      : super(key: key);

  @override
  Widget build(BuildContext context) {
    return CheckboxMenuButton(
      key: key,
      value: value,
      child: child,
      onChanged: onChanged != null
          ? (bool? value) {
              if (ffi != null) {
                _menuDismissCallback(ffi!);
              }
              onChanged?.call(value);
            }
          : null,
    );
  }
}

class RdoMenuButton<T> extends StatelessWidget {
  final T value;
  final T? groupValue;
  final ValueChanged<T?>? onChanged;
  final Widget? child;
  final FFI? ffi;
  // When true, submenu will be dismissed on activate; when false, it stays open.
  final bool closeOnActivate;
  const RdoMenuButton({
    Key? key,
    required this.value,
    required this.groupValue,
    required this.child,
    this.ffi,
    this.onChanged,
    this.closeOnActivate = true,
  }) : super(key: key);

  @override
  Widget build(BuildContext context) {
    return RadioMenuButton(
      value: value,
      groupValue: groupValue,
      child: child,
      closeOnActivate: closeOnActivate,
      onChanged: onChanged != null
          ? (T? value) {
              if (ffi != null && closeOnActivate) {
                _menuDismissCallback(ffi!);
              }
              onChanged?.call(value);
            }
          : null,
    );
  }
}

class _DraggableShowHide extends StatefulWidget {
  final String id;
  final FFI ffi;
  final SessionID sessionId;
  final RxDouble fraction;
  final Rxn<double> previewFraction;
  final Rxn<Size> toolbarSize;
  final RxBool dragging;
  final ToolbarState toolbarState;
  final BorderRadius borderRadius;

  final Function(bool) setFullscreen;
  final Function() setMinimize;

  const _DraggableShowHide({
    Key? key,
    required this.id,
    required this.ffi,
    required this.sessionId,
    required this.fraction,
    required this.previewFraction,
    required this.toolbarSize,
    required this.dragging,
    required this.toolbarState,
    required this.setFullscreen,
    required this.setMinimize,
    required this.borderRadius,
  }) : super(key: key);

  @override
  State<_DraggableShowHide> createState() => _DraggableShowHideState();
}

class _DraggableShowHideState extends State<_DraggableShowHide> {
  double left = 0.0;
  double right = 1.0;
  Offset? _lastPointerDown;
  double? _dragGrabOffset;
  double? _dragToolbarWidth;

  RxBool get collapse => widget.toolbarState.collapse;

  @override
  initState() {
    super.initState();

    final confLeft = double.tryParse(
        bind.mainGetLocalOption(key: kOptionRemoteMenubarDragLeft));
    if (confLeft == null) {
      bind.mainSetLocalOption(
          key: kOptionRemoteMenubarDragLeft, value: left.toString());
    } else {
      left = confLeft;
    }
    final confRight = double.tryParse(
        bind.mainGetLocalOption(key: kOptionRemoteMenubarDragRight));
    if (confRight == null) {
      bind.mainSetLocalOption(
          key: kOptionRemoteMenubarDragRight, value: right.toString());
    } else {
      right = confRight;
    }
  }

  void _ensureDragGrabOffset(Offset cursor) {
    if (_dragGrabOffset != null) return;
    final mediaWidth = MediaQueryData.fromView(View.of(context)).size.width;
    final toolbarWidth =
        (widget.toolbarSize.value ?? _toolbarFallbackSize).width;
    _dragToolbarWidth = toolbarWidth;
    final toolbarLeft = (mediaWidth - toolbarWidth) * widget.fraction.value;
    _dragGrabOffset =
        _clampToolbarFraction(cursor.dx - toolbarLeft, 0, toolbarWidth);
  }

  void _updatePreview(Offset cursor) {
    _ensureDragGrabOffset(cursor);
    final mediaWidth = MediaQueryData.fromView(View.of(context)).size.width;
    widget.previewFraction.value = _fractionForAlignedDrag(
      cursor: cursor.dx,
      grabOffset: _dragGrabOffset ?? 0,
      parentExtent: mediaWidth,
      toolbarExtent: _dragToolbarWidth ?? _toolbarFallbackSize.width,
      left: left,
      right: right,
    );
  }

  void _resetDragTracking() {
    _lastPointerDown = null;
    _dragGrabOffset = null;
    _dragToolbarWidth = null;
  }

  void _commitPreview() {
    final frac = widget.previewFraction.value;
    widget.previewFraction.value = null;
    widget.dragging.value = false;
    _resetDragTracking();
    if (frac == null) return;
    widget.fraction.value = frac;
    _toolbarFractionBySession[widget.sessionId.toString()] = frac;
    bind.sessionPeerOption(
      sessionId: widget.sessionId,
      name: _kRemoteMenubarDragX,
      value: frac.toString(),
    );
  }

  Widget _buildDraggable(BuildContext context) {
    return Listener(
      onPointerDown: (event) => _lastPointerDown = event.position,
      child: Draggable(
        // The toolbar stays on the top edge, so lock the feedback to
        // horizontal motion; otherwise the handle floats away from the top.
        axis: Axis.horizontal,
        child: Icon(
          Icons.drag_indicator,
          size: 20,
          color: MyTheme.color(context).drag_indicator,
        ),
        feedback: widget,
        onDragStarted: () {
          final pointerDown = _lastPointerDown;
          if (pointerDown != null) {
            _ensureDragGrabOffset(pointerDown);
          }
          widget.dragging.value = true;
          // Seed the preview at the current position so something shows the
          // instant the drag begins, before the first onDragUpdate.
          widget.previewFraction.value = widget.fraction.value;
        },
        onDragUpdate: (details) {
          _updatePreview(details.globalPosition);
        },
        onDragEnd: (_) => _commitPreview(),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final ButtonStyle buttonStyle = ButtonStyle(
      minimumSize: MaterialStateProperty.all(const Size(0, 0)),
      padding: MaterialStateProperty.all(EdgeInsets.zero),
    );
    final isFullscreen = stateGlobal.fullscreen;
    const double iconSize = 20;

    buttonWrapper(VoidCallback? onPressed, Widget child,
        {Color? hoverColor}) {
      final hover = hoverColor ?? _ToolbarTheme.accentColor(context);
      final bgColor = buttonStyle.backgroundColor?.resolve({});
      return TextButton(
        onPressed: onPressed,
        child: child,
        style: buttonStyle.copyWith(
          backgroundColor: MaterialStateProperty.resolveWith((states) {
            if (states.contains(MaterialState.hovered)) {
              return (bgColor ?? hover).withOpacity(0.15);
            }
            return bgColor;
          }),
        ),
      );
    }

    final child = Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        _buildDraggable(context),
        Obx(() => buttonWrapper(
              () {
                widget.setFullscreen(!isFullscreen.value);
              },
              Tooltip(
                message: translate(
                    isFullscreen.isTrue ? 'Exit Fullscreen' : 'Fullscreen'),
                child: Icon(
                  isFullscreen.isTrue
                      ? Icons.fullscreen_exit
                      : Icons.fullscreen,
                  size: iconSize,
                ),
              ),
            )),
        Obx(() => Offstage(
                offstage: isFullscreen.isFalse,
                child: buttonWrapper(
                  widget.setMinimize,
                  Tooltip(
                    message: translate('Minimize'),
                    child: Icon(
                      Icons.remove,
                      size: iconSize,
                    ),
                  ),
                ),
              )),
        buttonWrapper(
          () => setState(() {
            widget.toolbarState.switchCollapse(widget.sessionId);
          }),
          Obx((() => Tooltip(
                message: translate(
                    collapse.isFalse ? 'Hide Toolbar' : 'Show Toolbar'),
                child: Icon(
                  collapse.isTrue ? Icons.expand_more : Icons.expand_less,
                  size: iconSize,
                ),
              ))),
        ),
      ],
    );
    return TextButtonTheme(
      data: TextButtonThemeData(style: buttonStyle),
      child: Container(
        decoration: BoxDecoration(
          color: Theme.of(context)
              .menuBarTheme
              .style
              ?.backgroundColor
              ?.resolve(MaterialState.values.toSet()),
          border: Border.all(
            color: _ToolbarTheme.borderColor(context),
            width: 1,
          ),
          borderRadius: widget.borderRadius,
        ),
        child: SizedBox(
          height: 20,
          child: child,
        ),
      ),
    );
  }
}

_menuDismissCallback(FFI ffi) => ffi.inputModel.refreshMousePos();

Widget _buildPointerTrackWidget(Widget child, FFI? ffi) {
  return Listener(
    onPointerHover: (PointerHoverEvent e) => {
      if (ffi != null) {ffi.inputModel.lastMousePos = e.position}
    },
    child: MouseRegion(
      child: child,
    ),
  );
}
