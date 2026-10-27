import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_hbb/common.dart';
import 'package:flutter_hbb/common/widgets/audio_input.dart';
import 'package:flutter_hbb/common/widgets/setting_widgets.dart';
import 'package:flutter_hbb/consts.dart';
import 'package:flutter_hbb/desktop/pages/desktop_home_page.dart';
import 'package:flutter_hbb/common/widgets/keyboard_shortcuts/page_body.dart';
import 'package:flutter_hbb/desktop/pages/desktop_keyboard_shortcuts_page.dart';
import 'package:flutter_hbb/desktop/pages/desktop_tab_page.dart';
import 'package:flutter_hbb/models/platform_model.dart';
import 'package:flutter_hbb/models/server_model.dart';
import 'package:flutter_hbb/models/shortcut_model.dart';
import 'package:flutter_hbb/models/state_model.dart';
import 'package:get/get.dart';
import 'package:provider/provider.dart';
import 'package:url_launcher/url_launcher_string.dart';

import '../../common/widgets/dialog.dart';

const double _kTabWidth = 200;
const double _kTabHeight = 42;
const double _kCardFixedWidth = 540;
const double _kCardLeftMargin = 15;
const double _kContentHMargin = 15;
const double _kContentHSubMargin = _kContentHMargin + 33;
const double _kCheckBoxLeftMargin = 10;
const double _kRadioLeftMargin = 10;
const double _kListViewBottomMargin = 15;
const double _kTitleFontSize = 20;
const double _kContentFontSize = 15;
const String _kSettingPageControllerTag = 'settingPageController';
const String _kSettingPageTabKeyTag = 'settingPageTabKey';

class _TabInfo {
  late final SettingsTabKey key;
  late final String label;
  late final IconData unselected;
  late final IconData selected;
  _TabInfo(this.key, this.label, this.unselected, this.selected);
}

enum SettingsTabKey {
  general,
  safety,
  network,
  display,
  audio,
  about,
}

class DesktopSettingPage extends StatefulWidget {
  final SettingsTabKey initialTabkey;
  static final List<SettingsTabKey> tabKeys = [
    if (bind.mainGetBuildinOption(key: kOptionHideGeneralSetting) != 'Y')
      SettingsTabKey.general,
    if (!bind.isOutgoingOnly() && !bind.isDisableSettings() && bind.mainGetBuildinOption(key: kOptionHideSecuritySetting) != 'Y')
      SettingsTabKey.safety,
    SettingsTabKey.network,
    if (!bind.isIncomingOnly()) SettingsTabKey.display,
    if (!bind.isOutgoingOnly()) SettingsTabKey.audio,
    SettingsTabKey.about,
  ];

  DesktopSettingPage({Key? key, required this.initialTabkey}) : super(key: key);

  @override
  State<DesktopSettingPage> createState() =>
      _DesktopSettingPageState(initialTabkey);

  static void switch2page(SettingsTabKey page) {
    try {
      int index = tabKeys.indexOf(page);
      if (index == -1) {
        return;
      }
      if (Get.isRegistered<PageController>(tag: _kSettingPageControllerTag) &&
          Get.isRegistered<Rx<SettingsTabKey>>(tag: _kSettingPageTabKeyTag)) {
        DesktopTabPage.onAddSetting(initialPage: page);
        PageController controller =
            Get.find<PageController>(tag: _kSettingPageControllerTag);
        Rx<SettingsTabKey> selected =
            Get.find<Rx<SettingsTabKey>>(tag: _kSettingPageTabKeyTag);
        selected.value = page;
        controller.jumpToPage(index);
      } else {
        DesktopTabPage.onAddSetting(initialPage: page);
      }
    } catch (e) {
      debugPrintStack(label: '$e');
    }
  }
}

