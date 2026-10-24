/// Keyboard shortcut action IDs - must match
/// src/keyboard/shortcuts.rs::action_id.
const kShortcutActionSendCtrlAltDel = 'send_ctrl_alt_del';
const kShortcutActionToggleFullscreen = 'toggle_fullscreen';
const kShortcutActionSwitchDisplayNext = 'switch_display_next';
const kShortcutActionSwitchDisplayPrev = 'switch_display_prev';
const kShortcutActionSwitchDisplayAll = 'switch_display_all';
const kShortcutActionInsertLock = 'insert_lock';
const kShortcutActionRefresh = 'refresh';
const kShortcutActionSwitchSides = 'switch_sides';
const kShortcutActionCloseTab = 'close_tab';
const kShortcutActionToggleToolbar = 'toggle_toolbar';
const kShortcutActionRestartRemote = 'restart_remote';
const kShortcutActionSwitchTabNext = 'switch_tab_next';
const kShortcutActionSwitchTabPrev = 'switch_tab_prev';

/// Actions that move keyboard focus to another session or a dialog. They run when the
/// key that fired them is released, not when it is pressed, so the matcher
/// that consumed the press also sees its repeats and its release. On Linux a
/// legacy-mode session matches in Dart while a map-mode session matches in
/// Rust, and neither knows the other's fired keys. Mirrors `runs_on_release`
/// in `src/keyboard/shortcuts.rs`; both are checked against
/// `flutter/test/fixtures/key_up_shortcut_actions.json`.
const kShortcutActionsRunOnKeyUp = {
  kShortcutActionCloseTab,
  kShortcutActionSwitchTabNext,
  kShortcutActionSwitchTabPrev,
  kShortcutActionRestartRemote,
  kShortcutActionSwitchSides,
};
const kShortcutActionToggleMute = 'toggle_mute';
const kShortcutActionPinToolbar = 'pin_toolbar';
const kShortcutActionViewModeOriginal = 'view_mode_original';
const kShortcutActionViewModeAdaptive = 'view_mode_adaptive';
const kShortcutActionToggleDisableClipboard = 'toggle_disable_clipboard';
const kShortcutActionToggleEnableFileCopyPaste = 'toggle_enable_file_copy_paste';
const kShortcutActionViewModeCustom = 'view_mode_custom';
const kShortcutActionImageQualityBest = 'image_quality_best';
const kShortcutActionImageQualityBalanced = 'image_quality_balanced';
const kShortcutActionImageQualityLow = 'image_quality_low';

const kShortcutLocalConfigKey = 'keyboard-shortcuts';
const kShortcutEventName = 'shortcut_triggered';

/// Canonical default keyboard-shortcut bindings, mirroring Rust's
/// `default_bindings()` in `src/keyboard/shortcuts.rs`. Used by:
///   * the configuration page when seeding defaults on first enable, after
///     [filterDefaultBindingsForPlatform] has trimmed platform-specific
///     entries.
///
/// Parity with Rust is unit-tested on both sides against
/// `flutter/test/fixtures/default_keyboard_shortcuts.json` — see the
/// `kDefaultShortcutBindings matches fixture` test in
/// `flutter/test/keyboard_shortcuts_test.dart` and
/// `default_bindings_match_fixture_json` in `src/keyboard/shortcuts.rs`.
/// Any change here MUST also update the fixture and the Rust source, or CI
/// will fail in the side that drifted.
final List<Map<String, Object>> kDefaultShortcutBindings = [
  for (final entry in <List<Object>>[
    [kShortcutActionSendCtrlAltDel,         'delete'],
    [kShortcutActionToggleFullscreen,       'enter'],
    [kShortcutActionSwitchDisplayNext,      'arrow_right'],
    [kShortcutActionSwitchDisplayPrev,      'arrow_left'],
    // The tested iPad/Bluetooth keyboard setup did not deliver the complete
    // Cmd+Option+Shift+S chord, also in Safari. Other physical keyboards are
    // unverified; Mute can be rebound to a chord that reaches the app.
    [kShortcutActionToggleMute,             's'],
  ])
    {
      'action': entry[0],
      'mods': const ['primary', 'alt', 'shift'],
      'key': entry[1],
    },
];
