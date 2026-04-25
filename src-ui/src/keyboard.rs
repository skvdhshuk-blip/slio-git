//! Keyboard shortcuts handling
//!
//! Provides keyboard shortcut support for staging/unstaging operations

#![allow(dead_code)]

use iced::Event;
use iced::keyboard;

/// Keyboard shortcut actions
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShortcutAction {
    StageFile,
    UnstageFile,
    StageAll,
    UnstageAll,
    Refresh,
    ToggleChangesPanel,
    // Commit operations (IDEA: Ctrl+K)
    OpenCommitDialog,
    ToggleAmendCommitMode,
    // Push operations (IDEA: Ctrl+Shift+K)
    OpenPushDialog,
    // Diff operations (IDEA: Ctrl+D)
    ShowFileDiff,
    // Navigation
    NavigatePrevFile,
    NavigateNextFile,
    PrevHunk,
    NextHunk,
    // Commit (IDEA: Ctrl+Enter inside commit dialog)
    Commit,
    // Stash operations
    StashSave,
    StashPop,
    StashDrop,
    StashList,
    // Tab switching
    SwitchToLogTab,
    SwitchToChangesTab,
    // Open folder (Welcome + global, N9)
    OpenFolder,
    // Toggle console output panel (IDEA: Cmd+F12 / Ctrl+F12)
    ToggleConsole,
}

/// Keyboard shortcut definition
#[derive(Debug, Clone)]
pub struct KeyboardShortcut {
    pub modifiers: keyboard::Modifiers,
    pub key: keyboard::Key,
    pub action: ShortcutAction,
}

impl KeyboardShortcut {
    /// Check if this shortcut matches the given event
    pub fn matches(&self, event: &Event) -> bool {
        if let Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) = event {
            return *key == self.key && modifiers.contains(self.modifiers);
        }
        false
    }
}