class _DesktopSettingPageState extends State<DesktopSettingPage>
    with
        TickerProviderStateMixin,
        AutomaticKeepAliveClientMixin,
        WidgetsBindingObserver {
  late PageController controller;
  late Rx<SettingsTabKey> selectedTab;

  @override
  bool get wantKeepAlive => true;

  final RxBool _block = false.obs;
  final RxBool _canBeBlocked = false.obs;
  Timer? _videoConnTimer;

  _DesktopSettingPageState(SettingsTabKey initialTabkey) {
    var initialIndex = DesktopSettingPage.tabKeys.indexOf(initialTabkey);
    if (initialIndex == -1) {
      initialIndex = 0;
    }
    selectedTab = DesktopSettingPage.tabKeys[initialIndex].obs;
    Get.put<Rx<SettingsTabKey>>(selectedTab, tag: _kSettingPageTabKeyTag);
    controller = PageController(initialPage: initialIndex);
    Get.put<PageController>(controller, tag: _kSettingPageControllerTag);
    controller.addListener(() {
      if (controller.page != null) {
        int page = controller.page!.toInt();
        if (page < DesktopSettingPage.tabKeys.length) {
          selectedTab.value = DesktopSettingPage.tabKeys[page];
        }
      }
    });
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    super.didChangeAppLifecycleState(state);
    if (state == AppLifecycleState.resumed) {
      shouldBeBlocked(_block, canBeBlocked);
    }
  }

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    _videoConnTimer =
        periodic_immediate(Duration(milliseconds: 1000), () async {
      if (!mounted) {
        return;
      }
      final blocked = await canBeBlocked();
      if (!mounted) {
        return;
      }
      _canBeBlocked.value = blocked;
    });
  }

  @override
  void dispose() {
    _videoConnTimer?.cancel();
    WidgetsBinding.instance.removeObserver(this);
    Get.delete<PageController>(tag: _kSettingPageControllerTag);
    Get.delete<Rx<SettingsTabKey>>(tag: _kSettingPageTabKeyTag);
    // Get.delete does not dispose a plain ChangeNotifier.
    controller.dispose();
    super.dispose();
  }

  List<_TabInfo> _settingTabs() {
    final List<_TabInfo> settingTabs = <_TabInfo>[];
    for (final tab in DesktopSettingPage.tabKeys) {
      switch (tab) {
        case SettingsTabKey.general:
          settingTabs.add(_TabInfo(
              tab, 'General', Icons.settings_outlined, Icons.settings));
          break;
        case SettingsTabKey.safety:
          settingTabs.add(_TabInfo(tab, 'Security',
              Icons.enhanced_encryption_outlined, Icons.enhanced_encryption));
          break;
        case SettingsTabKey.network:
          settingTabs.add(
              _TabInfo(tab, 'Network', Icons.lan_outlined, Icons.lan));
          break;
        case SettingsTabKey.display:
          settingTabs.add(_TabInfo(tab, 'Display',
              Icons.desktop_windows_outlined, Icons.desktop_windows));
          break;
        case SettingsTabKey.audio:
          settingTabs.add(_TabInfo(
              tab, 'Audio', Icons.volume_up_outlined, Icons.volume_up));
          break;
        case SettingsTabKey.about:
          settingTabs
              .add(_TabInfo(tab, 'About', Icons.info_outline, Icons.info));
          break;
      }
    }
    return settingTabs;
  }

  List<Widget> _children() {
    final children = List<Widget>.empty(growable: true);
    for (final tab in DesktopSettingPage.tabKeys) {
      switch (tab) {
        case SettingsTabKey.general:
          children.add(const _General());
          break;
        case SettingsTabKey.safety:
          children.add(const _Safety());
          break;
        case SettingsTabKey.network:
          children.add(const _Network());
          break;
        case SettingsTabKey.display:
          children.add(const _Display());
          break;
        case SettingsTabKey.audio:
          children.add(const _Audio());
          break;
        case SettingsTabKey.about:
          children.add(const _About());
          break;
      }
    }
    return children;
  }

  Widget _buildBlock({required List<Widget> children}) {
    // check both mouseMoveTime and videoConnCount
    return Obx(() {
      final videoConnBlock =
          _canBeBlocked.value && stateGlobal.videoConnCount > 0;
      return Stack(children: [
        buildRemoteBlock(
          block: _block,
          mask: false,
          use: canBeBlocked,
          child: preventMouseKeyBuilder(
            child: Row(children: children),
            block: videoConnBlock,
          ),
        ),
        if (videoConnBlock)
          Container(
            color: Colors.black.withOpacity(0.5),
          )
      ]);
    });
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    return Scaffold(
      backgroundColor: Theme.of(context).colorScheme.background,
      body: _buildBlock(
        children: <Widget>[
          SizedBox(
            width: _kTabWidth,
            child: Column(
              children: [
                _header(context),
                Flexible(child: _listView(tabs: _settingTabs())),
              ],
            ),
          ),
          const VerticalDivider(width: 1),
          Expanded(
            child: Container(
              color: Theme.of(context).scaffoldBackgroundColor,
              child: PageView(
                controller: controller,
                physics: NeverScrollableScrollPhysics(),
                children: _children(),
              ),
            ),
          )
        ],
      ),
    );
  }

  Widget _header(BuildContext context) {
    final settingsText = Text(
      translate('Settings'),
      textAlign: TextAlign.left,
      style: TextStyle(
        color: MyTheme.color(context).accent,
        fontSize: _kTitleFontSize,
        fontWeight: FontWeight.w400,
      ),
    );
    return Row(
      children: [
        SizedBox(
            height: 62,
            child: settingsText,
          ).marginOnly(left: 20, top: 10),
        const Spacer(),
      ],
    );
  }

  Widget _listView({required List<_TabInfo> tabs}) {
    final scrollController = ScrollController();
    return ListView(
      controller: scrollController,
      children: tabs.map((tab) => _listItem(tab: tab)).toList(),
    );
  }

  Widget _listItem({required _TabInfo tab}) {
    return Obx(() {
      bool selected = tab.key == selectedTab.value;
      return SizedBox(
        width: _kTabWidth,
        height: _kTabHeight,
        child: InkWell(
          onTap: () {
            if (selectedTab.value != tab.key) {
              int index = DesktopSettingPage.tabKeys.indexOf(tab.key);
              if (index == -1) {
                return;
              }
              controller.jumpToPage(index);
            }
            selectedTab.value = tab.key;
          },
          child: Row(children: [
            Container(
              width: 4,
              height: _kTabHeight * 0.7,
              color: selected ? MyTheme.color(context).accent : null,
            ),
            Icon(
              selected ? tab.selected : tab.unselected,
              color: selected ? MyTheme.color(context).accent : null,
              size: 20,
            ).marginOnly(left: 13, right: 10),
            Text(
              translate(tab.label),
              style: TextStyle(
                  color: selected ? MyTheme.color(context).accent : null,
                  fontWeight: FontWeight.w400,
                  fontSize: _kContentFontSize),
            ),
          ]),
        ),
      );
    });
  }
}

