import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_hbb/common.dart';
import 'package:flutter_hbb/common/formatter/id_formatter.dart';
import 'package:flutter_hbb/common/widgets/animated_rotation_widget.dart';
import 'package:flutter_hbb/common/widgets/custom_password.dart';
import 'package:flutter_hbb/consts.dart';
import 'package:flutter_hbb/desktop/pages/connection_page.dart';
import 'package:flutter_hbb/desktop/pages/desktop_setting_page.dart';
import 'package:flutter_hbb/models/platform_model.dart';
import 'package:flutter_hbb/models/server_model.dart';
import 'package:flutter_hbb/utils/multi_window_manager.dart';
import 'package:flutter_hbb/utils/platform_channel.dart';
import 'package:get/get.dart';
import 'package:provider/provider.dart';
import 'package:window_size/window_size.dart' as window_size;

class DesktopHomePage extends StatefulWidget {
  const DesktopHomePage({Key? key}) : super(key: key);

  @override
  State<DesktopHomePage> createState() => _DesktopHomePageState();
}

class _DesktopHomePageState extends State<DesktopHomePage>
    with AutomaticKeepAliveClientMixin, WidgetsBindingObserver {
  static const double _leftPaneWidth = 260.0;

  final _leftPaneScrollController = ScrollController();

  @override
  bool get wantKeepAlive => true;
  StreamSubscription? _uniLinksSubscription;
  var svcStopped = false.obs;
  Timer? _updateTimer;

  final RxBool _block = false.obs;
  final RxList<String> _localIps = <String>[].obs;
  late final String _hostname = _loadHostname();

  @override
  Widget build(BuildContext context) {
    super.build(context);
    return _buildBlock(
        child: Column(
      children: [
        Expanded(
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              buildLeftPane(context),
              const VerticalDivider(width: 1),
              Expanded(child: buildRightPane(context)),
            ],
          ),
        ),
        const Divider(height: 1),
        OnlineStatusWidget(localIps: _localIps),
      ],
    ));
  }

  Widget _buildBlock({required Widget child}) {
    return buildRemoteBlock(
        block: _block, mask: true, use: canBeBlocked, child: child);
  }

  static String _loadHostname() {
    try {
      final info = jsonDecode(bind.mainGetLoginDeviceInfo());
      final name = info['name'];
      if (name is String) return name;
    } catch (e) {
      debugPrint('failed to read the computer name: $e');
    }
    return '';
  }

  /// "This computer" panel: name, ID, LAN IPs and password.
  Widget buildLeftPane(BuildContext context) {
    return ChangeNotifierProvider.value(
      value: gFFI.serverModel,
      child: Container(
        width: _leftPaneWidth,
        color: Theme.of(context).colorScheme.background,
        child: SingleChildScrollView(
          controller: _leftPaneScrollController,
          padding: const EdgeInsets.only(left: 20, right: 16, bottom: 16),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              buildPresetPasswordWarning(),
              Align(alignment: Alignment.center, child: loadLogo()),
              Text(
                translate('This computer'),
                style: Theme.of(context).textTheme.titleLarge,
              ).marginOnly(top: 16, bottom: 12),
              buildNameBoard(context),
              buildIpBoard(context),
              buildPasswordBoard(context),
            ],
          ),
        ),
      ),
    );
  }

  buildRightPane(BuildContext context) {
    return Container(
      color: Theme.of(context).scaffoldBackgroundColor,
      child: ConnectionPage(),
    );
  }

  TextStyle? _labelStyle(BuildContext context) => TextStyle(
      fontSize: 13,
      color: Theme.of(context).textTheme.titleLarge?.color?.withOpacity(0.6));

  /// A labeled block with the accent bar on the left, as in the old ID board.
  Widget _board(BuildContext context,
      {required String label, required Widget child}) {
    return Container(
      padding: const EdgeInsets.only(left: 8),
      decoration: BoxDecoration(
        border: Border(
          left: BorderSide(color: MyTheme.color(context).accent, width: 2),
        ),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(label, style: _labelStyle(context)),
          child,
        ],
      ),
    ).marginOnly(bottom: 16);
  }

  Widget _copyButton(BuildContext context, String Function() text) {
    final color =
        Theme.of(context).textTheme.titleLarge?.color?.withOpacity(0.6);
    return IconButton(
      tooltip: translate('Copy'),
      visualDensity: VisualDensity.compact,
      iconSize: 18,
      splashRadius: 16,
      icon: Icon(Icons.copy_rounded, color: color),
      onPressed: () {
        Clipboard.setData(ClipboardData(text: text()));
        showToast(translate('Copied'));
      },
    );
  }

  /// Computer name, with the ID as a secondary line.
  Widget buildNameBoard(BuildContext context) {
    final name = _hostname;
    return _board(
      context,
      label: translate('Computer name'),
      child: Consumer<ServerModel>(
        builder: (context, model, child) {
          final id = model.serverId.text;
          return Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  Expanded(
                    child: SelectableText(
                      name.isEmpty ? '-' : name,
                      maxLines: 1,
                      style: const TextStyle(
                          fontSize: 20, fontWeight: FontWeight.w600),
                    ),
                  ),
                  if (name.isNotEmpty) _copyButton(context, () => name),
                ],
              ),
              Row(
                children: [
                  Expanded(
                    child: Text(
                      '${translate('ID')}: ${id.isEmpty ? '-' : formatID(id)}',
                      overflow: TextOverflow.ellipsis,
                      style: _labelStyle(context),
                    ),
                  ),
                  if (id.isNotEmpty)
                    _copyButton(context, () => trimID(model.serverId.text)),
                ],
              ),
            ],
          );
        },
      ),
    );
  }

  Widget buildIpBoard(BuildContext context) {
    return _board(
      context,
      label: translate('IP address'),
      child: Obx(() {
        final ips = _localIps.toList();
        if (ips.isEmpty) {
          return Text(translate('Not connected to a network'),
                  style: const TextStyle(fontSize: 15))
              .marginOnly(top: 4, bottom: 4);
        }
        return Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: ips
              .map((ip) => Row(
                    children: [
                      Expanded(
                        child: SelectableText(ip,
                            maxLines: 1, style: const TextStyle(fontSize: 16)),
                      ),
                      _copyButton(context, () => ip),
                    ],
                  ))
              .toList(),
        );
      }),
    );
  }

  buildPasswordBoard(BuildContext context) {
    return Consumer<ServerModel>(
      builder: (context, model, child) {
        return buildPasswordBoard2(context, model);
      },
    );
  }

  buildPasswordBoard2(BuildContext context, ServerModel model) {
    RxBool refreshHover = false.obs;
    RxBool editHover = false.obs;
    final textColor = Theme.of(context).textTheme.titleLarge?.color;
    final idleColor = textColor?.withOpacity(0.4);
    return _board(
      context,
      label: translate('One-time Password'),
      child: Row(
        children: [
          Expanded(
            child: GestureDetector(
              onDoubleTap: () {
                Clipboard.setData(ClipboardData(text: model.serverPasswd.text));
                showToast(translate('Copied'));
              },
              child: TextFormField(
                controller: model.serverPasswd,
                readOnly: true,
                decoration: InputDecoration(
                  border: InputBorder.none,
                  filled: false,
                  isDense: true,
                  contentPadding: EdgeInsets.only(top: 8, bottom: 8),
                ),
                style: TextStyle(fontSize: 18),
              ),
            ),
          ),
          _copyButton(context, () => model.serverPasswd.text),
          AnimatedRotationWidget(
            onPressed: () => bind.mainUpdateTemporaryPassword(),
            child: Tooltip(
              message: translate('Refresh Password'),
              child: Obx(() => RotatedBox(
                  quarterTurns: 2,
                  child: Icon(
                    Icons.refresh,
                    color: refreshHover.value ? textColor : idleColor,
                    size: 20,
                  ))),
            ),
            onHover: (value) => refreshHover.value = value,
          ).marginOnly(left: 4, right: 4),
          if (!bind.isDisableSettings())
            InkWell(
              child: Tooltip(
                message: translate('Change Password'),
                child: Obx(
                  () => Icon(
                    Icons.edit,
                    color: editHover.value ? textColor : idleColor,
                    size: 20,
                  ).marginOnly(left: 4),
                ),
              ),
              onTap: () => DesktopSettingPage.switch2page(SettingsTabKey.safety),
              onHover: (value) => editHover.value = value,
            ),
        ],
      ),
    );
  }

  @override
  void initState() {
    super.initState();
    bind.mainDiscover();
    _updateTimer = periodic_immediate(const Duration(seconds: 1), () async {
      await gFFI.serverModel.fetchID();
      final v = await mainGetBoolOption(kOptionStopService);
      if (v != svcStopped.value) {
        svcStopped.value = v;
        setState(() {});
      }
    });
    Get.put<RxBool>(svcStopped, tag: 'stop-service');
    rustDeskWinManager.registerActiveWindowListener(onActiveWindowChanged);

    screenToMap(window_size.Screen screen) => {
          'frame': {
            'l': screen.frame.left,
            't': screen.frame.top,
            'r': screen.frame.right,
            'b': screen.frame.bottom,
          },
          'visibleFrame': {
            'l': screen.visibleFrame.left,
            't': screen.visibleFrame.top,
            'r': screen.visibleFrame.right,
            'b': screen.visibleFrame.bottom,
          },
          'scaleFactor': screen.scaleFactor,
        };

    bool isChattyMethod(String methodName) {
      switch (methodName) {
        case kWindowBumpMouse: return true;
      }

      return false;
    }

    rustDeskWinManager.setMethodHandler((call, fromWindowId) async {
      if (!isChattyMethod(call.method)) {
        debugPrint(
          "[Main] call ${call.method} with args ${call.arguments} from window $fromWindowId");
      }
      if (call.method == kWindowMainWindowOnTop) {
        windowOnTop(null);
      } else if (call.method == kWindowGetScreenList) {
        return jsonEncode(
            (await window_size.getScreenList()).map(screenToMap).toList());
      } else if (call.method == kWindowActionRebuild) {
        reloadCurrentWindow();
      } else if (call.method == kWindowEventShow) {
        await rustDeskWinManager.registerActiveWindow(call.arguments["id"]);
      } else if (call.method == kWindowEventHide) {
        await rustDeskWinManager.unregisterActiveWindow(call.arguments['id']);
      } else if (call.method == kWindowConnect) {
        await connectMainDesktop(
          call.arguments['id'],
          isFileTransfer: call.arguments['isFileTransfer'],
          password: call.arguments['password'],
          connToken: call.arguments['connToken'],
        );
      } else if (call.method == kWindowBumpMouse) {
        return RdPlatformChannel.instance.bumpMouse(
          dx: call.arguments['dx'],
          dy: call.arguments['dy']);
      } else if (call.method == kWindowEventMoveTabToNewWindow) {
        final args = call.arguments.split(',');
        int? windowId;
        try {
          windowId = int.parse(args[0]);
        } catch (e) {
          debugPrint("Failed to parse window id '${call.arguments}': $e");
        }
        WindowType? windowType;
        try {
          windowType = WindowType.values.byName(args[3]);
        } catch (e) {
          debugPrint("Failed to parse window type '${call.arguments}': $e");
        }
        if (windowId != null && windowType != null) {
          await rustDeskWinManager.moveTabToNewWindow(
              windowId, args[1], args[2], windowType);
        }
      } else if (call.method == kWindowEventOpenMonitorSession) {
        final args = jsonDecode(call.arguments);
        final windowId = args['window_id'] as int;
        final peerId = args['peer_id'] as String;
        final display = args['display'] as int;
        final displayCount = args['display_count'] as int;
        final windowType = args['window_type'] as int;
        final screenRect = parseParamScreenRect(args);
        await rustDeskWinManager.openMonitorSession(
            windowId, peerId, display, displayCount, screenRect, windowType);
      } else if (call.method == kWindowEventRemoteWindowCoords) {
        final windowId = int.tryParse(call.arguments);
        if (windowId != null) {
          return jsonEncode(
              await rustDeskWinManager.getOtherRemoteWindowCoords(windowId));
        }
      }
    });
    _uniLinksSubscription = listenUniLinks();
    WidgetsBinding.instance.addObserver(this);
  }

  @override
  void dispose() {
    _uniLinksSubscription?.cancel();
    Get.delete<RxBool>(tag: 'stop-service');
    _updateTimer?.cancel();
    WidgetsBinding.instance.removeObserver(this);
    super.dispose();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    super.didChangeAppLifecycleState(state);
    if (state == AppLifecycleState.resumed) {
      shouldBeBlocked(_block, canBeBlocked);
    }
  }
}

