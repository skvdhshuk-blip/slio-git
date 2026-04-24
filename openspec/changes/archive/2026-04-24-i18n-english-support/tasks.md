## 1. English Locale Instance

- [x] 1.1 Add `pub static EN: I18n` to `src-ui/src/i18n.rs` with all 147 fields translated to English, using IDEA Git wording as reference
- [x] 1.2 Add `pub fn locale(lang: Option<&str>) -> &'static I18n` function that returns `&EN` or `&ZH_CN` based on preference
- [x] 1.3 Verify `EN` compiles and all fields are populated (no empty strings)

## 2. Hardcoded String Audit & Migration

- [ ] 2.1 Add new `I18n` fields for strings currently hardcoded but missing from the struct (feedback messages, dialog titles, status bar text, etc.)
- [ ] 2.2 Replace hardcoded Chinese strings in `src-ui/src/main.rs` with `i18n.*` references
- [ ] 2.3 Replace hardcoded Chinese strings in `src-ui/src/views/*.rs` (main_window, history_view, branch_popup, settings_view, tag_dialog, rebase_editor)
- [x] 2.4 Replace hardcoded Chinese strings in `src-ui/src/widgets/*.rs` (menu, commit_panel, diff_editor, merge_editor) — verified empty via grep 2026-04-24
- [x] 2.5 Replace hardcoded Chinese strings in `src-ui/src/state.rs` (feedback messages, shell titles) — verified empty via grep 2026-04-24
- [x] 2.6 Run CJK grep audit: `grep -rn '[\u4e00-\u9fff]' src-ui/src/views/ src-ui/src/widgets/` and fix remaining user-facing literals

## 3. Locale Detection & Persistence

- [x] 3.1 Add `sys_locale` crate to `src-ui/Cargo.toml` (or use existing transitive dep)
- [x] 3.2 Add `language: Option<String>` field to settings state in `src-ui/src/state.rs`
- [x] 3.3 Implement auto-detection: read system locale on startup, select `EN` or `ZH_CN`
- [x] 3.4 Wire locale selection into `view()` function — replace `&i18n::ZH_CN` with dynamic `i18n::locale(settings.language.as_deref())`
- [x] 3.5 Persist `language` field to settings JSON file on save

## 4. Settings Panel UI

- [x] 4.1 Add language picker to `src-ui/src/views/settings_view.rs` with options: Auto (System) / 中文 / English
- [x] 4.2 Add `Message::ChangeLanguage(Option<String>)` and handler to update state and persist
- [x] 4.3 Show restart-required hint when language is changed

## 5. Verification

- [x] 5.1 Build and run in English mode — verify all major views render correctly (Changes, Log, Branch popup, Settings, context menus)
- [x] 5.2 Build and run in Chinese mode — verify no regressions
- [x] 5.3 Run `cargo test --workspace` — ensure all tests pass
- [x] 5.4 Run e2e test suite with English locale to verify keyboard shortcuts and basic flows still work