//#region pages

class _General extends StatefulWidget {
  const _General({Key? key}) : super(key: key);

  @override
  State<_General> createState() => _GeneralState();
}

class _GeneralState extends State<_General> {
  @override
  Widget build(BuildContext context) {
    final scrollController = ScrollController();
    return ListView(
      controller: scrollController,
      children: [
        theme(),
        _Card(title: 'Language', children: [language()]),
        if (!bind.isIncomingOnly()) keyboardShortcuts(),
      ],
    ).marginOnly(bottom: _kListViewBottomMargin);
  }

  Widget keyboardShortcuts() {
    // The bindings JSON (LocalConfig key `keyboard-shortcuts`) holds three
    // flags + the bindings list: {enabled, pass_through, bindings}. When the
    // master is off, the pass-through toggle and the Configure entry are
    // hidden — both are meaningless without an active matcher.
    return StatefulBuilder(builder: (context, setLocalState) {
      final enabled = ShortcutModel.isEnabled();
      return _Card(title: 'Keyboard shortcuts', children: [
        _OptionCheckBox(
          context,
          'Enable keyboard shortcuts in remote session',
          kShortcutLocalConfigKey,
          isServer: false,
          optGetter: ShortcutModel.isEnabled,
          optSetter: (_, v) async {
            await ShortcutModel.setEnabled(v);
            setLocalState(() {});
          },
        ),
        if (enabled) ...[
          _OptionCheckBox(
            context,
            'Pass-through to remote',
            kShortcutLocalConfigKey,
            isServer: false,
            optGetter: ShortcutModel.isPassThrough,
            optSetter: (_, v) async {
              await ShortcutModel.setPassThrough(v);
              setLocalState(() {});
            },
            trailing: const InfoTooltipIcon(tipKey: 'shortcut-passthrough-tip'),
          ),
          _ShortcutsConfigureRow(),
        ],
      ]);
    });
  }

  Widget theme() {
    final current = MyTheme.getThemeModePreference().toShortString();
    onChanged(String value) async {
      await MyTheme.changeDarkMode(MyTheme.themeModeFromString(value));
      setState(() {});
    }

    final isOptFixed = isOptionFixed(kCommConfKeyTheme);
    return _Card(title: 'Theme', children: [
      _Radio<String>(context,
          value: 'light',
          groupValue: current,
          label: 'Light',
          onChanged: isOptFixed ? null : onChanged),
      _Radio<String>(context,
          value: 'dark',
          groupValue: current,
          label: 'Dark',
          onChanged: isOptFixed ? null : onChanged),
      _Radio<String>(context,
          value: 'system',
          groupValue: current,
          label: 'Follow System',
          onChanged: isOptFixed ? null : onChanged),
    ]);
  }

