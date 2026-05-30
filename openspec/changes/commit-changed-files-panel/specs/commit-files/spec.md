## ADDED Requirements

### Requirement: Commit changed files data API
git-core SHALL provide a function to retrieve the list of files changed in a specific commit, comparing with its first parent.

#### Scenario: Normal commit with file changes
- **WHEN** `get_commit_changed_files(repo, commit_id)` is called on a commit with 3 changed files
- **THEN** it returns a Vec of 3 `CommitChangedFile` entries, each with path, change status (Added/Modified/Deleted/Renamed), and old/new paths

#### Scenario: Root commit (no parent)
- **WHEN** called on a root commit
- **THEN** all files are reported as Added

#### Scenario: Merge commit
- **WHEN** called on a merge commit
- **THEN** it compares with the first parent (index 0)

### Requirement: Changed files panel in history view
The history view SHALL display a scrollable file list below the commit details when a commit is selected.

#### Scenario: Select commit shows file list
- **WHEN** user selects a commit in the history view
- **THEN** the detail panel shows the changed files with change-type indicators (A/M/D/R) and file count

#### Scenario: Flat and Tree display modes
- **WHEN** user toggles between Flat and Tree mode
- **THEN** the file list switches between flat path list and directory-grouped tree view

#### Scenario: Click file to view diff
- **WHEN** user clicks a file in the changed files list
- **THEN** the app shows the diff for that file in the selected commit
