import 'package:debounce_throttle/debounce_throttle.dart';
import 'package:flutter/material.dart';
import 'package:flutter_hbb/common.dart';
import 'package:flutter_hbb/consts.dart';
import 'package:flutter_hbb/models/platform_model.dart';
import 'package:get/get.dart';

customImageQualityWidget(
    {required double initQuality,
    required double initFps,
    required Function(double)? setQuality,
    required Function(double)? setFps,
    required bool showFps,
    required bool showMoreQuality}) {
  if (initQuality < kMinQuality ||
      initQuality > (showMoreQuality ? kMaxMoreQuality : kMaxQuality)) {
    initQuality = kDefaultQuality;
  }
  if (initFps < kMinFps || initFps > kMaxFps) {
    initFps = kDefaultFps;
  }
  final qualityValue = initQuality.obs;
  final fpsValue = initFps.obs;

  final RxBool moreQualityChecked = RxBool(qualityValue.value > kMaxQuality);
  final debouncerQuality = Debouncer<double>(
    Duration(milliseconds: 1000),
    onChanged: setQuality,
    initialValue: qualityValue.value,
  );
  final debouncerFps = Debouncer<double>(
    Duration(milliseconds: 1000),
    onChanged: setFps,
    initialValue: fpsValue.value,
  );

  onMoreChanged(bool? value) {
    if (value == null) return;
    moreQualityChecked.value = value;
    if (!value && qualityValue.value > 100) {
      qualityValue.value = 100;
    }
    debouncerQuality.value = qualityValue.value;
  }

  return Column(
    children: [
      Obx(() => Row(
            children: [
              Expanded(
                flex: 3,
                child: Slider(
                  value: qualityValue.value,
                  min: kMinQuality,
                  max: moreQualityChecked.value ? kMaxMoreQuality : kMaxQuality,
                  divisions: moreQualityChecked.value
                      ? ((kMaxMoreQuality - kMinQuality) / 10).round()
                      : ((kMaxQuality - kMinQuality) / 5).round(),
                  onChanged: setQuality == null
                      ? null
                      : (double value) async {
                          qualityValue.value = value;
                          debouncerQuality.value = value;
                        },
                ),
              ),
              Expanded(
                  flex: 1,
                  child: Text(
                    '${qualityValue.value.round()}%',
                    style: const TextStyle(fontSize: 15),
                  )),
              Expanded(
                  flex: 1,
                  child: Text(
                    translate('Bitrate'),
                    style: const TextStyle(fontSize: 15),
                  )),
              // mobile doesn't have enough space
              if (showMoreQuality)
                Expanded(
                    flex: 1,
                    child: Row(
                      children: [
                        Checkbox(
                          value: moreQualityChecked.value,
                          onChanged: onMoreChanged,
                        ),
                        Expanded(
                          child: Text(translate('More')),
                        )
                      ],
                    ))
            ],
          )),
      if (showFps)
        Obx(() => Row(
              children: [
                Expanded(
                  flex: 3,
                  child: Slider(
                    value: fpsValue.value,
                    min: kMinFps,
                    max: kMaxFps,
                    divisions: ((kMaxFps - kMinFps) / 5).round(),
                    onChanged: setFps == null
                        ? null
                        : (double value) async {
                            fpsValue.value = value;
                            debouncerFps.value = value;
                          },
                  ),
                ),
                Expanded(
                    flex: 1,
                    child: Text(
                      '${fpsValue.value.round()}',
                      style: const TextStyle(fontSize: 15),
                    )),
                Expanded(
                    flex: 2,
                    child: Text(
                      translate('FPS'),
                      style: const TextStyle(fontSize: 15),
                    ))
              ],
            )),
    ],
  );
}

customImageQualitySetting() {
  final qualityKey = 'custom_image_quality';
  final fpsKey = 'custom-fps';

  final initQuality =
      (double.tryParse(bind.mainGetUserDefaultOption(key: qualityKey)) ??
          kDefaultQuality);
  final isQuanlityFixed = isOptionFixed(qualityKey);
  final initFps =
      (double.tryParse(bind.mainGetUserDefaultOption(key: fpsKey)) ??
          kDefaultFps);
  final isFpsFixed = isOptionFixed(fpsKey);

  return customImageQualityWidget(
      initQuality: initQuality,
      initFps: initFps,
      setQuality: isQuanlityFixed
          ? null
          : (v) {
              bind.mainSetUserDefaultOption(
                  key: qualityKey, value: v.toString());
            },
      setFps: isFpsFixed
          ? null
          : (v) {
              bind.mainSetUserDefaultOption(key: fpsKey, value: v.toString());
            },
      showFps: true,
      showMoreQuality: true);
}

List<(String, String)> otherDefaultSettings() {
  List<(String, String)> v = [
    ('Mute', kOptionDisableAudio),
    ('Enable file copy and paste', kOptionEnableFileCopyPaste),
    ('Disable clipboard', kOptionDisableClipboard),
  ];

  return v;
}

String getOtherDefaultSettingOption(String key) {
  return bind.mainGetUserDefaultOption(key: key);
}

Future<void> setOtherDefaultSettingOption(String key, String value) {
  return bind.mainSetUserDefaultOption(key: key, value: value);
}

bool isOtherDefaultSettingReadOnly(String key) => isOptionFixed(key);