  Widget language() {
    return futureBuilder(future: () async {
      String langs = await bind.mainGetLangs();
      return {'langs': langs};
    }(), hasData: (res) {
      Map<String, String> data = res as Map<String, String>;
      List<dynamic> langsList = jsonDecode(data['langs']!);
      Map<String, String> langsMap = {for (var v in langsList) v[0]: v[1]};
      List<String> keys = langsMap.keys.toList();
      List<String> values = langsMap.values.toList();
      keys.insert(0, defaultOptionLang);
      values.insert(0, translate('Default'));
      String currentKey = bind.mainGetLocalOption(key: kCommConfKeyLang);
      if (!keys.contains(currentKey)) {
        currentKey = defaultOptionLang;
      }
      final isOptFixed = isOptionFixed(kCommConfKeyLang);
      return ComboBox(
        keys: keys,
        values: values,
        initialKey: currentKey,
        onChanged: (key) async {
          await bind.mainSetLocalOption(key: kCommConfKeyLang, value: key);
          reloadAllWindows();
          bind.mainChangeLanguage(lang: key);
        },
        enabled: !isOptFixed,
      ).marginOnly(left: _kContentHMargin);
    });
  }

}

enum _AccessMode {
  custom,
  full,
  view,
}

class _Safety extends StatefulWidget {
  const _Safety({Key? key}) : super(key: key);

  @override
  State<_Safety> createState() => _SafetyState();
}

class _SafetyState extends State<_Safety> with AutomaticKeepAliveClientMixin {
  @override
  bool get wantKeepAlive => true;
  bool locked = false;
  final scrollController = ScrollController();

  @override
  Widget build(BuildContext context) {
    super.build(context);
    return SingleChildScrollView(
        controller: scrollController,
        child: Column(
          children: [
            _lock(locked, 'Unlock Security Settings', () {
              locked = false;
              setState(() => {});
            }),
            preventMouseKeyBuilder(
              block: locked,
              child: Column(children: [
                permissions(context),
                password(context),
              ]),
            ),
          ],
        )).marginOnly(bottom: _kListViewBottomMargin);
  }

  Widget permissions(context) {
    bool enabled = !locked;
    // Simple temp wrapper for PR check
    tmpWrapper() {
      String accessMode = bind.mainGetOptionSync(key: kOptionAccessMode);
      _AccessMode mode;
      if (accessMode == 'full') {
        mode = _AccessMode.full;
      } else if (accessMode == 'view') {
        mode = _AccessMode.view;
      } else {
        mode = _AccessMode.custom;
      }
      String initialKey;
      bool? fakeValue;
      switch (mode) {
        case _AccessMode.custom:
          initialKey = '';
          fakeValue = null;
          break;
        case _AccessMode.full:
          initialKey = 'full';
          fakeValue = true;
          break;
        case _AccessMode.view:
          initialKey = 'view';
          fakeValue = false;
          break;
      }

      return _Card(title: 'Permissions', children: [
        ComboBox(
            keys: [
              defaultOptionAccessMode,
              'full',
              'view',
            ],
            values: [
              translate('Custom'),
              translate('Full Access'),
              translate('Screen Share'),
            ],
            enabled: enabled && !isOptionFixed(kOptionAccessMode),
            initialKey: initialKey,
            onChanged: (mode) async {
              await bind.mainSetOption(key: kOptionAccessMode, value: mode);
              setState(() {});
            }).marginOnly(left: _kContentHMargin),
        Column(
          children: [
            _OptionCheckBox(
                context, 'Enable keyboard/mouse', kOptionEnableKeyboard,
                enabled: enabled, fakeValue: fakeValue),
            _OptionCheckBox(context, 'Enable clipboard', kOptionEnableClipboard,
                enabled: enabled, fakeValue: fakeValue),
            _OptionCheckBox(
                context, 'Enable file transfer', kOptionEnableFileTransfer,
                enabled: enabled, fakeValue: fakeValue),
            _OptionCheckBox(context, 'Enable audio', kOptionEnableAudio,
                enabled: enabled, fakeValue: fakeValue),
            _OptionCheckBox(
                context, 'Enable remote restart', kOptionEnableRemoteRestart,
                enabled: enabled, fakeValue: fakeValue),
            _OptionCheckBox(context, 'Enable remote configuration modification',
                kOptionAllowRemoteConfigModification,
                enabled: enabled, fakeValue: fakeValue),
          ],
        ),
      ]);
    }

    return tmpWrapper();
  }

