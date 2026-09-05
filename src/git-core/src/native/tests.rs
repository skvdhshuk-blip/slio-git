use super::{
    journal::{self, Kind},
    sequencer,
};
use crate::{RebaseTodoEntry, Repository};
use std::{fs, path::Path, process::Command};
use tempfile::TempDir;

fn git(path: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(path)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}
fn fixture() -> (TempDir, Repository, Vec<String>) {
    let dir = TempDir::new().unwrap();
    git(dir.path(), &["init", "-b", "main"]);
    git(dir.path(), &["config", "user.name", "Original Author"]);
    git(dir.path(), &["config", "user.email", "author@example.com"]);
    let mut ids = Vec::new();
    for (name, text) in [("a", "one\n"), ("b", "two\n"), ("c", "three\n")] {
        fs::write(dir.path().join(name), text).unwrap();
        git(dir.path(), &["add", name]);
        git(dir.path(), &["commit", "-m", name]);
        ids.push(git(dir.path(), &["rev-parse", "HEAD"]));
    }
    let repo = Repository::open(dir.path()).unwrap();
    (dir, repo, ids)
}
fn entry(id: &str, action: &str) -> RebaseTodoEntry {
    RebaseTodoEntry {
        action: action.into(),
        commit: id.into(),
        message: String::new(),
    }
}
fn start(
    repo: &Repository,
    base: Option<&str>,
    steps: &[RebaseTodoEntry],
) -> Result<bool, crate::GitError> {
    sequencer::start(repo, Kind::Rebase, base.map(str::to_owned), steps, false)
}
fn clean(path: &Path) {
    assert_eq!(git(path, &["status", "--porcelain"]), "");
}

#[test]
fn native_root_reordering_preserves_tree_authors_and_first_parent_order() {
    let (dir, repo, ids) = fixture();
    let old_tree = git(dir.path(), &["rev-parse", "HEAD^{tree}"]);
    assert!(
        start(
            &repo,
            None,
            &[
                entry(&ids[2], "pick"),
                entry(&ids[0], "pick"),
                entry(&ids[1], "pick")
            ]
        )
        .unwrap()
    );
    assert_eq!(
        git(dir.path(), &["log", "--reverse", "--format=%s"]),
        "c\na\nb"
    );
    assert_eq!(git(dir.path(), &["rev-parse", "HEAD^{tree}"]), old_tree);
    assert_eq!(
        git(dir.path(), &["log", "--format=%an <%ae>"]),
        "Original Author <author@example.com>\nOriginal Author <author@example.com>\nOriginal Author <author@example.com>"
    );
    clean(dir.path());
}

#[test]
fn native_fixup_squash_drop_and_empty_commits() {
    for action in ["fixup", "squash", "drop"] {
        let (dir, repo, ids) = fixture();
        assert!(
            start(
                &repo,
                None,
                &[
                    entry(&ids[0], "pick"),
                    entry(&ids[1], action),
                    entry(&ids[2], "pick")
                ]
            )
            .unwrap()
        );
        assert_eq!(git(dir.path(), &["rev-list", "--count", "HEAD"]), "2");
        let message = git(dir.path(), &["log", "--format=%B", "HEAD^"]);
        assert_eq!(message, if action == "squash" { "a\n\nb" } else { "a" });
        assert_eq!(dir.path().join("b").exists(), action != "drop");
        clean(dir.path());
    }
    let (dir, repo, ids) = fixture();
    git(dir.path(), &["commit", "--allow-empty", "-m", "empty"]);
    let empty = git(dir.path(), &["rev-parse", "HEAD"]);
    assert!(
        start(
            &repo,
            Some(&ids[0]),
            &[
                entry(&ids[1], "pick"),
                entry(&ids[2], "pick"),
                entry(&empty, "pick")
            ]
        )
        .unwrap()
    );
    assert_eq!(git(dir.path(), &["log", "-1", "--format=%s"]), "empty");
    assert_eq!(git(dir.path(), &["rev-list", "--count", "HEAD"]), "4");
}

