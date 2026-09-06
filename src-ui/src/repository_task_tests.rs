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
