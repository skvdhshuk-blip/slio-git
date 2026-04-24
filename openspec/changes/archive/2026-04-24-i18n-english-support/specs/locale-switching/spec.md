## ADDED Requirements

### Requirement: System locale auto-detection
On startup, the app must detect the system language and select the appropriate locale automatically.

#### Scenario: Chinese system uses Chinese locale
- **WHEN** the system locale starts with "zh" (e.g., zh-CN, zh-TW, zh-Hans)
- **THEN** the app displays in Chinese (`ZH_CN`)

#### Scenario: Non-Chinese system uses English locale
- **WHEN** the system locale is "en-US", "de-DE", "ja-JP", or any non-Chinese locale
- **THEN** the app displays in English (`EN`)

#### Scenario: Locale detection failure falls back to English
- **WHEN** the system locale cannot be determined
- **THEN** the app defaults to English (`EN`)

### Requirement: Manual language override in Settings
Users can manually select their preferred language in the Settings panel.

#### Scenario: Settings panel shows language picker
- **WHEN** the user opens the Settings panel
- **THEN** a language selector is visible with options: "Auto (System)", "中文", "English"

#### Scenario: Selecting a language persists across restarts
- **WHEN** the user selects "English" in Settings and restarts the app
- **THEN** the app launches in English regardless of system locale

#### Scenario: Auto mode re-enables system detection
- **WHEN** the user selects "Auto (System)" in Settings
- **THEN** the app uses system locale detection on next launch

### Requirement: Language preference persistence
The selected language is stored in the app's settings file.

#### Scenario: Settings file stores language
- **WHEN** the user changes language to English
- **THEN** the settings file contains `"language": "en"`

#### Scenario: No language key means auto-detect
- **WHEN** the settings file has no `"language"` key or it is `null`
- **THEN** the app uses system locale auto-detection
