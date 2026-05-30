## Why

When a `git merge` stops on conflicts, Git itself prepares a default commit message (in `.git/MERGE_MSG`, e.g. `Merge branch 'feature' into main` plus a conflicts block) that every major client — terminal git, GitHub Desktop, IntelliJ — reuses automatically once the user resolves the conflicts. In slio-git the commit dialog currently opens with an **empty** message after conflict resolution, forcing the user to manually type `Merge branch 'X' into Y` for every merge. This is friction the tool already has all the information to remove.

## What Changes

- When the repository state transitions to `Merging` (or the commit dialog opens while state is `Merging`), slio-git MUST populate the commit dialog message with the merge message Git staged on disk (`.git/MERGE_MSG`), including the standard conflicts summary block.
- If `.git/MERGE_MSG` is missing or unreadable, fall back to a synthesized message of the form `Merge branch '<source>' into <target>`, derived from `MERGE_HEAD` plus the current branch name. If the source cannot be resolved (detached `MERGE_HEAD` entry, anonymous ref), fall back to `Merge commit '<short-oid>' into <target>`.
- The auto-populated message is a **default**: it only replaces an empty editor, never overwrites text the user has already typed or edited in the commit dialog for this merge session.
- Clear the auto-populated state after the merge commit succeeds or the merge is aborted (`quit_merge`), so the next (non-merge) commit dialog starts empty as it does today.
- No change to how the commit is actually written — `create_commit` already handles the `Merge` repository state and multi-parent commit.

## Capabilities

### New Capabilities
- `merge-commit-message`: Auto-populates the commit dialog message with Git's prepared merge message (or a synthesized fallback) whenever the repository is in the `Merging` state, without overwriting user edits.

### Modified Capabilities
<!-- None — this is additive behavior scoped to the merge flow and does not change the requirements of any existing archived capability. -->

## Impact

- **Affected code**:
  - `src/git-core/src/commit.rs` (or a sibling module): add a function to read `.git/MERGE_MSG` and synthesize a fallback from `MERGE_HEAD` + current branch.
  - `src-ui/src/views/commit_dialog.rs`: expose a way to seed `message`/`message_editor` with a merge default while tracking whether the content is still the untouched default.
  - `src-ui/src/main.rs`: on repository refresh and when opening the commit dialog, when state is `Merging` and the dialog message is empty/untouched, call the new helper to seed the editor.
- **APIs**: new public helper in `git-core` (e.g. `git_core::commit::prepared_merge_message(&Repository) -> Option<String>`), consumed only by the UI layer.
- **Dependencies**: none new — uses existing `git2` `Repository`, `MERGE_HEAD` parsing already present in `commit.rs`, and the filesystem read pattern used there.
- **i18n**: one new string for the fallback template (`merge_commit_message_fallback_fmt`) in `src-ui/src/i18n.rs` for both locales; no change to existing strings.
- **Tests**: unit test in `git-core` for reading `MERGE_MSG` and for the synthesized fallback; integration coverage extending the existing conflict-resolver e2e scenario to assert the prepopulated message.