#[test]
fn native_edit_uses_existing_amend_and_survives_reopening() {
    let (dir, repo, ids) = fixture();
    assert!(
        !start(
            &repo,
            Some(&ids[0]),
            &[entry(&ids[1], "reword"), entry(&ids[2], "pick")]
        )
        .unwrap()
    );
    assert_eq!(
        repo.get_state(),
        crate::repository::RepositoryState::Rebasing
    );
    let head = git(dir.path(), &["rev-parse", "HEAD"]);
    crate::commit::amend_commit(&repo, &head, "renamed\n").unwrap();
    drop(repo);
    let reopened = Repository::open(dir.path()).unwrap();
    assert!(crate::rebase::rebase_continue(&reopened).unwrap().success);
    assert_eq!(
        git(dir.path(), &["log", "--reverse", "--format=%s"]),
        "a\nrenamed\nc"
    );
    clean(dir.path());
}

fn conflict_fixture() -> (TempDir, Repository, String, String) {
    let (dir, repo, ids) = fixture();
    git(dir.path(), &["checkout", "-b", "other", &ids[0]]);
    fs::write(dir.path().join("a"), "theirs\n").unwrap();
    git(dir.path(), &["commit", "-am", "theirs"]);
    let theirs = git(dir.path(), &["rev-parse", "HEAD"]);
    git(dir.path(), &["checkout", "main"]);
    fs::write(dir.path().join("a"), "ours\n").unwrap();
    git(dir.path(), &["commit", "-am", "ours"]);
    let ours = git(dir.path(), &["rev-parse", "HEAD"]);
    (dir, repo, ours, theirs)
}

#[test]
fn native_cherry_pick_conflict_continue_and_abort_preserve_unrelated_files() {
    for abort in [false, true] {
        let (dir, repo, ours, theirs) = conflict_fixture();
        fs::write(dir.path().join("b"), "unrelated dirty\n").unwrap();
        assert!(
            !sequencer::start(
                &repo,
                Kind::CherryPick,
                Some(ours.clone()),
                &[entry(&theirs, "pick")],
                false
            )
            .unwrap()
        );
        assert!(
            !crate::index::get_conflicted_files(&repo)
                .unwrap()
                .is_empty()
        );
        fs::write(dir.path().join("notes"), "unrelated new\n").unwrap();
        fs::write(dir.path().join("a"), "resolved\n").unwrap();
        if abort {
            crate::commit_actions::abort_in_progress_commit_action(
                &repo,
                crate::commit_actions::InProgressCommitActionKind::CherryPick,
            )
            .unwrap();
            assert_eq!(git(dir.path(), &["rev-parse", "HEAD"]), ours);
            assert_eq!(fs::read_to_string(dir.path().join("a")).unwrap(), "ours\n");
        } else {
            crate::commit_actions::continue_in_progress_commit_action(
                &repo,
                crate::commit_actions::InProgressCommitActionKind::CherryPick,
            )
            .unwrap();
            assert_eq!(git(dir.path(), &["log", "-1", "--format=%s"]), "theirs");
            assert_eq!(
                fs::read_to_string(dir.path().join("a")).unwrap(),
                "resolved\n"
            );
        }
        assert_eq!(
            fs::read_to_string(dir.path().join("b")).unwrap(),
            "unrelated dirty\n"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("notes")).unwrap(),
            "unrelated new\n"
        );
    }
}

#[test]
fn native_rebase_conflict_skip_returns_to_the_current_tip() {
    let (dir, repo, ours, theirs) = conflict_fixture();
    assert!(!start(&repo, Some(&theirs), &[entry(&ours, "pick")]).unwrap());
    assert!(crate::rebase::rebase_skip(&repo).unwrap().success);
    assert_eq!(git(dir.path(), &["rev-parse", "HEAD"]), theirs);
    clean(dir.path());
}

#[test]
fn native_recovers_each_persistent_boundary_without_duplicate_commits() {
    for boundary in [
        "journal",
        "file",
        "index",
        "head",
        "publish-ref",
        "publish-head",
    ] {
        for hit in 0..12 {
            let (dir, repo, ids) = fixture();
            journal::fail_after(boundary, hit);
            let result = start(
                &repo,
                None,
                &[
                    entry(&ids[2], "pick"),
                    entry(&ids[0], "pick"),
                    entry(&ids[1], "pick"),
                ],
            );
            journal::fail_after("disabled", 0);
            if result.is_err() {
                assert!(super::active(&repo), "{boundary} {hit}: {result:?}");
                drop(repo);
                let reopened = Repository::open(dir.path()).unwrap();
                assert!(
                    crate::rebase::rebase_continue(&reopened).unwrap().success,
                    "{boundary} {hit}"
                );
            }
            assert_eq!(
                git(dir.path(), &["log", "--reverse", "--format=%s"]),
                "c\na\nb",
                "{boundary} {hit}: {result:?}"
            );
            assert_eq!(git(dir.path(), &["rev-list", "--count", "HEAD"]), "3");
            clean(dir.path());
        }
    }
}

