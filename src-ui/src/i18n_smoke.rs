#[cfg(test)]
mod tests {
    use crate::i18n;

    fn has_cjk(s: &str) -> bool {
        s.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
    }

    #[test]
    fn en_locale_has_no_cjk() {
        let en = &i18n::EN;
        let keys: &[(&'static str, &'static str)] = &[
            ("kbd_stage_file", en.kbd_stage_file),
            ("kbd_unstage_file", en.kbd_unstage_file),
            ("kbd_stage_all", en.kbd_stage_all),
            ("kbd_unstage_all", en.kbd_unstage_all),
            ("kbd_refresh", en.kbd_refresh),
            ("kbd_toggle_changes_panel", en.kbd_toggle_changes_panel),
            ("kbd_open_commit_dialog", en.kbd_open_commit_dialog),
            (
                "kbd_toggle_amend_commit_mode",
                en.kbd_toggle_amend_commit_mode,
            ),
            ("kbd_open_push_dialog", en.kbd_open_push_dialog),
            ("kbd_show_file_diff", en.kbd_show_file_diff),
            ("kbd_navigate_prev_file", en.kbd_navigate_prev_file),
            ("kbd_navigate_next_file", en.kbd_navigate_next_file),
            ("kbd_prev_hunk", en.kbd_prev_hunk),
            ("kbd_next_hunk", en.kbd_next_hunk),
            ("kbd_commit", en.kbd_commit),
            ("kbd_stash_save", en.kbd_stash_save),
            ("kbd_stash_pop", en.kbd_stash_pop),
            ("kbd_stash_drop", en.kbd_stash_drop),
            ("kbd_stash_list", en.kbd_stash_list),
            ("kbd_switch_to_log_tab", en.kbd_switch_to_log_tab),
            ("kbd_switch_to_changes_tab", en.kbd_switch_to_changes_tab),
            ("clipboard_unsupported", en.clipboard_unsupported),
            ("clipboard_no_command", en.clipboard_no_command),
            ("lang_change_prompt", en.lang_change_prompt),
            ("lang_change_restart_hint", en.lang_change_restart_hint),
            ("checkout_ref_done_fmt", en.checkout_ref_done_fmt),
            ("checkout_ref_dirty", en.checkout_ref_dirty),
            ("settings_saved", en.settings_saved),
            ("app_name", en.app_name),
        ];
        for (key, value) in keys {
            assert!(!has_cjk(value), "en locale key {} has CJK: {}", key, value);
        }
    }
}
