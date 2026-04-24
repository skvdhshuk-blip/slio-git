## ADDED Requirements

### Requirement: History changed files SHALL open a dedicated diff popup
When a user clicks a changed file in the history view, the system SHALL open a dedicated history commit diff popup instead of navigating to the main Changes workspace.

#### Scenario: Click file in commit changed-files panel
- **WHEN** a user selects a commit in the history view and clicks a file in that commit's changed-files list
- **THEN** the system opens a dedicated diff popup for that commit/file pair
- **AND** the active shell section and Git tool-window tab remain on the history view

### Requirement: The popup SHALL preserve history browsing context
The history commit diff popup SHALL not overwrite the user's current history browsing context.

#### Scenario: Close popup returns to the same history context
- **WHEN** a user closes the history commit diff popup
- **THEN** the previously selected commit in the history view remains selected
- **AND** the previously selected file row in the changed-files list remains selected
- **AND** the history view remains visible in the same browsing flow instead of switching to the main Changes workspace

#### Scenario: Open another history file while popup is already available
- **WHEN** a user requests a different changed file from the history view while using the history commit diff flow
- **THEN** the popup content updates to the newly requested commit/file diff
- **AND** the underlying history context remains intact

### Requirement: The popup SHALL reuse the current diff review experience in read-only mode
The history commit diff popup SHALL provide the same read-only review experience as the existing diff surface for commit-file inspection, while excluding workspace mutation actions.

#### Scenario: Popup shows diff review controls
- **WHEN** the history commit diff popup opens
- **THEN** it shows the selected file name/path, diff statistics, diff content, and hunk navigation controls
- **AND** the content is read-only

#### Scenario: Popup changes diff presentation without leaving history
- **WHEN** a user switches diff presentation inside the history commit diff popup
- **THEN** the popup updates to the requested presentation without navigating away from the history view
- **AND** the popup remains scoped to the selected history commit file diff
