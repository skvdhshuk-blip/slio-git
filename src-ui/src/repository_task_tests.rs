use super::*;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn git(path: &Path, args: &[&str]) -> String {
    let r = Command::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    String::from_utf8_lossy(&r.stdout).trim().to_string()
}
fn fixture(name: &str) -> (tempfile::TempDir, PathBuf, Repository) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let channel = if cfg!(feature = "app-store") {
        "mas"
    } else {
        "desktop"
    };
    let p = root.join(format!("ui-{channel}-{name}"));
    fs::create_dir_all(&p).unwrap();
    git(&p, &["init", "-b", "main"]);
    git(&p, &["config", "user.name", "Review UI"]);
    git(&p, &["config", "user.email", "ui@example.invalid"]);
    git(&p, &["config", "commit.gpgsign", "false"]);
    fs::write(p.join("base.txt"), "base\n").unwrap();
    git(&p, &["add", "base.txt"]);
    git(&p, &["commit", "-m", "base"]);
    let repo = Repository::discover(&p).unwrap();
    (dir, p, repo)
}
#[test]
fn review_late_refresh_cannot_cross_repository_identity() {
    let (_da, a, ra) = fixture("late-a");
    let (_db, b, rb) = fixture("late-b");
    fs::write(a.join("only-a.txt"), "a\n").unwrap();
    fs::write(b.join("only-b.txt"), "b\n").unwrap();
    let mut state = AppState::new();
    state.set_repository(ra, &i18n::EN);
    let session = repository_session(&state);
    let late = run_refresh_blocking(a.clone()).unwrap();
    state.set_repository(rb, &i18n::EN);
    let _ = update(
        &mut state,
        scope_repository_result(session, Message::RefreshComplete(Ok(late))),
    );
    let names: Vec<_> = state
        .untracked_files
        .iter()
        .map(|c| c.path.as_str())
        .collect();
    println!(
        "active={:?}; presented={names:?}",
        state.current_repository.as_ref().unwrap().path()
    );
    assert_eq!(
        names,
        vec!["only-b.txt"],
        "late results from A must not replace B's visible changes"
    );
}
#[test]
fn review_late_commit_and_push_after_close_does_not_panic() {
    let (_dir, p, repo) = fixture("late-commit-close");
    fs::write(p.join("new.txt"), "new\n").unwrap();
    git(&p, &["add", "new.txt"]);
    let mut state = AppState::new();
    state.set_repository(repo, &i18n::EN);
    let session = repository_session(&state);
    let late = run_commit_blocking(p, "new commit".into(), None).unwrap();
    let _ = update(&mut state, Message::CloseRepository);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = update(
            &mut state,
            scope_repository_result(session, Message::CommitComplete(Ok(late), true)),
        );
    }));
    println!("late completion panicked={}", result.is_err());
    assert!(result.is_ok());
}
#[test]
fn review_commit_and_push_uses_configured_upstream() {
    let (_dir, p, _) = fixture("upstream-push");
    let bare = p.parent().unwrap().join(if cfg!(feature = "app-store") {
        "ui-mas-upstream.git"
    } else {
        "ui-desktop-upstream.git"
    });
    git(&p, &["init", "--bare", bare.to_str().unwrap()]);
    git(&p, &["remote", "add", "company", bare.to_str().unwrap()]);
    git(&p, &["push", "-u", "company", "main:release"]);
    fs::write(p.join("base.txt"), "next\n").unwrap();
    git(&p, &["add", "base.txt"]);
    git(&p, &["commit", "-m", "next"]);
    let result = run_push_blocking(p.clone(), Some("main".into()));
    println!("push configured company/release={result:?}");
    assert!(result.is_ok());
    assert_eq!(
        git(&bare, &["rev-parse", "refs/heads/release"]),
        git(&p, &["rev-parse", "HEAD"])
    );
}