  Widget password(BuildContext context) {
    return ChangeNotifierProvider.value(
        value: gFFI.serverModel,
        child: Consumer<ServerModel>(builder: ((context, model, child) {
          var onChanged = !locked
              ? (value) {
                  if (value != null) {
                    () async {
                      await model.setTemporaryPasswordLength(value.toString());
                      await model.updatePasswordModel();
                    }();
                  }
                }
              : null;
          List<Widget> lengthRadios = ['6', '8', '10']
              .map((value) => GestureDetector(
                    child: Row(
                      children: [
                        Radio(
                            value: value,
                            groupValue: model.temporaryPasswordLength,
                            onChanged: onChanged),
                        Text(
                          value,
                          style: TextStyle(
                              color: disabledTextColor(
                                  context, onChanged != null)),
                        ),
                      ],
                    ).paddingOnly(right: 10),
                    onTap: () => onChanged?.call(value),
                  ))
              .toList();

          final isOptFixedNumOTP =
              isOptionFixed(kOptionAllowNumericOneTimePassword);
          final isNumOPTChangable = !isOptFixedNumOTP && !locked;
          final numericOneTimePassword = GestureDetector(
            child: InkWell(
                child: Row(
              children: [
                Checkbox(
                        value: model.allowNumericOneTimePassword,
                        onChanged: isNumOPTChangable
                            ? (bool? v) {
                                model.switchAllowNumericOneTimePassword();
                              }
                            : null)
                    .marginOnly(right: 5),
                Expanded(
                    child: Text(
                  translate('Numeric one-time password'),
                  style: TextStyle(
                      color: disabledTextColor(context, isNumOPTChangable)),
                ))
              ],
            )),
            onTap: isNumOPTChangable
                ? () => model.switchAllowNumericOneTimePassword()
                : null,
          ).marginOnly(left: _kContentHSubMargin - 5);

          // The one-time password always works; the permanent one does once it is set.
          return _Card(title: 'Password', children: [
            _SubLabeledWidget(
                context,
                'One-time password length',
                Row(
                  children: [
                    ...lengthRadios,
                  ],
                ),
                enabled: !locked),
            numericOneTimePassword,
            if (!isChangePermanentPasswordDisabled())
              _SubButton('Set permanent password', setPasswordDialog, !locked),
          ]);
        })));
  }

}

class _Network extends StatefulWidget {
  const _Network({Key? key}) : super(key: key);

  @override
  State<_Network> createState() => _NetworkState();
}

class _NetworkState extends State<_Network> {
  @override
  Widget build(BuildContext context) {
    final scrollController = ScrollController();
    return ListView(controller: scrollController, children: [
      _Card(title: 'Network', children: [
        _OptionCheckBox(
            context, 'Deny LAN discovery', 'enable-lan-discovery',
            reverse: true),
        ...directIp(context),
      ]),
      _Card(title: 'Windows Firewall', children: [
        _SubButton('Allow in firewall', () => bind.mainAllowInFirewall()),
      ]),
    ]).marginOnly(bottom: _kListViewBottomMargin);
  }

  List<Widget> directIp(BuildContext context) {
    TextEditingController controller = TextEditingController();
    RxBool applyEnabled = false.obs;
    controller.text = bind.mainGetOptionSync(key: kOptionDirectAccessPort);
    final isOptFixed = isOptionFixed(kOptionDirectAccessPort);
    final enabled = !isOptFixed;
    return [
      _SubLabeledWidget(
        context,
        'Port',
        Row(children: [
          SizedBox(
            width: 95,
            child: TextField(
              controller: controller,
              enabled: enabled,
              onChanged: (_) => applyEnabled.value = true,
              inputFormatters: [
                FilteringTextInputFormatter.allow(RegExp(
                    r'^([0-9]|[1-9]\d|[1-9]\d{2}|[1-9]\d{3}|[1-5]\d{4}|6[0-4]\d{3}|65[0-4]\d{2}|655[0-2]\d|6553[0-5])$')),
              ],
              decoration: const InputDecoration(
                hintText: '21118',
                contentPadding:
                    EdgeInsets.symmetric(vertical: 12, horizontal: 12),
              ),
            ).workaroundFreezeLinuxMint().marginOnly(right: 15),
          ),
          Obx(() => ElevatedButton(
                onPressed: applyEnabled.value && enabled
                    ? () async {
                        applyEnabled.value = false;
                        await bind.mainSetOption(
                            key: kOptionDirectAccessPort,
                            value: controller.text);
                      }
                    : null,
                child: Text(
                  translate('Apply'),
                ),
              ))
        ]),
        enabled: enabled,
      ),
    ];
  }
}

class _Display extends StatefulWidget {
  const _Display({Key? key}) : super(key: key);

  @override
  State<_Display> createState() => _DisplayState();
}

class _DisplayState extends State<_Display> {
  @override
  Widget build(BuildContext context) {
    final scrollController = ScrollController();
    return ListView(controller: scrollController, children: [
      viewStyle(context),
      imageQuality(context),
      hwcodec(),
      other(context),
    ]).marginOnly(bottom: _kListViewBottomMargin);
  }

  Widget viewStyle(BuildContext context) {
    final isOptFixed = isOptionFixed(kOptionViewStyle);
    onChanged(String value) async {
      await bind.mainSetUserDefaultOption(key: kOptionViewStyle, value: value);
      setState(() {});
    }

    final groupValue = bind.mainGetUserDefaultOption(key: kOptionViewStyle);
    return _Card(title: 'Default View Style', children: [
      _Radio(context,
          value: kRemoteViewStyleOriginal,
          groupValue: groupValue,
          label: 'Scale original',
          onChanged: isOptFixed ? null : onChanged),
      _Radio(context,
          value: kRemoteViewStyleAdaptive,
          groupValue: groupValue,
          label: 'Scale adaptive',
          onChanged: isOptFixed ? null : onChanged),
    ]);
  }

