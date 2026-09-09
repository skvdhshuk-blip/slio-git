//! Real Git repositories protect the repaired history, recovery and push contracts.
mod test_helpers;
use git_core::*;
use std::{fs, path::Path, process::Command};
use tempfile::TempDir;
use test_helpers::TestRepo;

fn git(path: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().into()
}
fn linear() -> (TestRepo, Vec<String>) {
    let fixture = TestRepo::new().unwrap();
    let mut ids = Vec::new();
    for (file, content, message) in [
        ("a.txt", "a\n", "A"),
        ("b.txt", "b\n", "B"),
        ("c.txt", "c\n", "C"),
    ] {
        fixture.add_and_commit(file, content, message).unwrap();
        ids.push(git(fixture.path(), &["rev-parse", "HEAD"]));
    }
    (fixture, ids)
}

#[test]
fn patch_roundtrip_preserves_binary_and_non_utf8_content() {
    let fixture = TestRepo::new().unwrap();
    fixture.add_and_commit("base.txt", "base", "base").unwrap();
    let base = git(fixture.path(), &["rev-parse", "HEAD"]);
    let bytes = b"\0\xff\xfe\x01binary\0";
    fs::write(fixture.path().join("image.bin"), bytes).unwrap();
    fs::write(fixture.path().join("legacy.txt"), b"old encoding \xff\n").unwrap();
    git(fixture.path(), &["add", "."]);
    git(fixture.path(), &["commit", "-m", "binary and legacy text"]);
    let tip = git(fixture.path(), &["rev-parse", "HEAD"]);
    let out = TempDir::new().unwrap();
    let patch = out.path().join("change.patch");
    let repo = Repository::open(fixture.path()).unwrap();
    export_commit_patch(&repo, &tip, &patch).unwrap();
    git(fixture.path(), &["reset", "--hard", &base]);
    git(fixture.path(), &["am", patch.to_str().unwrap()]);
    assert_eq!(fs::read(fixture.path().join("image.bin")).unwrap(), bytes);
    assert_eq!(
        fs::read(fixture.path().join("legacy.txt")).unwrap(),
        b"old encoding \xff\n"
    );
}

#[test]
fn reword_and_squash_preserve_author_tree_and_commit_count() {
    for squash in [false, true] {
        let (fixture, ids) = linear();
        let original_tree = git(fixture.path(), &["rev-parse", "HEAD^{tree}"]);
        let original_author = git(
            fixture.path(),
            &["show", "-s", "--format=%an <%ae>", &ids[1]],
        );
        git(
            fixture.path(),
            &["config", "user.name", "Different Committer"],
        );
        let repo = Repository::open(fixture.path()).unwrap();
        if squash {
            squash_commit_to_previous(&repo, &ids[1]).unwrap();
        } else {
            edit_commit_message(&repo, &ids[1]).unwrap();
            let stopped = git(fixture.path(), &["rev-parse", "HEAD"]);
            amend_commit(&repo, &stopped, "New B").unwrap();
            assert!(rebase_continue(&repo).unwrap().success);
        }
        assert_eq!(
            git(fixture.path(), &["rev-parse", "HEAD^{tree}"]),
            original_tree
        );
        assert_eq!(
            git(fixture.path(), &["rev-list", "--count", "HEAD"]),
            if squash { "2" } else { "3" }
        );
        assert_eq!(
            git(
                fixture.path(),
                &["show", "-s", "--format=%an <%ae>", "HEAD~1"]
            ),
            original_author
        );
        assert!(git(fixture.path(), &["status", "--porcelain"]).is_empty());
        if !squash {
            assert_eq!(
                git(fixture.path(), &["show", "-s", "--format=%s", "HEAD~1"]),
                "New B"
            );
        }
    }
}

