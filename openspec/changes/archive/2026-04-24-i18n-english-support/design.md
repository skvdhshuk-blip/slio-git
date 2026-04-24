## Context

slio-git has a well-structured `I18n` struct with 147 fields, a `ZH_CN` static instance, and an `i18n` parameter threaded through the view layer. However, many views and widgets still use hardcoded Chinese string literals instead of `i18n.*` references. The app has no locale detection or language preference persistence.

## Goals / Non-Goals

**Goals:**
- Complete English locale (`EN` static) with IDEA-consistent wording
- System locale auto-detection on startup (fall back to English for non-Chinese systems)
- Manual language override in Settings panel, persisted to config
- Audit and replace all hardcoded Chinese strings with `i18n.*` references

**Non-Goals:**
- Full ICU/fluent-based i18n framework (overkill for 2 locales)
- RTL layout support
- Locale-specific date/number formatting (git timestamps are already ISO)
- Additional languages beyond zh-CN and en

## Decisions

### 1. Static locale instances vs runtime string loading

**Decision**: Keep static `&'static I18n` references (`ZH_CN`, `EN`), selected at startup and switchable at runtime.

**Rationale**: The current pattern works, avoids heap allocation, and the struct is `'static`. Two locales don't justify a dynamic loading system. Adding a third locale later just means adding another `static` instance.

**Alternative considered**: `rust-i18n` macro crate — already in `Cargo.toml` but unused. It requires `.yml` files and macro-generated code. Not worth the migration cost for 2 locales when the current struct approach is explicit and type-safe.

### 2. Locale detection strategy

**Decision**: Check `sys_locale::get_locale()` on startup. If it starts with `"zh"`, use `ZH_CN`; otherwise use `EN`. User can override in Settings.

**Rationale**: Simple, deterministic, no platform-specific code. The `sys_locale` crate is tiny (already a transitive dep via iced).

### 3. Hardcoded string audit scope

**Decision**: Fix strings in `main.rs`, `views/*.rs`, `widgets/*.rs`, and `state.rs`. Strings inside `git-core` (error messages, internal logs) stay as-is — they are developer-facing, not user-facing.

**Rationale**: User-facing text must be localized; internal/debug text does not.

### 4. Settings persistence

**Decision**: Add `language: Option<String>` to the existing settings JSON file (`~/.config/slio-git/settings.json` or platform equivalent). Values: `"zh-CN"`, `"en"`, or `null` (auto-detect).

**Rationale**: Reuses existing settings infrastructure. `null` default means auto-detect on fresh installs.

## Risks / Trade-offs

- **Risk**: Some hardcoded strings may be missed during audit → Mitigation: grep for CJK characters (`[\u4e00-\u9fff]`) in `src-ui/src/` after implementation to catch stragglers.
- **Risk**: English text may not fit in UI areas designed for shorter Chinese text → Mitigation: Test at 1024x768 minimum resolution; use ellipsis/truncation where needed (already exists in branch name display).
- **Trade-off**: No hot-reload of locale — user must restart app after changing language. Acceptable for v1; could add runtime switch later by re-creating the view tree.