  Widget imageQuality(BuildContext context) {
    onChanged(String value) async {
      await bind.mainSetUserDefaultOption(
          key: kOptionImageQuality, value: value);
      setState(() {});
    }

    final isOptFixed = isOptionFixed(kOptionImageQuality);
    final groupValue = bind.mainGetUserDefaultOption(key: kOptionImageQuality);
    return _Card(title: 'Default Image Quality', children: [
      _Radio(context,
          value: kRemoteImageQualityBest,
          groupValue: groupValue,
          label: 'Good image quality',
          onChanged: isOptFixed ? null : onChanged),
      _Radio(context,
          value: kRemoteImageQualityBalanced,
          groupValue: groupValue,
          label: 'Balanced',
          onChanged: isOptFixed ? null : onChanged),
      _Radio(context,
          value: kRemoteImageQualityLow,
          groupValue: groupValue,
          label: 'Optimize reaction time',
          onChanged: isOptFixed ? null : onChanged),
      _Radio(context,
          value: kRemoteImageQualityCustom,
          groupValue: groupValue,
          label: 'Custom',
          onChanged: isOptFixed ? null : onChanged),
      Offstage(
        offstage: groupValue != kRemoteImageQualityCustom,
        child: customImageQualitySetting(),
      )
    ]);
  }

  Widget hwcodec() {
    final hwcodec = bind.mainHasHwcodec();
    final vram = bind.mainHasVram();
    return Offstage(
      offstage: !(hwcodec || vram),
      child: _Card(title: 'Hardware Codec', children: [
        _OptionCheckBox(
          context,
          'Enable hardware codec',
          kOptionEnableHwcodec,
          update: (bool v) {
            if (v) {
              bind.mainCheckHwcodec();
            }
          },
        )
      ]),
    );
  }

  Widget otherRow(String label, String key) {
    final value = getOtherDefaultSettingOption(key) == 'Y';
    final isOptFixed = isOtherDefaultSettingReadOnly(key);
    onChanged(bool b) async {
      await setOtherDefaultSettingOption(
        key,
        b ? 'Y' : (key == kOptionEnableFileCopyPaste ? 'N' : defaultOptionNo),
      );
      setState(() {});
    }

    return GestureDetector(
        child: Row(
          children: [
            Checkbox(
                    value: value,
                    onChanged: isOptFixed ? null : (_) => onChanged(!value))
                .marginOnly(right: 5),
            Expanded(
              child: Text(translate(label)),
            )
          ],
        ).marginOnly(left: _kCheckBoxLeftMargin),
        onTap: isOptFixed ? null : () => onChanged(!value));
  }

  Widget other(BuildContext context) {
    final children =
        otherDefaultSettings().map((e) => otherRow(e.$1, e.$2)).toList();
    return _Card(title: 'Other Default Options', children: children);
  }
}

class _Audio extends StatefulWidget {
  const _Audio({Key? key}) : super(key: key);

  @override
  State<_Audio> createState() => _AudioState();
}

class _AudioState extends State<_Audio> {
  @override
  Widget build(BuildContext context) {
    final scrollController = ScrollController();
    return ListView(controller: scrollController, children: [
      audio(context),
    ]).marginOnly(bottom: _kListViewBottomMargin);
  }

  Widget audio(BuildContext context) {
    builder(devices, currentDevice, setDevice) {
      final child = ComboBox(
        keys: devices,
        values: devices,
        initialKey: currentDevice,
        onChanged: (key) async {
          setDevice(key);
          setState(() {});
        },
      ).marginOnly(left: _kContentHMargin);
      return _Card(title: 'Audio Input Device', children: [child]);
    }

    return AudioInput(builder: builder);
  }
}

class _About extends StatefulWidget {
  const _About({Key? key}) : super(key: key);

  @override
  State<_About> createState() => _AboutState();
}