#[test]
fn edit_message_survives_reopen_and_publishes_only_after_continue() {
    let (fixture, ids) = linear();
    let branch = git(fixture.path(), &["symbolic-ref", "HEAD"]);
    let repo = Repository::open(fixture.path()).unwrap();
    assert_eq!(
        edit_commit_message(&repo, &ids[1]).unwrap(),
        git_core::commit_actions::RewriteExecution::InProgress
    );
    assert_eq!(
        repo.get_state(),
        git_core::repository::RepositoryState::Rebasing
    );
    assert_eq!(git(fixture.path(), &["rev-parse", &branch]), ids[2]);
    let stopped = git(fixture.path(), &["rev-parse", "HEAD"]);
    amend_commit(&repo, &stopped, "Edited B").unwrap();
    drop(repo);
    let repo = Repository::open(fixture.path()).unwrap();
    assert!(rebase_continue(&repo).unwrap().success);
    assert_eq!(git(fixture.path(), &["symbolic-ref", "HEAD"]), branch);
    assert_eq!(
        git(fixture.path(), &["show", "-s", "--format=%s", "HEAD~1"]),
        "Edited B"
    );
    assert_eq!(git(fixture.path(), &["rev-list", "--count", "HEAD"]), "3");
    assert!(git(fixture.path(), &["status", "--porcelain"]).is_empty());
}

#[test]
fn interactive_squash_and_fixup_remove_parent_commit() {
    for action in ["squash", "fixup"] {
        let (fixture, ids) = linear();
        let repo = Repository::open(fixture.path()).unwrap();
        let mut plan = prepare_interactive_rebase_plan(&repo, &ids[0]).unwrap();
        plan.entries[1].action = action.into();
        start_interactive_rebase(&repo, None, &plan.entries).unwrap();
        assert_eq!(git(fixture.path(), &["rev-list", "--count", "HEAD"]), "2");
        assert!(get_rebase_status(&repo).unwrap().is_none());
        assert!(git(fixture.path(), &["status", "--porcelain"]).is_empty());
        assert!(fixture.path().join("b.txt").exists());
    }
}

#[test]
fn interactive_conflicts_can_continue_skip_or_abort_after_reopen() {
    let recoveries = if cfg!(feature = "app-store") {
        vec!["continue", "skip", "abort", "edit"]
    } else {
        // System Git commits a resolved edit conflict when continuing it.
        vec!["continue", "skip", "abort"]
    };
    for recovery in recoveries {
        let fixture = TestRepo::new().unwrap();
        let mut ids = Vec::new();
        for value in ["A", "B", "C"] {
            fixture.add_and_commit("shared.txt", value, value).unwrap();
            ids.push(git(fixture.path(), &["rev-parse", "HEAD"]));
        }
        let repo = Repository::open(fixture.path()).unwrap();
        let mut plan = prepare_interactive_rebase_plan(&repo, &ids[1]).unwrap();
        plan.entries[0].action = "drop".into();
        if recovery == "edit" {
            plan.entries[1].action = "edit".into();
        }
        start_interactive_rebase(&repo, plan.base_ref.as_deref(), &plan.entries).unwrap();
        assert!(
            has_rebase_conflicts(&repo).unwrap(),
            "{recovery}: {:?}",
            get_rebase_status(&repo)
        );
        assert!(
            rebase_continue(&repo).is_err(),
            "unresolved markers must not be committed"
        );
        fs::write(fixture.path().join("notes.txt"), "keep me").unwrap();
        drop(repo);
        let repo = Repository::open(fixture.path()).unwrap();
        match recovery {
            "continue" | "edit" => {
                fs::write(fixture.path().join("shared.txt"), "resolved").unwrap();
                let result = rebase_continue(&repo).unwrap();
                assert_eq!(
                    result.success,
                    recovery != "edit",
                    "{recovery}: {}",
                    result.message
                );
                if recovery == "edit" {
                    assert!(get_rebase_status(&repo).unwrap().is_some());
                    let head = git(fixture.path(), &["rev-parse", "HEAD"]);
                    amend_commit(&repo, &head, "Resolved and edited").unwrap();
                    assert!(rebase_continue(&repo).unwrap().success);
                    assert_eq!(
                        git(fixture.path(), &["log", "-1", "--format=%s"]),
                        "Resolved and edited"
                    );
                }
                assert_eq!(
                    fs::read_to_string(fixture.path().join("shared.txt")).unwrap(),
                    "resolved"
                );
            }
            "skip" => {
                assert!(rebase_skip(&repo).unwrap().success);
            }
            _ => {
                rebase_abort(&repo).unwrap();
                assert_eq!(git(fixture.path(), &["rev-parse", "HEAD"]), ids[2]);
            }
        }
        assert_eq!(
            fs::read_to_string(fixture.path().join("notes.txt")).unwrap(),
            "keep me"
        );
        assert!(get_rebase_status(&repo).unwrap().is_none());
    }
}