/// Get all registered keyboard shortcuts
pub fn get_shortcuts() -> Vec<KeyboardShortcut> {
    use keyboard::key::Named;
    use keyboard::{Key, Modifiers};

    vec![
        // Ctrl+S: Stage selected file
        KeyboardShortcut {
            modifiers: Modifiers::CTRL,
            key: Key::Character("s".into()),
            action: ShortcutAction::StageFile,
        },
        // Ctrl+U: Unstage selected file
        KeyboardShortcut {
            modifiers: Modifiers::CTRL,
            key: Key::Character("u".into()),
            action: ShortcutAction::UnstageFile,
        },
        // Ctrl+Shift+S: Stage all
        KeyboardShortcut {
            modifiers: Modifiers::CTRL | Modifiers::SHIFT,
            key: Key::Character("s".into()),
            action: ShortcutAction::StageAll,
        },
        // Ctrl+Shift+U: Unstage all
        KeyboardShortcut {
            modifiers: Modifiers::CTRL | Modifiers::SHIFT,
            key: Key::Character("u".into()),
            action: ShortcutAction::UnstageAll,
        },
        // Ctrl+R: Refresh
        KeyboardShortcut {
            modifiers: Modifiers::CTRL,
            key: Key::Character("r".into()),
            action: ShortcutAction::Refresh,
        },
        // Ctrl+K: Open commit dialog (IDEA style)
        KeyboardShortcut {
            modifiers: Modifiers::CTRL,
            key: Key::Character("k".into()),
            action: ShortcutAction::OpenCommitDialog,
        },
        // IDEA: Alt+M / macOS Ctrl+Alt+M toggle amend commit mode
        KeyboardShortcut {
            modifiers: Modifiers::ALT,
            key: Key::Character("m".into()),
            action: ShortcutAction::ToggleAmendCommitMode,
        },
        KeyboardShortcut {
            modifiers: Modifiers::CTRL | Modifiers::ALT,
            key: Key::Character("m".into()),
            action: ShortcutAction::ToggleAmendCommitMode,
        },
        // Ctrl+Shift+K: Open push dialog (IDEA style)
        KeyboardShortcut {
            modifiers: Modifiers::CTRL | Modifiers::SHIFT,
            key: Key::Character("k".into()),
            action: ShortcutAction::OpenPushDialog,
        },
        // Ctrl+D: Show diff for file (IDEA style)
        KeyboardShortcut {
            modifiers: Modifiers::CTRL,
            key: Key::Character("d".into()),
            action: ShortcutAction::ShowFileDiff,
        },
        // Ctrl+Alt+Left: Previous file
        KeyboardShortcut {
            modifiers: Modifiers::CTRL | Modifiers::ALT,
            key: Key::Named(Named::ArrowLeft),
            action: ShortcutAction::NavigatePrevFile,
        },
        // Ctrl+Alt+Right: Next file
        KeyboardShortcut {
            modifiers: Modifiers::CTRL | Modifiers::ALT,
            key: Key::Named(Named::ArrowRight),
            action: ShortcutAction::NavigateNextFile,
        },
        // Ctrl+Enter: Commit (IDEA style)
        KeyboardShortcut {
            modifiers: Modifiers::CTRL,
            key: Key::Named(Named::Enter),
            action: ShortcutAction::Commit,
        },
        // Ctrl+Shift+Z: Save stash
        KeyboardShortcut {
            modifiers: Modifiers::CTRL | Modifiers::SHIFT,
            key: Key::Character("z".into()),
            action: ShortcutAction::StashSave,
        },
        // Ctrl+Z: Pop stash
        KeyboardShortcut {
            modifiers: Modifiers::CTRL,
            key: Key::Character("z".into()),
            action: ShortcutAction::StashPop,
        },
        // Ctrl+Alt+Z: Drop stash
        KeyboardShortcut {
            modifiers: Modifiers::CTRL | Modifiers::ALT,
            key: Key::Character("z".into()),
            action: ShortcutAction::StashDrop,
        },
        // Ctrl+O: Open Folder (global; ref: FlatWelcomeFrame.kt:111)
        // Mac Cmd mapping deferred to N9-followup (primary_modifier() refactor)
        KeyboardShortcut {
            modifiers: Modifiers::CTRL,
            key: Key::Character("o".into()),
            action: ShortcutAction::OpenFolder,
        },
        // Ctrl+L: Switch to Log tab
        KeyboardShortcut {
            modifiers: Modifiers::CTRL,
            key: Key::Character("l".into()),
            action: ShortcutAction::SwitchToLogTab,
        },
        // Ctrl+Shift+L: Switch to Changes tab
        KeyboardShortcut {
            modifiers: Modifiers::CTRL | Modifiers::SHIFT,
            key: Key::Character("l".into()),
            action: ShortcutAction::SwitchToChangesTab,
        },
        // F7: Next hunk (IDEA style)
        KeyboardShortcut {
            modifiers: Modifiers::empty(),
            key: Key::Named(Named::F7),
            action: ShortcutAction::NextHunk,
        },
        // Shift+F7: Previous hunk (IDEA style)
        KeyboardShortcut {
            modifiers: Modifiers::SHIFT,
            key: Key::Named(Named::F7),
            action: ShortcutAction::PrevHunk,
        },
        // Cmd+F12 / Ctrl+F12: Toggle console (IDEA style)
        KeyboardShortcut {
            modifiers: Modifiers::CTRL,
            key: Key::Named(Named::F12),
            action: ShortcutAction::ToggleConsole,
        },
    ]
}

/// Find the action for a keyboard event
pub fn find_action(event: &Event) -> Option<ShortcutAction> {
    for shortcut in get_shortcuts() {
        if shortcut.matches(event) {
            return Some(shortcut.action);
        }
    }
    None
}