#[test]
fn reopened_repository_rejects_old_success_and_failure_messages() {
    let (_dir, p, repo) = fixture("reopened");
    let mut state = AppState::new();
    state.set_repository(repo.clone(), &i18n::EN);
    let old = repository_session(&state);
    let stale = run_refresh_blocking(p.clone()).unwrap();
    let _ = update(&mut state, Message::CloseRepository);
    fs::write(p.join("fresh.txt"), b"fresh\n").unwrap();
    state.set_repository(repo, &i18n::EN);
    let generation = state.repository_generation;
    for message in [
        Message::RefreshComplete(Ok(stale.clone())),
        Message::GitOpComplete(Err("old staging error".into()), "old success".into()),
        Message::CommitComplete(Err("old commit error".into()), true),
        Message::PushComplete(Err("old push error".into())),
        Message::CheckoutComplete(Err("old checkout error".into())),
        Message::TagPushCompleted(Err(("old-tag".into(), "origin".into(), "old error".into()))),
        Message::CommitDialogMessage(CommitDialogMessage::GenerateCommitMessageResult(Ok(
            "old message".into(),
        ))),
    ] {
        let _ = update(&mut state, scope_repository_result(old.clone(), message));
        assert_eq!(state.repository_generation, generation);
        assert_eq!(state.untracked_files[0].path, "fresh.txt");
        assert!(state.error_message.is_none());
        assert_ne!(state.commit_dialog.message, "old message");
    }
    // A completion from the new session must still be applied.
    fs::write(p.join("newer.txt"), b"newer\n").unwrap();
    let current = scope_repository_result(
        repository_session(&state),
        Message::RefreshComplete(Ok(run_refresh_blocking(p).unwrap())),
    );
    let _ = update(&mut state, current);
    assert!(
        state
            .untracked_files
            .iter()
            .any(|change| change.path == "newer.txt")
    );
}

#[test]
fn commit_push_stops_when_branch_changed_before_upload() {
    let (_dir, p, _) = fixture("switched-branch");
    git(&p, &["checkout", "-b", "other"]);
    let error = run_push_blocking(p, Some("main".into())).unwrap_err();
    assert!(error.contains("current branch changed"), "{error}");
}

#[test]
fn cancelled_clone_cannot_finish_a_new_dialog_or_open_an_old_project() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let mut state = AppState::new();
    state.clone_dialog.open();
    let old = state.clone_dialog.generation;
    let cancellation = Arc::new(AtomicBool::new(false));
    state.clone_dialog.cancellation = Some(cancellation.clone());
    state.clone_dialog.is_cloning = true;
    state.clone_dialog.password = "temporary test credential".into();
    let _ = update(&mut state, Message::CloneMessage(CloneMessage::Cancel));
    assert!(cancellation.load(Ordering::Relaxed));
    assert!(!state.clone_dialog.is_cloning);
    assert!(state.clone_dialog.password.is_empty());
    state.clone_dialog.open();
    state.clone_dialog.is_cloning = true;
    for result in [Ok(PathBuf::from("/tmp/old-clone")), Err("old error".into())] {
        let task = update(&mut state, Message::CloneComplete(old, result));
        assert_eq!(task.units(), 0);
        assert!(state.clone_dialog.open);
        assert!(state.clone_dialog.is_cloning);
        assert!(state.clone_dialog.error.is_none());
    }
}

#[test]
fn network_menu_actions_dispatch_work_without_blocking_the_event_handler() {
    let (_dir, _p, repo) = fixture("network-dispatch");
    for message in [
        Message::RemoteDialogMessage(views::remote_dialog::RemoteDialogMessage::Fetch),
        Message::RemoteDialogMessage(views::remote_dialog::RemoteDialogMessage::ExecutePush),
        Message::RemoteDialogMessage(views::remote_dialog::RemoteDialogMessage::ExecutePull),
        Message::BranchPopupMessage(BranchPopupMessage::FetchRemote("origin".into())),
        Message::BranchPopupMessage(BranchPopupMessage::PushBranch {
            branch: "main".into(),
            remote: "origin".into(),
        }),
        Message::TagDialogMessage(views::tag_dialog::TagDialogMessage::DeleteRemoteTag(
            "tag".into(),
        )),
        Message::ToolbarRemoteActionSelected {
            action: ToolbarRemoteAction::Push,
            remote: "origin".into(),
        },
    ] {
        let mut state = AppState::new();
        state.set_repository(repo.clone(), &i18n::EN);
        let task = update(&mut state, message);
        assert!(task.units() > 0, "network work must be returned as a task");
        assert!(state.network_operation.is_some());
    }
}

