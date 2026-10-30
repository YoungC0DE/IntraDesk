// flutter/lib/common/widgets/keyboard_shortcuts/page_body.dart
//
// Body widget for the Keyboard Shortcuts configuration page, rendered by
// `desktop/pages/desktop_keyboard_shortcuts_page.dart` inside its Scaffold +
// AppBar shell.
//
// The body owns:
//   * a grouped list of actions, each with its current binding plus
//     edit / clear icons;
//   * the JSON read/write helpers under [kShortcutLocalConfigKey] in the
//     canonical {enabled, bindings:[{action,mods,key}]} shape;
//   * the recording-dialog round-trip and conflict-replace bookkeeping;
//   * "Reset to defaults" (called from the AppBar, which calls
//     [KeyboardShortcutsPageBodyState.resetToDefaultsWithConfirm]).
//
// The enable/pass-through toggles live in the General settings tab, the only
// entry point to this page.

import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';

import '../../../common.dart';
import '../../../consts.dart';
import '../../../models/platform_model.dart';
import '../../../models/shortcut_model.dart';
import 'display.dart';
import 'recording_dialog.dart';
import 'shortcut_actions.dart';
import 'shortcut_utils.dart';

/// The body widget. Render this inside a Scaffold.
class KeyboardShortcutsPageBody extends StatefulWidget {
  const KeyboardShortcutsPageBody({Key? key}) : super(key: key);

  @override
  State<KeyboardShortcutsPageBody> createState() =>
      KeyboardShortcutsPageBodyState();
}

/// Public state so platform shells can call [resetToDefaultsWithConfirm] from
/// their AppBar action.
class KeyboardShortcutsPageBodyState extends State<KeyboardShortcutsPageBody> {
  // ----- Persistence helpers -----

  Map<String, dynamic> _readJson() {
    final raw = bind.mainGetLocalOption(key: kShortcutLocalConfigKey);
    if (raw.isEmpty) return {'enabled': false, 'bindings': <dynamic>[]};
    try {
      final parsed = jsonDecode(raw) as Map<String, dynamic>;
      parsed['bindings'] ??= <dynamic>[];
      parsed['enabled'] ??= false;
      return parsed;
    } catch (_) {
      return {'enabled': false, 'bindings': <dynamic>[]};
    }
  }

  Future<void> _writeJson(Map<String, dynamic> json) async {
    await bind.mainSetLocalOption(
        key: kShortcutLocalConfigKey, value: jsonEncode(json));
    // Refresh the Rust matcher cache so writes take effect immediately.
    bind.mainReloadKeyboardShortcuts();
    if (mounted) setState(() {});
  }

  /// Replace the bindings entry for [actionId] with [binding]. If [binding]
  /// is null, removes the existing entry. If the user is replacing a
  /// conflicting binding, [clearActionId] points at the action whose
  /// (now-stale) binding should be removed in the same write.
  Future<void> _setBinding(
    String actionId, {
    Map<String, dynamic>? binding,
    String? clearActionId,
  }) async {
    final json = _readJson();
    final list = shortcutBindingMapsFrom(json['bindings']);
    list.removeWhere((b) {
      final a = b['action'];
      return a == actionId || (clearActionId != null && a == clearActionId);
    });
    if (binding != null) {
      list.add(binding);
    }
    json['bindings'] = list;
    await _writeJson(json);
  }

  Future<void> _resetToDefaults() async {
    final json = _readJson();
    // Single source of truth lives in `ShortcutModel.currentPlatformCapabilities`
    // — the same helper feeds the first-enable seed pass, this Reset action,
    // and the action-list filter below, so the three can never disagree on
    // which actions belong on this platform.
    json['bindings'] = filterDefaultBindingsForPlatform(
      jsonDecode(bind.mainGetDefaultKeyboardShortcuts()) as List,
      ShortcutModel.currentPlatformCapabilities(),
    );
    await _writeJson(json);
  }

  String _labelFor(String actionId) {
    // Intentionally walks the unfiltered list (via the recursive helper, so
    // both direct entries and subgroup entries are covered) — a stale
    // cross-platform binding (e.g. Toggle Toolbar carried over from
    // desktop) should still resolve to its human-readable label in conflict
    // warnings.
    for (final entry in allActionEntries(kKeyboardShortcutActionGroups)) {
      if (entry.id == actionId) return translate(entry.labelKey);
    }
    return actionId;
  }

  /// Action groups visible on the current platform. Reads the same
  /// capability set as the seed-defaults / reset-to-defaults paths from
  /// `ShortcutModel.currentPlatformCapabilities`, so the UI lists exactly
  /// the actions whose handlers the matcher can dispatch here.
  List<KeyboardShortcutActionGroup> _groupsForCurrentPlatform() {
    return filterKeyboardShortcutActionGroupsForPlatform(
      ShortcutModel.currentPlatformCapabilities(),
    );
  }

  // ----- UI handlers -----

  Future<void> _onEdit(KeyboardShortcutActionEntry entry) async {
    final json = _readJson();
    final bindings = shortcutBindingMapsFrom(json['bindings']);
    final result = await showRecordingDialog(
      context: context,
      actionId: entry.id,
      actionLabel: translate(entry.labelKey),
      existingBindings: bindings,
      actionLabelLookup: _labelFor,
    );
    if (result == null) return;
    await _setBinding(
      entry.id,
      binding: result.binding,
      clearActionId: result.clearActionId,
    );
  }

  Future<void> _onClear(KeyboardShortcutActionEntry entry) async {
    await _setBinding(entry.id, binding: null);
  }

