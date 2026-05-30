## 1. git-core: read and synthesize merge message

- [x] 1.1 In `src/git-core/src/commit.rs`, add `pub fn prepared_merge_message(repo: &Repository) -> Result<Option<String>, GitError>` that reads `.git/MERGE_MSG`, trims trailing whitespace, returns `Ok(None)` for missing/empty files, handles non-UTF-8 content with `String::from_utf8_lossy`, and maps unexpected I/O errors to `GitError::OperationFailed`.
- [x] 1.2 Add `pub fn synthesize_merge_message(repo: &Repository) -> Result<String, GitError>` that reads `MERGE_HEAD`, resolves each OID to a branch short name when possible (else uses a 7-char short OID), reads the current branch short name, and formats the message per the rules in the spec (single branch, single commit, octopus, and detached-HEAD variants).
- [x] 1.3 Refactor `load_merge_head_parents` (or add an internal helper) to expose the list of merge-head OIDs separately from the `Commit<'_>` lookups, so `synthesize_merge_message` can share the same parsing without duplicating `MERGE_HEAD` reading logic.
- [x] 1.4 Re-export the new functions from `git_core::commit` (they are already accessible via the module path; no changes to `lib.rs` expected unless it becomes cleaner to re-export from the crate root).

## 2. git-core: tests

- [x] 2.1 Add a unit test in `src/git-core/src/commit.rs` that runs `git merge` on a temp repo with a conflict, resolves the conflict, then asserts `prepared_merge_message` returns `Some(msg)` containing `"Merge branch"` and the `"# Conflicts:"` block.
- [x] 2.2 Add a unit test that deletes `.git/MERGE_MSG` after the conflicted merge, then asserts `prepared_merge_message` returns `Ok(None)` and `synthesize_merge_message` returns `"Merge branch '<source>' into <target>"` matching the current branch names.
- [x] 2.3 Add a unit test for the detached-HEAD branch: check out a commit by OID, initiate a merge, and assert `synthesize_merge_message` returns a message without the ` into ...` suffix.
- [x] 2.4 Add a unit test for multi-parent `MERGE_HEAD` (simulate by writing two OIDs into `.git/MERGE_HEAD` after removing `MERGE_MSG`) and assert the output is `Merge branches 'a', 'b' into <target>` (or the short-OID equivalent).

## 3. Commit dialog state

- [x] 3.1 In `src-ui/src/views/commit_dialog.rs`, add `pub default_message: Option<String>` to `CommitDialogState` and initialize it to `None` in `new`, `for_new_commit`, and `for_amend`.
- [x] 3.2 Add `pub fn set_default_message(&mut self, default: String)`: if `self.message` is empty **or** equals `self.default_message.as_deref().unwrap_or("")`, overwrite `self.message` and re-create `self.message_editor` with the new default; always store the new default in `self.default_message`. Do not touch `error` / `success_message`.
- [x] 3.3 Add `pub fn clear_default_message(&mut self)` that resets `self.default_message` to `None` without touching `self.message` (so user-authored text survives).
- [x] 3.4 Add unit-level coverage (inline `#[cfg(test)]` if the module already has tests, otherwise add a focused test module) for the three seeding cases in decision #3: empty editor → seed; editor equals previous default → re-seed with new default; editor edited by user → no change.

## 4. main.rs wiring

- [x] 4.1 In `src-ui/src/main.rs`, introduce a helper `fn sync_merge_commit_message_default(state: &mut AppState, repo: &git_core::Repository)` that: reads `repo.get_state()`; when `Merging`, calls `prepared_merge_message` then `synthesize_merge_message` as a fallback, and forwards the result to `state.commit_dialog.set_default_message`; when not `Merging`, calls `state.commit_dialog.clear_default_message()`.
- [x] 4.2 Call the helper from `open_commit_dialog` after `require_repository`, before navigating to `ShellSection::Changes`.
- [x] 4.3 Call the helper from `refresh_repository_after_action` (or from the common post-action path that already updates `current_repository.state`) so state transitions into/out of `Merging` — including conflict resolution completion and `QuitMergeState` — are picked up without opening the dialog explicitly.
- [x] 4.4 In `submit_commit_dialog`, after `refresh_repository_after_action`, also call the helper so the default is cleared once the merge state transitions back to `Clean`. (Covered by the helper call inside `refresh_repository_after_action`, which `submit_commit_dialog` already invokes.)

## 5. e2e / regression coverage

- [x] 5.1 Extend `e2e/scenarios/test_conflict_resolver.py` (or add a sibling scenario) to: create a conflict, resolve it via the UI, open the commit dialog, and assert the message editor already contains `"Merge branch"` without user input. Commit the merge and assert the next commit dialog opens empty. — **Pivot:** the Python e2e harness is screenshot-only and cannot read the editor text. Instead, added `views::commit_dialog::tests::merge_msg_from_real_conflict_seeds_dialog_and_preserves_user_edits` which spins up a real conflicted-merge repo, asserts `prepared_merge_message` seeds the dialog, and verifies `commit_success` clears the default after the merge commit — a stronger assertion than the screenshot approach would allow.
- [x] 5.2 Add a regression case that edits the prefilled message, triggers a UI refresh (e.g. file change), and asserts the user-authored content is preserved. — Covered by the same integration test (user edits "custom merge note", a second `set_default_message` call simulates a refresh, and the user text survives) plus the `set_default_message_overwrites_previous_default_but_not_user_text` unit test.

## 6. Documentation & release notes

- [x] 6.1 If release notes or a CHANGELOG entry is maintained for the current version, add a bullet describing the prefilled merge message behavior and its fallback. — No CHANGELOG/RELEASE_NOTES file is tracked in-repo; release notes are generated by the GitHub Actions `release.yml` workflow (as documented in `README.md`). No maintained entry to update.
- [x] 6.2 Verify no user-facing i18n string needs a new translation key (only the English fallback template, which per decision #2 stays English); if an on-screen hint is added later, revisit both locales in `src-ui/src/i18n.rs`. — Verified: the implementation adds no on-screen UI text; synthesized fallback is the English string that lands in `git log` (matching Git's own convention). `src-ui/src/i18n.rs` is untouched.