/// Format a keyboard shortcut for display
pub fn format_shortcut(shortcut: &KeyboardShortcut) -> String {
    let mut parts = Vec::new();

    if shortcut.modifiers.contains(keyboard::Modifiers::CTRL) {
        parts.push("Ctrl".to_string());
    }
    if shortcut.modifiers.contains(keyboard::Modifiers::SHIFT) {
        parts.push("Shift".to_string());
    }
    if shortcut.modifiers.contains(keyboard::Modifiers::ALT) {
        parts.push("Alt".to_string());
    }

    if let keyboard::Key::Character(c) = &shortcut.key {
        parts.push(c.to_uppercase());
    } else {
        parts.push(format!("{:?}", shortcut.key));
    }

    parts.join("+")
}

/// Get the description for a shortcut action
pub fn action_description(action: ShortcutAction, i18n: &crate::i18n::I18n) -> &'static str {
    match action {
        ShortcutAction::StageFile => i18n.kbd_stage_file,
        ShortcutAction::UnstageFile => i18n.kbd_unstage_file,
        ShortcutAction::StageAll => i18n.kbd_stage_all,
        ShortcutAction::UnstageAll => i18n.kbd_unstage_all,
        ShortcutAction::Refresh => i18n.kbd_refresh,
        ShortcutAction::ToggleChangesPanel => i18n.kbd_toggle_changes_panel,
        ShortcutAction::OpenCommitDialog => i18n.kbd_open_commit_dialog,
        ShortcutAction::ToggleAmendCommitMode => i18n.kbd_toggle_amend_commit_mode,
        ShortcutAction::OpenPushDialog => i18n.kbd_open_push_dialog,
        ShortcutAction::ShowFileDiff => i18n.kbd_show_file_diff,
        ShortcutAction::NavigatePrevFile => i18n.kbd_navigate_prev_file,
        ShortcutAction::NavigateNextFile => i18n.kbd_navigate_next_file,
        ShortcutAction::PrevHunk => i18n.kbd_prev_hunk,
        ShortcutAction::NextHunk => i18n.kbd_next_hunk,
        ShortcutAction::Commit => i18n.kbd_commit,
        ShortcutAction::StashSave => i18n.kbd_stash_save,
        ShortcutAction::StashPop => i18n.kbd_stash_pop,
        ShortcutAction::StashDrop => i18n.kbd_stash_drop,
        ShortcutAction::StashList => i18n.kbd_stash_list,
        ShortcutAction::SwitchToLogTab => i18n.kbd_switch_to_log_tab,
        ShortcutAction::SwitchToChangesTab => i18n.kbd_switch_to_changes_tab,
        ShortcutAction::OpenFolder => i18n.welcome_open_folder_btn,
        ShortcutAction::ToggleConsole => i18n.kbd_toggle_console,
    }
}

#[cfg(test)]
mod tests {
    use super::{KeyboardShortcut, ShortcutAction, action_description, get_shortcuts};
    use iced::keyboard::{Key, Modifiers};

    fn has_shortcut(shortcut: &KeyboardShortcut, modifiers: Modifiers, key: Key) -> bool {
        shortcut.modifiers == modifiers && shortcut.key == key
    }

    #[test]
    fn idea_style_toggle_amend_shortcuts_are_registered() {
        let shortcuts = get_shortcuts();

        assert!(shortcuts.iter().any(|shortcut| {
            shortcut.action == ShortcutAction::ToggleAmendCommitMode
                && has_shortcut(shortcut, Modifiers::ALT, Key::Character("m".into()))
        }));

        assert!(shortcuts.iter().any(|shortcut| {
            shortcut.action == ShortcutAction::ToggleAmendCommitMode
                && has_shortcut(
                    shortcut,
                    Modifiers::CTRL | Modifiers::ALT,
                    Key::Character("m".into()),
                )
        }));
    }

    #[test]
    fn toggle_amend_shortcut_has_user_facing_description() {
        let i18n = crate::i18n::I18n::chinese();
        assert_eq!(
            action_description(ShortcutAction::ToggleAmendCommitMode, &i18n),
            i18n.kbd_toggle_amend_commit_mode
        );
    }
}
