//! Unit tests for branch::checkout_ref (T4b).

mod test_helpers;

use git_core::{GitError, RefKind, Repository, checkout_ref};
use std::process::Command;
use test_helpers::TestRepo;

fn run_git(path: &std::path::Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .expect("run git");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn create_branch(path: &std::path::Path, name: &str) {
    Command::new("git")
        .args(["branch", name])
        .current_dir(path)
        .output()
        .expect("git branch");
}

fn create_tag(path: &std::path::Path, name: &str) {
    Command::new("git")
        .args(["tag", name])
        .current_dir(path)
        .output()
        .expect("git tag");
}

fn head_oid(path: &std::path::Path) -> String {
    run_git(path, &["rev-parse", "HEAD"])
}

fn is_detached(path: &std::path::Path) -> bool {
    let out = Command::new("git")
        .args(["symbolic-ref", "--quiet", "HEAD"])
        .current_dir(path)
        .output()
        .expect("symbolic-ref");
    !out.status.success()
}

#[test]
fn checkout_ref_branch_success() {
    let tr = TestRepo::new().unwrap();
    let p = tr.path();

    tr.add_and_commit("a.txt", "first\n", "init").unwrap();
    create_branch(p, "feature");
    tr.add_and_commit("a.txt", "second\n", "second commit")
        .unwrap();

    let repo = Repository::discover(p).unwrap();
    let outcome = checkout_ref(&repo, "feature").unwrap();

    assert_eq!(outcome.ref_kind, RefKind::Branch);
    assert!(!is_detached(p), "branch checkout should keep symbolic HEAD");
    assert_eq!(
        run_git(p, &["rev-parse", "--abbrev-ref", "HEAD"]),
        "feature"
    );
    assert!(!outcome.target_oid.is_empty());
}

#[test]
fn checkout_ref_tag_detaches() {
    let tr = TestRepo::new().unwrap();
    let p = tr.path();

    tr.add_and_commit("a.txt", "v1\n", "init").unwrap();
    create_tag(p, "v1.0");
    tr.add_and_commit("a.txt", "v2\n", "second").unwrap();

    let repo = Repository::discover(p).unwrap();
    let outcome = checkout_ref(&repo, "v1.0").unwrap();

    assert_eq!(outcome.ref_kind, RefKind::Tag);
    assert!(is_detached(p), "tag checkout should detach HEAD");
}

#[test]
fn checkout_ref_commit_detaches() {
    let tr = TestRepo::new().unwrap();
    let p = tr.path();

    tr.add_and_commit("a.txt", "first\n", "init").unwrap();
    let first_oid = head_oid(p);
    tr.add_and_commit("a.txt", "second\n", "second").unwrap();

    let repo = Repository::discover(p).unwrap();
    let outcome = checkout_ref(&repo, &first_oid).unwrap();

    assert_eq!(outcome.ref_kind, RefKind::Commit);
    assert!(is_detached(p), "commit checkout should detach HEAD");
    assert_eq!(&outcome.target_oid, &first_oid);
}

#[test]
fn checkout_ref_invalid_errors() {
    let tr = TestRepo::new().unwrap();
    let p = tr.path();
    tr.add_and_commit("a.txt", "x\n", "init").unwrap();

    let repo = Repository::discover(p).unwrap();

    // Empty string
    let err = checkout_ref(&repo, "").unwrap_err();
    assert!(
        matches!(err, GitError::InvalidInput { .. }),
        "empty should be InvalidInput"
    );

    // Too long (513 chars)
    let long = "a".repeat(513);
    let err = checkout_ref(&repo, &long).unwrap_err();
    assert!(
        matches!(err, GitError::InvalidInput { .. }),
        "overlong should be InvalidInput"
    );

    // Nonexistent ref
    let err = checkout_ref(&repo, "nonexistent-ref-xyz").unwrap_err();
    assert!(
        matches!(err, GitError::InvalidInput { .. }),
        "unknown ref should be InvalidInput"
    );
}

#[test]
fn checkout_ref_dirty_workspace_refuses() {
    let tr = TestRepo::new().unwrap();
    let p = tr.path();

    tr.add_and_commit("a.txt", "clean\n", "init").unwrap();
    create_branch(p, "feature");

    // Dirty the working tree
    tr.write_file("a.txt", "dirty\n").unwrap();

    let repo = Repository::discover(p).unwrap();
    let err = checkout_ref(&repo, "feature").unwrap_err();

    assert!(
        matches!(err, GitError::DirtyWorkingTree),
        "dirty workspace should be refused, got: {err:?}"
    );
}