void setPasswordDialog() async {
  final p0 = TextEditingController(text: "");
  final p1 = TextEditingController(text: "");
  var errMsg0 = "";
  var errMsg1 = "";
  final localPasswordSet =
      (await bind.mainGetCommon(key: "local-permanent-password-set")) == "true";
  final permanentPasswordSet =
      (await bind.mainGetCommon(key: "permanent-password-set")) == "true";
  final presetPassword = permanentPasswordSet && !localPasswordSet;
  var canSubmit = false;
  final RxString rxPass = "".obs;
  final rules = [
    DigitValidationRule(),
    UppercaseValidationRule(),
    LowercaseValidationRule(),
    // SpecialCharacterValidationRule(),
    MinCharactersValidationRule(8),
  ];
  final maxLength = bind.mainMaxEncryptLen();
  final statusTip = localPasswordSet
      ? translate('password-hidden-tip')
      : (presetPassword ? translate('preset-password-in-use-tip') : '');

  gFFI.dialogManager.show((setState, close, context) {
    updateCanSubmit() {
      canSubmit = p0.text.trim().isNotEmpty || p1.text.trim().isNotEmpty;
    }

    submit() async {
      if (!canSubmit) {
        return;
      }
      setState(() {
        errMsg0 = "";
        errMsg1 = "";
      });
      final pass = p0.text.trim();
      if (pass.isNotEmpty) {
        final Iterable violations = rules.where((r) => !r.validate(pass));
        if (violations.isNotEmpty) {
          setState(() {
            errMsg0 =
                '${translate('Prompt')}: ${violations.map((r) => r.name).join(', ')}';
          });
          return;
        }
      }
      if (p1.text.trim() != pass) {
        setState(() {
          errMsg1 =
              '${translate('Prompt')}: ${translate("The confirmation is not identical.")}';
        });
        return;
      }
      final ok = await bind.mainSetPermanentPasswordWithResult(password: pass);
      if (!ok) {
        setState(() {
          errMsg0 = '${translate('Prompt')}: ${translate("Failed")}';
        });
        return;
      }
      close();
    }

    return CustomAlertDialog(
      title: Row(
        mainAxisAlignment: MainAxisAlignment.center,
        children: [
          Icon(Icons.key, color: MyTheme.color(context).accent),
          Text(translate("Set Password")).paddingOnly(left: 10),
        ],
      ),
      content: ConstrainedBox(
        constraints: const BoxConstraints(minWidth: 500),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            SizedBox(
              height: 6.0,
            ),
            Row(
              children: [
                Expanded(
                  child: TextField(
                    obscureText: true,
                    decoration: InputDecoration(
                        labelText: translate('Password'),
                        errorText: errMsg0.isNotEmpty ? errMsg0 : null),
                    controller: p0,
                    autofocus: true,
                    onChanged: (value) {
                      rxPass.value = value.trim();
                      setState(() {
                        errMsg0 = '';
                        updateCanSubmit();
                      });
                    },
                    maxLength: maxLength,
                  ).workaroundFreezeLinuxMint(),
                ),
              ],
            ),
            Row(
              children: [
                Expanded(child: PasswordStrengthIndicator(password: rxPass)),
              ],
            ).marginOnly(top: 2, bottom: 8),
            SizedBox(
              height: 8.0,
            ),
            Row(
              children: [
                Expanded(
                  child: TextField(
                    obscureText: true,
                    decoration: InputDecoration(
                        labelText: translate('Confirmation'),
                        errorText: errMsg1.isNotEmpty ? errMsg1 : null),
                    controller: p1,
                    onChanged: (value) {
                      setState(() {
                        errMsg1 = '';
                        updateCanSubmit();
                      });
                    },
                    maxLength: maxLength,
                  ).workaroundFreezeLinuxMint(),
                ),
              ],
            ),
            if (statusTip.isNotEmpty)
              Row(
                children: [
                  Icon(Icons.info, color: Colors.amber, size: 18)
                      .marginOnly(right: 6),
                  Expanded(
                      child: Text(
                    statusTip,
                    style: const TextStyle(fontSize: 13, height: 1.1),
                  ))
                ],
              ).marginOnly(top: 6, bottom: 2),
            SizedBox(
              height: 8.0,
            ),
            Obx(() => Wrap(
                  runSpacing: 8.0,
                  spacing: 4,
                  children: rules.map((e) {
                    var checked = e.validate(rxPass.value.trim());
                    return Chip(
                        label: Text(
                          e.name,
                          style: TextStyle(
                              color: checked
                                  ? const Color(0xFF0A9471)
                                  : Color.fromARGB(255, 198, 86, 157)),
                        ),
                        backgroundColor: checked
                            ? const Color(0xFFD0F7ED)
                            : Color.fromARGB(255, 247, 205, 232));
                  }).toList(),
                ))
          ],
        ),
      ),
      actions: (() {
        final cancelButton = dialogButton(
          "Cancel",
          icon: Icon(Icons.close_rounded),
          onPressed: close,
          isOutline: true,
        );
        final removeButton = dialogButton(
          "Remove",
          icon: Icon(Icons.delete_outline_rounded),
          onPressed: () async {
            setState(() {
              errMsg0 = "";
              errMsg1 = "";
            });
            final ok =
                await bind.mainSetPermanentPasswordWithResult(password: "");
            if (!ok) {
              setState(() {
                errMsg0 = '${translate('Prompt')}: ${translate("Failed")}';
              });
              return;
            }
            close();
          },
          buttonStyle: ButtonStyle(
              backgroundColor: MaterialStatePropertyAll(Colors.red)),
        );
        final okButton = dialogButton(
          "OK",
          icon: Icon(Icons.done_rounded),
          onPressed: canSubmit ? submit : null,
        );
        return [
          cancelButton,
          if (localPasswordSet) removeButton,
          okButton,
        ];
      })(),
      onSubmit: canSubmit ? submit : null,
      onCancel: close,
    );
  });
}