#[test]
fn cherry_pick_and_revert_abort_preserve_untracked_files() {
    for kind in [
        InProgressCommitActionKind::CherryPick,
        InProgressCommitActionKind::Revert,
    ] {
        let fixture = TestRepo::new().unwrap();
        fixture.add_and_commit("shared.txt", "A", "A").unwrap();
        fixture.add_and_commit("shared.txt", "B", "B").unwrap();
        let selected = git(fixture.path(), &["rev-parse", "HEAD"]);
        if kind == InProgressCommitActionKind::CherryPick {
            git(fixture.path(), &["reset", "--hard", "HEAD~1"]);
        }
        fixture.add_and_commit("shared.txt", "C", "C").unwrap();
        let repo = Repository::open(fixture.path()).unwrap();
        let result = match kind {
            InProgressCommitActionKind::CherryPick => cherry_pick_commit(&repo, &selected),
            InProgressCommitActionKind::Revert => revert_commit(&repo, &selected),
        };
        assert!(result.is_err());
        fs::write(fixture.path().join("notes.txt"), "keep me").unwrap();
        abort_in_progress_commit_action(&repo, kind).unwrap();
        assert!(fixture.path().join("notes.txt").exists());
        assert_eq!(
            fs::read_to_string(fixture.path().join("shared.txt")).unwrap(),
            "C"
        );
    }
}

