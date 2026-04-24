## Why

slio-git currently only supports Chinese (zh-CN) UI text. As an open-source project listed on awesome-iced, English support is essential to reach international users and contributors. The existing `I18n` struct already provides a clean abstraction layer — adding English is a matter of creating a second locale instance and wiring up locale selection.

## What Changes

- Add a complete `EN` (English) locale constant with all 147 fields, using JetBrains IDEA Git tool window wording as reference
- Add locale detection (system language) and manual override via settings
- Persist language preference in app settings
- Wire all hardcoded Chinese strings in views/widgets through the `i18n` parameter instead of inline literals
- Add a language selector to the Settings panel

## Capabilities

### New Capabilities
- `english-locale`: Complete English translation of all I18n fields, following IDEA Git wording conventions
- `locale-switching`: Runtime locale detection (system language), manual override in Settings, and persistence across sessions

### Modified Capabilities

## Impact

- `src-ui/src/i18n.rs` — Add `EN` static instance, add `locale()` detection function
- `src-ui/src/main.rs` — Replace `&i18n::ZH_CN` with dynamic locale selection based on settings
- `src-ui/src/views/*.rs`, `src-ui/src/widgets/*.rs` — Audit and replace any remaining hardcoded Chinese string literals with `i18n.*` references
- `src-ui/src/views/settings_view.rs` — Add language picker (Chinese / English)
- `src-ui/src/state.rs` — Add `language` field to settings state, persist to config file
- No breaking changes; Chinese remains the default for existing users
