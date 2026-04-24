## ADDED Requirements

### Requirement: Complete English locale instance
A static `EN` locale must provide English translations for all 147 fields in the `I18n` struct, using JetBrains IDEA Git tool window wording as the reference standard.

#### Scenario: English locale covers all fields
- **WHEN** the `EN` static is compiled
- **THEN** every field in the `I18n` struct has a non-empty English value

#### Scenario: English wording follows IDEA conventions
- **WHEN** a Git operation label has a direct IDEA equivalent (e.g., "Cherry-Pick", "Revert Commit", "Reset Current Branch to Here...")
- **THEN** the English text matches IDEA's exact wording

#### Scenario: Staging area labels match IDEA
- **WHEN** the staging area is displayed in English
- **THEN** sections are labeled "Staged", "Unstaged", and "Untracked"

### Requirement: No hardcoded Chinese in user-facing UI
All user-visible Chinese string literals in `src-ui/src/` must be replaced with `i18n.*` field references.

#### Scenario: Context menu items use i18n
- **WHEN** a right-click context menu is displayed
- **THEN** all menu item labels come from `i18n.*` fields, not inline string literals

#### Scenario: Feedback messages use i18n
- **WHEN** a feedback banner or toast is shown
- **THEN** the message text is derived from `i18n.*` fields or parameterized with `i18n.*` fragments

#### Scenario: Grep audit passes
- **WHEN** searching `src-ui/src/views/` and `src-ui/src/widgets/` for CJK characters in string literals
- **THEN** no user-facing hardcoded Chinese text remains (internal logs and comments excluded)