#[test]
fn force_with_lease_refuses_unfetched_remote_updates() {
    let (alice, _) = linear();
    let remote = TempDir::new().unwrap();
    git(remote.path(), &["init", "--bare"]);
    git(
        alice.path(),
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    let branch = git(alice.path(), &["branch", "--show-current"]);
    git(alice.path(), &["push", "-u", "origin", &branch]);
    let bob = TempDir::new().unwrap();
    git(
        bob.path(),
        &["clone", "-b", &branch, remote.path().to_str().unwrap(), "."],
    );
    git(bob.path(), &["config", "user.name", "Bob"]);
    git(bob.path(), &["config", "user.email", "bob@example.test"]);
    fs::write(bob.path().join("bob.txt"), "Bob").unwrap();
    git(bob.path(), &["add", "."]);
    git(bob.path(), &["commit", "-m", "Bob"]);
    git(bob.path(), &["push"]);
    let expected = git(remote.path(), &["rev-parse", &branch]);
    alice.add_and_commit("alice.txt", "Alice", "Alice").unwrap();
    let repo = Repository::open(alice.path()).unwrap();
    assert!(git_core::remote::force_push(&repo, "origin", &branch).is_err());
    assert_eq!(git(remote.path(), &["rev-parse", &branch]), expected);
}

#[test]
fn push_to_here_keeps_the_lease_from_confirmation_even_after_fetch() {
    let (fixture, ids) = linear();
    let remote = TempDir::new().unwrap();
    git(remote.path(), &["init", "--bare"]);
    let branch = git(fixture.path(), &["branch", "--show-current"]);
    git(
        fixture.path(),
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    git(fixture.path(), &["push", "-u", "origin", &branch]);
    let repo = Repository::open(fixture.path()).unwrap();
    let target = resolve_push_current_branch_target(&repo, &ids[1]).unwrap();
    fixture.add_and_commit("d.txt", "D", "D").unwrap();
    git(fixture.path(), &["push"]);
    git(fixture.path(), &["fetch"]);
    let expected = git(remote.path(), &["rev-parse", &branch]);
    assert!(push_current_branch_to_commit(&repo, &target).is_err());
    assert_eq!(git(remote.path(), &["rev-parse", &branch]), expected);
}

#[test]
fn force_push_uses_destination_branch_lease_and_sets_upstream() {
    let (fixture, ids) = linear();
    let remote = TempDir::new().unwrap();
    git(remote.path(), &["init", "--bare"]);
    git(
        fixture.path(),
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    let branch = git(fixture.path(), &["branch", "--show-current"]);
    git(
        fixture.path(),
        &["push", "origin", &format!("{branch}:release")],
    );
    git(fixture.path(), &["reset", "--hard", &ids[1]]);
    let repo = Repository::open(fixture.path()).unwrap();
    git_core::remote::push_with_options(
        &repo,
        "origin",
        &branch,
        git_core::remote::PushOptions {
            target_branch: Some("release"),
            force_with_lease: true,
            set_upstream: true,
            ..Default::default()
        },
        None,
    )
    .unwrap();
    assert_eq!(git(remote.path(), &["rev-parse", "release"]), ids[1]);
    assert_eq!(
        git(
            fixture.path(),
            &["config", &format!("branch.{branch}.merge")]
        ),
        "refs/heads/release"
    );
}

#[test]
#[cfg(unix)]
fn push_reports_server_hook_rejection() {
    use std::os::unix::fs::PermissionsExt;
    let (fixture, ids) = linear();
    let remote = TempDir::new().unwrap();
    git(remote.path(), &["init", "--bare"]);
    let branch = git(fixture.path(), &["branch", "--show-current"]);
    git(
        fixture.path(),
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    git(fixture.path(), &["push", "-u", "origin", &branch]);
    let hook = remote.path().join("hooks/pre-receive");
    fs::write(&hook, "#!/bin/sh\necho 'review rejection' >&2\nexit 1\n").unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    // Local libgit2 transport has no receive-pack process and does not run
    // server hooks. Exercise a real Git protocol server for rejection semantics.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    struct Daemon(std::process::Child);
    impl Drop for Daemon {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let _daemon = Daemon(
        Command::new("git")
            .args([
                "daemon",
                "--reuseaddr",
                "--export-all",
                "--enable=receive-pack",
                "--listen=127.0.0.1",
                &format!("--port={port}"),
                &format!("--base-path={}", remote.path().parent().unwrap().display()),
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(
            std::time::Instant::now() < deadline,
            "git daemon did not start"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    git(
        fixture.path(),
        &[
            "remote",
            "set-url",
            "origin",
            &format!(
                "git://127.0.0.1:{port}/{}",
                remote.path().file_name().unwrap().to_str().unwrap()
            ),
        ],
    );
    let repo = Repository::open(fixture.path()).unwrap();
    let target = resolve_push_current_branch_target(&repo, &ids[1]).unwrap();
    assert!(push_current_branch_to_commit(&repo, &target).is_err());
    assert_eq!(git(remote.path(), &["rev-parse", &branch]), ids[2]);
}

#[test]
fn rebase_skip_finishes_all_remaining_commits_and_keeps_notes() {
    let fixture = TestRepo::new().unwrap();
    fixture
        .add_and_commit("shared.txt", "base", "base")
        .unwrap();
    let main = git(fixture.path(), &["branch", "--show-current"]);
    git(fixture.path(), &["checkout", "-b", "feature"]);
    fixture
        .add_and_commit("shared.txt", "feature", "conflicting")
        .unwrap();
    fixture
        .add_and_commit("later.txt", "later", "later")
        .unwrap();
    fixture.add_and_commit("last.txt", "last", "last").unwrap();
    git(fixture.path(), &["checkout", &main]);
    fixture
        .add_and_commit("shared.txt", "main", "main")
        .unwrap();
    git(fixture.path(), &["checkout", "feature"]);
    let repo = Repository::open(fixture.path()).unwrap();
    let _ = rebase_start(&repo, &main);
    assert!(has_rebase_conflicts(&repo).unwrap());
    fs::write(fixture.path().join("notes.txt"), "keep me").unwrap();
    assert!(rebase_skip(&repo).unwrap().success);
    assert!(get_rebase_status(&repo).unwrap().is_none());
    assert_eq!(
        fs::read_to_string(fixture.path().join("shared.txt")).unwrap(),
        "main"
    );
    for path in ["later.txt", "last.txt", "notes.txt"] {
        assert!(fixture.path().join(path).exists());
    }
    assert_eq!(
        git(fixture.path(), &["branch", "--show-current"]),
        "feature"
    );
}

#[test]
fn concurrent_patch_staging_keeps_repositories_isolated() {
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|id| {
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let fixture = TestRepo::new().unwrap();
                let path = format!("file-{id}.txt");
                fixture.add_and_commit(&path, "A\nB\n", "base").unwrap();
                fs::write(fixture.path().join(&path), format!("A\n{id}\nB\n")).unwrap();
                let repo = Repository::open(fixture.path()).unwrap();
                let hunks = get_file_hunks(&repo, Path::new(&path)).unwrap();
                let line = hunks[0]
                    .lines
                    .iter()
                    .position(|line| line.origin == '+')
                    .unwrap();
                barrier.wait();
                stage_lines(&repo, Path::new(&path), 0, &[line], LineSide::New).unwrap();
                assert_eq!(
                    git(fixture.path(), &["show", &format!(":{path}")]),
                    format!("A\n{id}\nB")
                );
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
}

#[test]
fn commit_action_continue_preserves_unrelated_files_and_rejects_markers() {
    for kind in [
        InProgressCommitActionKind::CherryPick,
        InProgressCommitActionKind::Revert,
    ] {
        let fixture = TestRepo::new().unwrap();
        fixture.add_and_commit("shared.txt", "A", "A").unwrap();
        fixture.add_and_commit("shared.txt", "B", "B").unwrap();
        let selected = git(fixture.path(), &["rev-parse", "HEAD"]);
        if kind == InProgressCommitActionKind::CherryPick {
            git(fixture.path(), &["reset", "--hard", "HEAD~1"]);
        }
        fixture.add_and_commit("shared.txt", "C", "C").unwrap();
        fixture
            .add_and_commit("unrelated.txt", "original", "unrelated")
            .unwrap();
        let repo = Repository::open(fixture.path()).unwrap();
        let result = match kind {
            InProgressCommitActionKind::CherryPick => cherry_pick_commit(&repo, &selected),
            InProgressCommitActionKind::Revert => revert_commit(&repo, &selected),
        };
        assert!(result.is_err());
        let head = git(fixture.path(), &["rev-parse", "HEAD"]);
        fs::write(fixture.path().join("notes.txt"), "keep me").unwrap();
        fs::write(fixture.path().join("unrelated.txt"), "in progress").unwrap();
        assert!(continue_in_progress_commit_action(&repo, kind).is_err());
        assert_eq!(git(fixture.path(), &["rev-parse", "HEAD"]), head);
        fs::write(fixture.path().join("shared.txt"), "resolved").unwrap();
        continue_in_progress_commit_action(&repo, kind).unwrap();
        assert_eq!(
            git(fixture.path(), &["show", "HEAD:unrelated.txt"]),
            "original"
        );
        assert_eq!(
            git(fixture.path(), &["show", "HEAD:shared.txt"]),
            "resolved"
        );
        let status = git(fixture.path(), &["status", "--porcelain"]);
        assert!(status.contains("?? notes.txt"), "{status}");
        assert!(status.contains("M unrelated.txt"), "{status}");
        assert!(git(fixture.path(), &["diff", "--cached", "--name-only"]).is_empty());
    }
}
