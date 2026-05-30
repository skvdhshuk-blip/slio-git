## Context

`git merge` writes two files into `.git/` when it runs:
- `MERGE_HEAD` — one or more OIDs of the commits being merged in (already consumed by `src/git-core/src/commit.rs:119 load_merge_head_parents`).
- `MERGE_MSG` — a ready-to-use commit message, e.g.

  ```
  Merge branch 'feature' into main

  # Conflicts:
  #   src/foo.rs
  ```

Today slio-git ignores `MERGE_MSG`. After the user resolves conflicts via the conflict resolver (`src-ui/src/widgets/conflict_resolver.rs`) and opens the commit dialog (`src-ui/src/views/commit_dialog.rs`), `CommitDialogState::for_new_commit` always initializes `message` to `String::new()` (`src-ui/src/views/commit_dialog.rs:77`). The user has to type the merge message by hand every time — which stands out because `create_commit` in `src/git-core/src/commit.rs:40` already does the right thing for merge commits once a message is provided.

The commit dialog is opened via `open_commit_dialog` in `src-ui/src/main.rs:4412`, which today only navigates to the `Changes` section and sets a banner; it does not re-create or re-seed `state.commit_dialog`. The dialog state is kept across UI refreshes, so a one-shot “seed on transition into Merging” approach must coexist with the existing long-lived dialog state.

## Goals / Non-Goals

**Goals:**
- When the repo enters `RepositoryState::Merging`, make the commit dialog's message editor default to the Git-prepared merge message (from `.git/MERGE_MSG`), matching the behavior of the terminal `git commit` that pops up an editor prefilled with that file.
- Provide a deterministic fallback (`Merge branch '<src>' into <dst>`) when `MERGE_MSG` is absent/unreadable.
- Preserve any edits the user already made to the commit message — never silently overwrite user input.
- Keep the rest of the commit flow untouched: amending, normal commits, rebase/cherry-pick/revert states are out of scope.