  /// Public — invoked from the platform AppBar action.
  Future<void> resetToDefaultsWithConfirm() async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: Text(translate('Reset to defaults')),
        content: Text(translate('shortcut-reset-confirm-tip')),
        actions: [
          dialogButton('Cancel',
              onPressed: () => Navigator.of(ctx).pop(false), isOutline: true),
          dialogButton('OK', onPressed: () => Navigator.of(ctx).pop(true)),
        ],
      ),
    );
    if (confirmed == true) {
      await _resetToDefaults();
    }
  }

  // ----- Build -----

  @override
  Widget build(BuildContext context) {
    final enabled = ShortcutModel.isEnabled();
    final theme = Theme.of(context);

    return ListView(
      padding: const EdgeInsets.all(16),
      children: [
        const SizedBox(height: 8),
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: 8),
          child: Text(
            translate('shortcut-page-description'),
            style: TextStyle(color: theme.hintColor),
          ),
        ),
        const SizedBox(height: 16),
        // Bindings list and configuration entry only show when shortcuts are
        // enabled — there is nothing to configure while the matcher is off.
        if (enabled)
          Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              for (final group in _groupsForCurrentPlatform())
                _buildGroup(context, group),
            ],
          ),
      ],
    );
  }

  // One indent unit per nesting level. Both "top item under top heading"
  // and "subgroup heading under top group" are *one* level deeper than the
  // top heading, so they share this indent — meaning a top-level direct
  // item and a sibling subgroup heading line up at exactly the same x.
  // Subgroup items are *two* levels deeper.
  static const double _kIndentStep = 16.0;

  /// Top-level group: heading at zero indent, then walk `children` in
  /// declaration order. Direct entries get [_kIndentStep] of indent so
  /// they read as "items under this heading"; subgroup headings sit at
  /// the same indent (a subgroup is a sibling of the direct items, just
  /// with its own nested entries below).
  Widget _buildGroup(BuildContext context, KeyboardShortcutActionGroup group) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        const SizedBox(height: 12),
        _buildHeading(context, group.titleKey, isSub: false),
        const SizedBox(height: 4),
        for (final child in group.children)
          switch (child) {
            KeyboardShortcutActionEntry() => Padding(
                padding: const EdgeInsets.only(left: _kIndentStep),
                child: _buildEntryRow(context, child),
              ),
            KeyboardShortcutActionSubgroup() =>
              _buildSubgroup(context, child),
          },
      ],
    );
  }

  Widget _buildSubgroup(
      BuildContext context, KeyboardShortcutActionSubgroup subgroup) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        const SizedBox(height: 8),
        _buildHeading(context, subgroup.titleKey, isSub: true),
        const SizedBox(height: 4),
        for (final entry in subgroup.entries)
          Padding(
            // Two indent steps: one for "subgroup heading is nested under
            // top heading" (matches the heading's own indent) and one for
            // "this entry is under the subgroup heading".
            padding: const EdgeInsets.only(left: _kIndentStep * 2),
            child: _buildEntryRow(context, entry),
          ),
      ],
    );
  }

  Widget _buildHeading(BuildContext context, String titleKey,
      {required bool isSub}) {
    // Subgroup heading nests one step under the top heading — same indent
    // as a top-level direct item, so the two line up at the same x.
    final indent = isSub ? _kIndentStep : 0.0;
    return Padding(
      padding: EdgeInsets.only(left: 8 + indent, right: 8),
      child: Row(
        children: [
          Text(
            translate(titleKey),
            style: TextStyle(
              fontWeight: isSub ? FontWeight.w500 : FontWeight.w600,
              color: isSub
                  ? Theme.of(context).hintColor
                  : Theme.of(context).colorScheme.primary,
            ),
          ),
          const SizedBox(width: 8),
          Expanded(child: Divider(thickness: isSub ? 0.5 : 1)),
        ],
      ),
    );
  }

  /// Dense row: label | shortcut | edit | clear, all in one Row.
  Widget _buildEntryRow(
      BuildContext context, KeyboardShortcutActionEntry entry) {
    final shortcut = ShortcutDisplay.formatFor(entry.id, requireEnabled: false);
    final hasBinding = shortcut != null;
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 6),
      child: Row(
        children: [
          Expanded(
            flex: 5,
            child: Text(translate(entry.labelKey)),
          ),
          Expanded(
            flex: 4,
            child: Text(
              shortcut ?? '—',
              style: TextStyle(
                fontFamily: defaultTargetPlatform == TargetPlatform.windows
                    ? 'Consolas'
                    : 'monospace',
                color: hasBinding ? null : Theme.of(context).hintColor,
              ),
            ),
          ),
          IconButton(
            tooltip: translate('Edit'),
            onPressed: () => _onEdit(entry),
            icon: const Icon(Icons.edit_outlined, size: 18),
          ),
          SizedBox(
            width: 40,
            child: hasBinding
                ? IconButton(
                    tooltip: translate('Clear'),
                    onPressed: () => _onClear(entry),
                    icon: const Icon(Icons.close, size: 18),
                  )
                : const SizedBox.shrink(),
          ),
        ],
      ),
    );
  }
}

/// Small help-icon tooltip used for inline explanations next to a checkbox /
/// row. Triggers on hover and tap. Public so the desktop
/// General settings tab can reuse it.
class InfoTooltipIcon extends StatelessWidget {
  final String tipKey;
  const InfoTooltipIcon({Key? key, required this.tipKey}) : super(key: key);

  @override
  Widget build(BuildContext context) {
    return Tooltip(
      message: translate(tipKey),
      triggerMode: TooltipTriggerMode.tap,
      preferBelow: false,
      waitDuration: const Duration(milliseconds: 250),
      showDuration: const Duration(seconds: 6),
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 6),
        child: Icon(
          Icons.help_outline,
          size: 16,
          color: Theme.of(context).hintColor,
        ),
      ),
    );
  }
}