class _AboutState extends State<_About> {
  @override
  Widget build(BuildContext context) {
    return futureBuilder(future: () async {
      final version = await bind.mainGetVersion();
      final buildDate = await bind.mainGetBuildDate();
      final fingerprint = await bind.mainGetFingerprint();
      final myId = await bind.mainGetMyId();
      return {
        'version': version,
        'buildDate': buildDate,
        'fingerprint': fingerprint,
        'myId': myId
      };
    }(), hasData: (data) {
      final version = data['version'].toString();
      final buildDate = data['buildDate'].toString();
      final fingerprint = data['fingerprint'].toString();
      final myId = data['myId'].toString();
      const linkStyle = TextStyle(decoration: TextDecoration.underline);
      final scrollController = ScrollController();
      return SingleChildScrollView(
        controller: scrollController,
        child: _Card(title: '${translate('About')} IntraDesk', children: [
          Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              const SizedBox(
                height: 8.0,
              ),
              SelectionArea(
                  child: Text('${translate('Version')}: $version')
                      .marginSymmetric(vertical: 4.0)),
              SelectionArea(
                  child: Text('${translate('Build Date')}: $buildDate')
                      .marginSymmetric(vertical: 4.0)),
              SelectionArea(
                    child: Text('${translate('Fingerprint')}: $fingerprint')
                        .marginSymmetric(vertical: 4.0)),
              SelectionArea(
                  child: Text('${translate('ID')}: $myId')
                      .marginSymmetric(vertical: 4.0)),
              SelectionArea(
                  child: Text(translate('intradesk-license-tip'))
                      .marginSymmetric(vertical: 4.0)),
              InkWell(
                  onTap: () {
                    launchUrlString(kIntraDeskSourceUrl);
                  },
                  child: Text(
                    translate('Source code'),
                    style: linkStyle,
                  ).marginSymmetric(vertical: 4.0)),
              InkWell(
                  onTap: () {
                    launchUrlString('$kIntraDeskSourceUrl/blob/master/LICENCE');
                  },
                  child: Text(
                    translate('License'),
                    style: linkStyle,
                  ).marginSymmetric(vertical: 4.0)),
              Container(
                decoration: BoxDecoration(color: MyTheme.color(context).accent),
                padding:
                    const EdgeInsets.symmetric(vertical: 24, horizontal: 8),
                child: SelectionArea(
                    child: Row(
                  children: [
                    Expanded(
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text(
                            'IntraDesk',
                            style: TextStyle(
                                fontWeight: FontWeight.w800,
                                color: MyTheme.onAccent),
                          ),
                          Text(
                            'RustDesk: Copyright © ${DateTime.now().toString().substring(0, 4)} Purslane Tech Pte. Ltd.',
                            style: TextStyle(color: MyTheme.onAccent),
                          ),
                        ],
                      ),
                    ),
                  ],
                )),
              ).marginSymmetric(vertical: 4.0)
            ],
          ).marginOnly(left: _kContentHMargin)
        ]),
      );
    });
  }
}

//#endregion

//#region components

// ignore: non_constant_identifier_names
Widget _Card(
    {required String title,
    required List<Widget> children,
    List<Widget>? title_suffix}) {
  return Row(
    children: [
      Flexible(
        child: SizedBox(
          width: _kCardFixedWidth,
          child: Card(
            child: Column(
              children: [
                Row(
                  children: [
                    Expanded(
                        child: Text(
                      translate(title),
                      textAlign: TextAlign.start,
                      style: const TextStyle(
                        fontSize: _kTitleFontSize,
                      ),
                    )),
                    ...?title_suffix
                  ],
                ).marginOnly(left: _kContentHMargin, top: 10, bottom: 10),
                ...children
                    .map((e) => e.marginOnly(top: 4, right: _kContentHMargin)),
              ],
            ).marginOnly(bottom: 10),
          ).marginOnly(left: _kCardLeftMargin, top: 15),
        ),
      ),
    ],
  );
}

// ignore: non_constant_identifier_names
Widget _OptionCheckBox(
  BuildContext context,
  String label,
  String key, {
  Function(bool)? update,
  bool reverse = false,
  bool enabled = true,
  Icon? checkedIcon,
  bool? fakeValue,
  bool isServer = true,
  bool Function()? optGetter,
  Future<void> Function(String, bool)? optSetter,
  // Optional widget rendered between the label and the trailing space.
  Widget? trailing,
}) {
  getOpt() => optGetter != null
      ? optGetter()
      : (isServer
          ? mainGetBoolOptionSync(key)
          : mainGetLocalBoolOptionSync(key));
  bool value = getOpt();
  final isOptFixed = isOptionFixed(key);
  if (reverse) value = !value;
  var ref = value.obs;
  onChanged(option) async {
    if (option != null) {
      if (reverse) option = !option;
      final setter =
          optSetter ?? (isServer ? mainSetBoolOption : mainSetLocalBoolOption);
      await setter(key, option);
      final readOption = getOpt();
      if (reverse) {
        ref.value = !readOption;
      } else {
        ref.value = readOption;
      }
      update?.call(readOption);
    }
  }

  if (fakeValue != null) {
    ref.value = fakeValue;
    enabled = false;
  }

  return GestureDetector(
    child: Obx(
      () => Row(
        children: [
          Checkbox(
                  value: ref.value,
                  onChanged: enabled && !isOptFixed ? onChanged : null)
              .marginOnly(right: 5),
          Offstage(
            offstage: !ref.value || checkedIcon == null,
            child: checkedIcon?.marginOnly(right: 5),
          ),
          // Without `trailing`, keep the original Expanded(Text) layout.
          if (trailing == null)
            Expanded(
                child: Text(
              translate(label),
              style: TextStyle(color: disabledTextColor(context, enabled)),
            ))
          else ...[
            Flexible(
              child: Text(
                translate(label),
                style: TextStyle(color: disabledTextColor(context, enabled)),
              ),
            ),
            trailing,
            const Spacer(),
          ],
        ],
      ),
    ).marginOnly(left: _kCheckBoxLeftMargin),
    onTap: enabled && !isOptFixed
        ? () {
            onChanged(!ref.value);
          }
        : null,
  );
}