**Non-Goals:**
- Rewording merge commits after the fact, or editing `MERGE_MSG` while the user is typing.
- Any new config/preference to toggle the behavior (matches Git's default; add a toggle only if user feedback demands it).
- Handling octopus merges beyond what `MERGE_MSG`/`MERGE_HEAD` already carry (Git itself writes the multi-parent template into `MERGE_MSG`, so reading the file covers this case for free).
- Changing how the commit itself is created — `create_commit` in `src/git-core/src/commit.rs:40` already finalizes merge state correctly.

## Decisions

### 1. Read `.git/MERGE_MSG` via `git-core`, not from the UI layer

- **What**: Add `pub fn prepared_merge_message(repo: &Repository) -> Result<Option<String>, GitError>` in `src/git-core/src/commit.rs`. It returns `Ok(Some(contents))` when `MERGE_MSG` exists and has non-empty content (after trimming trailing whitespace), `Ok(None)` if the file is absent, and an error only for unexpected I/O failures.
- **Why**: Keeps filesystem access and git-layout knowledge in `git-core` (consistent with `load_merge_head_parents`), lets the UI depend only on a typed API, and makes the behavior unit-testable with a real temp repo (as `commit.rs` tests already do).
- **Alternative considered**: Inlining the file read in `src-ui/src/main.rs`. Rejected: duplicates `.git/`-relative path logic that `commit.rs` already encapsulates, and makes it harder to test without spinning up the whole Iced app.

### 2. Synthesize a fallback when `MERGE_MSG` is missing

- **What**: Add `pub fn synthesize_merge_message(repo: &Repository) -> Result<String, GitError>` in the same module. It reads the current branch short name (via `repo.head()` peeled to a branch) and the `MERGE_HEAD` entries, then formats:
  - Single source, source resolves to a ref: `Merge branch '<source-short-name>' into <current-branch>`
  - Single source, no ref name: `Merge commit '<short-oid>' into <current-branch>`
  - Multiple sources: `Merge branches '<a>', '<b>' into <current-branch>` (comma-separated; use short OIDs for anonymous entries).
  - HEAD detached / no branch name: drop the `into …` suffix.
- **Why**: `MERGE_MSG` normally exists, but some flows (e.g. merges that completed with `--squash`, or users who manually deleted the file) won't have it. A synthesized default is better than an empty editor and matches git's own wording closely enough to be familiar. The i18n layer formats the UX text around it; the message body itself is English to match Git's on-disk convention and stay greppable in log output.
- **Alternative considered**: Falling back to a locale-translated message. Rejected: the committed message ends up in `git log` forever; keeping it in Git's conventional English form avoids polluting history with locale-specific strings and matches what CLI users expect.

### 3. "Default message" is an editor-seed, not a forced value

- **What**: Extend `CommitDialogState` with two additions:
  - `pub default_message: Option<String>` — the currently-suggested default (the prepared merge message).
  - When `message_editor` is untouched (either literally empty, or equal to `default_message`), `apply_message_edit` is free to overwrite it; once the user types something different, we stop re-seeding for the rest of this merge session.
  - Add `pub fn set_default_message(&mut self, default: String)` that only writes to `message` / `message_editor` if the current content is empty or matches the previous `default_message`. This makes the seeding idempotent across repeated state refreshes in `main.rs`.
- **Why**: The UI refresh loop calls `refresh_repository_after_action` repeatedly, so any seeding has to be re-entrant without clobbering edits. Comparing against `default_message` lets us distinguish "user hasn't touched it" from "user deleted our suggestion and typed their own".
- **Alternative considered**: A boolean `is_default_message_pristine` flag toggled on first `apply_message_edit`. Rejected: the current `apply_message_edit` is called for every keystroke including arrow-key cursor moves, which makes "did the user actually modify the text" brittle. Comparing the text itself is simpler and robust.

### 4. Trigger seeding from `main.rs` on state transition, not from a timer

- **What**: In `src-ui/src/main.rs`, after each `refresh_repository_after_action` (and in `open_commit_dialog`), check `repo.get_state()`. If the state is `Merging`:
  1. Call `git_core::commit::prepared_merge_message(&repo)`; fall back to `synthesize_merge_message(&repo)` when that returns `Ok(None)`.
  2. Pass the result to `state.commit_dialog.set_default_message(...)`.
  If the state leaves `Merging` (merge committed or aborted), clear `state.commit_dialog.default_message` so the next normal commit starts empty as today.
- **Why**: Repository state changes are already the trigger point for most merge-related UI updates (banner, conflict resolver open/close), so piggy-backing keeps the code in one place. Doing this only during refresh avoids racing with the user typing into the editor.
- **Alternative considered**: Watching `.git/MERGE_MSG` via `notify`. Rejected: adds a moving part for a file whose lifecycle is already well-defined by repository state transitions we already react to.

### 5. Clear `MERGE_MSG` handling stays in libgit2

- **What**: Do **not** manually delete `.git/MERGE_MSG` after commit. `repo_lock.cleanup_state()` already called in `create_commit` (`src/git-core/src/commit.rs:107`) removes `MERGE_HEAD`, `MERGE_MSG`, etc. We only own the in-memory seeding, not the disk state.
- **Why**: Single responsibility — disk cleanup is Git's concern; UI seed-reset happens when the repo state transitions out of `Merging`.

## Risks / Trade-offs

- **[Risk] Partial file read on Windows if git is mid-write**: Rare in practice because `MERGE_MSG` is written once before `git merge` returns. **Mitigation**: we only read `MERGE_MSG` when `repo.state() == Merging`, which git only reports after it has finished writing state files; treat read errors as `Ok(None)` and fall back to the synthesizer.
- **[Risk] User pastes something, then clears it, then sees our default reappear**: After `set_default_message` compares by content, an empty editor looks "untouched" so we re-seed. **Mitigation**: seeding is triggered by state transitions, not on every keystroke, and only runs once per `Merging` transition in practice. If we observe user complaints, we can add a "once the user cleared it, stay cleared" flag in a follow-up; deferred to keep this change scoped.
- **[Risk] Non-UTF-8 bytes in `MERGE_MSG`**: Unlikely (Git writes UTF-8), but possible with custom `prepare-commit-msg` hooks. **Mitigation**: use `String::from_utf8_lossy`-equivalent handling in `prepared_merge_message` so we never panic; if the content is lossy, still surface it — the user can edit.
- **[Trade-off] English fallback string**: Users running slio-git in Chinese UI will see an English merge message in the editor. **Mitigation**: this matches Git's own convention (the authoritative commit log string is English), and keeps committed history uniform; revisit if localization of Git output becomes a broader project concern.
- **[Risk] Large `MERGE_MSG` from octopus merges**: The file can grow with many parents/conflicts, but is still small in practice (KB-scale). **Mitigation**: none needed; we read it once per state transition.

## Migration Plan

- Purely additive behavior gated on `RepositoryState::Merging`; no schema, config, or on-disk format changes.
- **Rollback**: revert the change — pre-existing merges continue to work because `create_commit` already handles `Merging` state regardless of message origin.

## Open Questions

- Should the auto-populated message be visibly marked (e.g. a subtle "prepared by Git" hint above the editor) the first time it appears, or is the prefilled text self-explanatory? _Current decision_: no extra UI affordance in v1; prefilled text matches what CLI users already expect. Revisit if usability testing suggests confusion.
- Do we also want to extend the same prefill to the `Rebasing` / `Revert` / `CherryPick` repository states (which also stage analogous files like `REBASE_MSG`, `REVERT_HEAD`, `CHERRY_PICK_HEAD`)? _Current decision_: out of scope — this proposal is scoped to merges per the user's request; a follow-up can generalize the helper if the pattern proves useful.