#[test]
fn native_external_changes_are_rejected_without_overwriting_them() {
    for change in ["file", "index", "branch"] {
        let (dir, repo, ids) = fixture();
        journal::fail_after("file", 0);
        assert!(
            start(
                &repo,
                None,
                &[
                    entry(&ids[0], "pick"),
                    entry(&ids[1], "pick"),
                    entry(&ids[2], "pick")
                ]
            )
            .is_err()
        );
        journal::fail_after("disabled", 0);
        match change {
            "file" => {
                fs::write(dir.path().join("a"), "outside\n").unwrap();
            }
            "index" => {
                fs::write(dir.path().join("outside"), "outside\n").unwrap();
                git(dir.path(), &["add", "outside"]);
            }
            _ => {
                git(dir.path(), &["update-ref", "refs/heads/main", &ids[0]]);
            }
        }
        assert!(sequencer::continue_operation(&repo).is_err(), "{change}");
        assert!(super::active(&repo));
        if change == "file" {
            assert_eq!(
                fs::read_to_string(dir.path().join("a")).unwrap(),
                "outside\n"
            );
        }
    }
}

#[test]
fn native_merge_fast_forward_noff_squash_and_conflict() {
    for (no_ff, squash, count) in [(false, false, "3"), (true, false, "4"), (false, true, "2")] {
        let (dir, repo, ids) = fixture();
        git(dir.path(), &["reset", "--hard", &ids[0]]);
        super::merge::start(&repo, &ids[2], no_ff, squash, false).unwrap();
        if squash {
            crate::commit::create_commit(&repo, "squashed", "", "").unwrap();
        }
        assert_eq!(git(dir.path(), &["rev-list", "--count", "HEAD"]), count);
        clean(dir.path());
    }
    let (dir, repo, _, theirs) = conflict_fixture();
    assert!(super::merge::start(&repo, &theirs, false, false, false).is_err());
    assert_eq!(
        repo.get_state(),
        crate::repository::RepositoryState::Merging
    );
    fs::write(dir.path().join("a"), "resolved\n").unwrap();
    crate::commit::create_commit(&repo, "merged", "", "").unwrap();
    assert_eq!(
        git(dir.path(), &["log", "-1", "--format=%P"])
            .split_whitespace()
            .count(),
        2
    );
    clean(dir.path());
}

#[test]
fn native_rename_modes_symlinks_and_directory_transitions_match_git() {
    for boundary_hit in 0..2 {
        let (dir, repo, ids) = fixture();
        git(dir.path(), &["mv", "a", "renamed"]);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                dir.path().join("renamed"),
                fs::Permissions::from_mode(0o755),
            )
            .unwrap();
            std::os::unix::fs::symlink("renamed", dir.path().join("link")).unwrap();
        }
        git(dir.path(), &["add", "-A"]);
        git(dir.path(), &["commit", "-m", "rename and mode"]);
        let renamed = git(dir.path(), &["rev-parse", "HEAD"]);
        fs::remove_file(dir.path().join("b")).unwrap();
        fs::create_dir(dir.path().join("b")).unwrap();
        fs::write(dir.path().join("b/child"), "nested\n").unwrap();
        git(dir.path(), &["add", "-A"]);
        git(dir.path(), &["commit", "-m", "file to directory"]);
        let directory = git(dir.path(), &["rev-parse", "HEAD"]);
        fs::remove_file(dir.path().join("b/child")).unwrap();
        fs::remove_dir(dir.path().join("b")).unwrap();
        fs::write(dir.path().join("b"), "file again\n").unwrap();
        git(dir.path(), &["add", "-A"]);
        git(dir.path(), &["commit", "-m", "directory to file"]);
        let final_id = git(dir.path(), &["rev-parse", "HEAD"]);
        let before = git(dir.path(), &["ls-tree", "-r", "HEAD"]);
        journal::fail_after("directory-removed", boundary_hit);
        let result = start(
            &repo,
            Some(&ids[2]),
            &[
                entry(&renamed, "pick"),
                entry(&directory, "pick"),
                entry(&final_id, "pick"),
            ],
        );
        journal::fail_after("disabled", 0);
        assert!(
            result.is_err(),
            "the directory replacement boundary must be exercised"
        );
        let reopened = Repository::open(dir.path()).unwrap();
        assert!(crate::rebase::rebase_continue(&reopened).unwrap().success);
        assert_eq!(git(dir.path(), &["ls-tree", "-r", "HEAD"]), before);
        clean(dir.path());
    }
}