#[test]
fn network_completion_is_scoped_and_resets_busy_state_after_failure() {
    let (_da, a, ra) = fixture("network-a");
    let (_db, _b, rb) = fixture("network-b");
    let mut state = AppState::new();
    state.set_repository(ra, &i18n::EN);
    let old = repository_session(&state);
    let _ = dispatch_network(
        &mut state,
        NetworkSurface::Remote,
        |_| Err("offline".into()),
    );
    let refresh = run_refresh_blocking(a).unwrap();
    let _ = update(
        &mut state,
        scope_repository_result(
            old.clone(),
            Message::NetworkCompleted(
                NetworkSurface::Remote,
                Ok((Err("offline".into()), Ok(refresh.clone()))),
            ),
        ),
    );
    assert!(!state.remote_dialog.is_loading);
    assert!(state.network_operation.is_none());
    assert_eq!(state.remote_dialog.error.as_deref(), Some("offline"));
    state.set_repository(rb, &i18n::EN);
    let _ = update(
        &mut state,
        scope_repository_result(
            old,
            Message::NetworkCompleted(
                NetworkSurface::Remote,
                Ok((Err("late failure".into()), Ok(refresh))),
            ),
        ),
    );
    assert!(state.remote_dialog.error.is_none());
    assert!(state.network_operation.is_none());
}

#[test]
fn switching_repository_cancels_clone_without_reusing_its_generation() {
    let (_dir, _p, repo) = fixture("clone-switch");
    let mut state = AppState::new();
    state.clone_dialog.open();
    let old = state.clone_dialog.generation;
    let signal = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    state.clone_dialog.cancellation = Some(signal.clone());
    state.set_repository(repo, &i18n::EN);
    state.clone_dialog.open();
    assert_ne!(old, state.clone_dialog.generation);
    assert!(signal.load(std::sync::atomic::Ordering::Relaxed));
    let _ = update(
        &mut state,
        Message::CloneComplete(old, Err("old clone failed".into())),
    );
    assert!(state.clone_dialog.error.is_none());
}

#[test]
fn merge_abort_button_restores_files_and_exits_conflict_view() {
    let (_dir, path, _) = fixture("merge-abort-button");
    git(&path, &["checkout", "-b", "incoming"]);
    fs::write(path.join("base.txt"), "incoming\n").unwrap();
    git(&path, &["commit", "-am", "incoming"]);
    git(&path, &["checkout", "main"]);
    fs::write(path.join("base.txt"), "local\n").unwrap();
    git(&path, &["commit", "-am", "local"]);
    let original = git(&path, &["rev-parse", "HEAD"]);
    fs::write(path.join("keep.txt"), "untracked\n").unwrap();
    let repo = Repository::open(&path).unwrap();
    assert!(repo.merge_branch("incoming").is_err());
    let mut state = AppState::new();
    state.set_repository(repo, &i18n::EN);
    state.open_conflict_resolver(&i18n::EN).unwrap();
    let repo = state.current_repository.clone().unwrap();
    sync_merge_commit_message_default(&mut state, &repo);
    assert!(!state.commit_dialog.message.is_empty());
    let _ = update(
        &mut state,
        Message::StateActionMessage(StateAction::AbortMerge),
    );
    assert_eq!(
        fs::read_to_string(path.join("base.txt")).unwrap(),
        "local\n"
    );
    assert_eq!(git(&path, &["rev-parse", "HEAD"]), original);
    assert_eq!(
        fs::read_to_string(path.join("keep.txt")).unwrap(),
        "untracked\n"
    );
    assert_eq!(git(&path, &["diff", "--name-only", "--diff-filter=U"]), "");
    assert!(!state.has_conflicts());
    assert!(state.conflict_resolver.is_none());
    assert!(state.commit_dialog.message.is_empty());
    assert_eq!(state.shell.active_section, ShellSection::Changes);
}