// ignore: non_constant_identifier_names
Widget _Radio<T>(BuildContext context,
    {required T value,
    required T groupValue,
    required String label,
    required Function(T value)? onChanged,
    bool autoNewLine = true}) {
  final onChange2 = onChanged != null
      ? (T? value) {
          if (value != null) {
            onChanged(value);
          }
        }
      : null;
  return GestureDetector(
    child: Row(
      children: [
        Radio<T>(value: value, groupValue: groupValue, onChanged: onChange2),
        Expanded(
          child: Text(translate(label),
                  overflow: autoNewLine ? null : TextOverflow.ellipsis,
                  style: TextStyle(
                      fontSize: _kContentFontSize,
                      color: disabledTextColor(context, onChange2 != null)))
              .marginOnly(left: 5),
        ),
      ],
    ).marginOnly(left: _kRadioLeftMargin),
    onTap: () => onChange2?.call(value),
  );
}

// ignore: non_constant_identifier_names
Widget _SubButton(String label, Function() onPressed, [bool enabled = true]) {
  return Row(
    children: [
      ElevatedButton(
        onPressed: enabled ? onPressed : null,
        child: Text(
          translate(label),
        ).marginSymmetric(horizontal: 15),
      ),
    ],
  ).marginOnly(left: _kContentHSubMargin);
}

// ignore: non_constant_identifier_names
Widget _SubLabeledWidget(BuildContext context, String label, Widget child,
    {bool enabled = true}) {
  return Row(
    children: [
      Text(
        '${translate(label)}: ',
        style: TextStyle(color: disabledTextColor(context, enabled)),
      ),
      SizedBox(
        width: 10,
      ),
      child,
    ],
  ).marginOnly(left: _kContentHSubMargin);
}

Widget _lock(
  bool locked,
  String label,
  Function() onUnlock,
) {
  return Offstage(
      offstage: !locked,
      child: Row(
        children: [
          Flexible(
            child: SizedBox(
              width: _kCardFixedWidth,
              child: Card(
                child: ElevatedButton(
                  child: SizedBox(
                      height: 25,
                      child: Row(
                          mainAxisAlignment: MainAxisAlignment.center,
                          children: [
                            const Icon(
                              Icons.security_sharp,
                              size: 20,
                            ),
                            Text(translate(label)).marginOnly(left: 5),
                          ]).marginSymmetric(vertical: 2)),
                  onPressed: () async {
                    final unlockPin = bind.mainGetUnlockPin();
                    if (unlockPin.isEmpty || isUnlockPinDisabled()) {
                      bool checked = await callMainCheckSuperUserPermission();
                      if (checked) {
                        onUnlock();
                      }
                    } else {
                      checkUnlockPinDialog(unlockPin, onUnlock);
                    }
                  },
                ).marginSymmetric(horizontal: 2, vertical: 4),
              ).marginOnly(left: _kCardLeftMargin),
            ).marginOnly(top: 10),
          ),
        ],
      ));
}

class _ShortcutsConfigureRow extends StatelessWidget {
  // ignore: unused_element
  const _ShortcutsConfigureRow({Key? key}) : super(key: key);

  @override
  Widget build(BuildContext context) {
    return InkWell(
      onTap: () {
        Navigator.of(context).push(MaterialPageRoute(
          builder: (_) => const DesktopKeyboardShortcutsPage(),
        ));
      },
      child: Row(
        children: [
          Expanded(
            child: Text(translate('Configure shortcuts...')),
          ),
          Icon(Icons.arrow_forward_ios,
                  size: 16, color: disabledTextColor(context, true))
              .marginOnly(right: 4),
        ],
      ).marginOnly(
        left: _kCheckBoxLeftMargin,
        top: 6,
        bottom: 6,
      ),
    );
  }
}

//#endregion

//#region dialogs

//#endregion