#[cfg(feature = "app-store")]
#[test]
fn mas_leaves_foreign_conflicts_untouched() {
    for operation in ["merge", "cherry-pick", "rebase"] {
        let (dir, repo, _, theirs) = conflict_fixture();
        let output = Command::new("git")
            .args([operation, &theirs])
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert!(!output.status.success());
        let before_head = git(dir.path(), &["rev-parse", "HEAD"]);
        let before_index = fs::read(dir.path().join(".git/index")).unwrap();
        let before_file = fs::read(dir.path().join("a")).unwrap();
        assert!(crate::index::stage_all(&repo).is_err());
        assert!(
            crate::diff::resolve_conflict(
                &repo,
                Path::new("a"),
                crate::diff::ConflictResolution::Ours
            )
            .is_err()
        );
        assert!(crate::commit::create_commit(&repo, "foreign", "", "").is_err());
        assert!(crate::rebase::rebase_continue(&repo).is_err());
        assert!(crate::rebase::rebase_abort(&repo).is_err());
        assert_eq!(git(dir.path(), &["rev-parse", "HEAD"]), before_head);
        assert_eq!(
            fs::read(dir.path().join(".git/index")).unwrap(),
            before_index
        );
        assert_eq!(fs::read(dir.path().join("a")).unwrap(), before_file);
        assert!(!super::active(&repo));
        assert_ne!(
            repo.get_state(),
            crate::repository::RepositoryState::Clean,
            "{operation}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn exported_email_imports_with_standard_git_am() {
    let (dir, repo, ids) = fixture();
    let output = TempDir::new().unwrap();
    let patch = output.path().join("commit.patch");
    crate::commit_actions::export_commit_patch(&repo, &ids[2], &patch).unwrap();
    git(dir.path(), &["checkout", "-b", "imported", &ids[1]]);
    git(dir.path(), &["am", patch.to_str().unwrap()]);
    assert_eq!(
        git(dir.path(), &["rev-parse", "HEAD^{tree}"]),
        git(dir.path(), &["rev-parse", &format!("{}^{{tree}}", ids[2])])
    );
    assert_eq!(
        git(dir.path(), &["log", "-1", "--format=%B%an <%ae> %at"]),
        git(
            dir.path(),
            &["log", "-1", "--format=%B%an <%ae> %at", &ids[2]]
        )
    );
    clean(dir.path());
}

#[cfg(feature = "app-store")]
#[test]
fn worktree_deletion_requires_full_identity_and_clean_unlocked_directory() {
    let (dir, repo, _) = fixture();
    let linked = dir.path().join("linked");
    git(
        dir.path(),
        &["worktree", "add", "-b", "linked", linked.to_str().unwrap()],
    );
    let unrelated = TempDir::new().unwrap();
    let impostor = unrelated.path().join("linked");
    fs::create_dir(&impostor).unwrap();
    assert!(crate::worktree::remove_worktree(&repo, &impostor).is_err());
    assert!(linked.exists());
    fs::write(linked.join("untracked"), "keep").unwrap();
    assert!(crate::worktree::remove_worktree(&repo, &linked).is_err());
    fs::remove_file(linked.join("untracked")).unwrap();
    git(dir.path(), &["worktree", "lock", linked.to_str().unwrap()]);
    assert!(crate::worktree::remove_worktree(&repo, &linked).is_err());
    git(
        dir.path(),
        &["worktree", "unlock", linked.to_str().unwrap()],
    );
    crate::worktree::remove_worktree(&repo, &linked).unwrap();
    assert!(!linked.exists());
    assert!(impostor.exists());
}
