//! M1-A PR1 integration tests: history refs backfill and branches_containing_commit.

mod test_helpers;

use git_core::graph::{RefType, compute_ref_labels};
use git_core::{Repository, branches_containing_commit, get_history};
use test_helpers::TestRepo;

fn git(args: &[&str], cwd: &std::path::Path) {
    let status = std::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .status()
        .expect("git command failed");
    assert!(status.success(), "git {:?} failed", args);
}

fn git_sha(cwd: &std::path::Path) -> String {
    let out = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(cwd)
        .output()
        .expect("git rev-parse failed");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

// ── refs backfill: local branch label and is_current flag ────────────────────

#[test]
fn history_entry_has_local_branch_ref_and_head_flag() {
    let repo = TestRepo::new().unwrap();
    repo.add_and_commit("a.txt", "v1", "initial commit")
        .unwrap();

    let r = Repository::discover(repo.path()).unwrap();
    let history = get_history(&r, Some(10)).unwrap();

    assert!(!history.is_empty());
    let head = &history[0];
    let local: Vec<_> = head
        .refs
        .iter()
        .filter(|r| r.ref_type == RefType::LocalBranch)
        .collect();
    assert!(
        !local.is_empty(),
        "HEAD commit must have local branch ref, got: {:?}",
        head.refs
    );
    assert!(
        head.refs.iter().any(|r| r.is_current),
        "HEAD commit must have is_current=true"
    );

    // older commit (if any second commit added) must not have is_current
    repo.add_and_commit("a.txt", "v2", "second commit").unwrap();
    let r2 = Repository::discover(repo.path()).unwrap();
    let h2 = get_history(&r2, Some(10)).unwrap();
    assert!(h2.len() >= 2);
    assert!(
        !h2[1].refs.iter().any(|r| r.is_current),
        "older commit must not have is_current"
    );
}

// ── refs backfill: tag ref ────────────────────────────────────────────────────

#[test]
fn history_entry_tagged_commit_has_tag_ref() {
    let repo = TestRepo::new().unwrap();
    repo.add_and_commit("a.txt", "v1", "initial commit")
        .unwrap();
    git(&["tag", "v0.1.0"], repo.path());

    let r = Repository::discover(repo.path()).unwrap();
    let history = get_history(&r, Some(10)).unwrap();

    let tag_refs: Vec<_> = history[0]
        .refs
        .iter()
        .filter(|r| r.ref_type == RefType::Tag)
        .collect();
    assert!(
        !tag_refs.is_empty(),
        "tagged commit must have Tag ref, refs: {:?}",
        history[0].refs
    );
    assert_eq!(tag_refs[0].name, "v0.1.0");
}

// ── refs backfill: detached HEAD gets Head ref type ──────────────────────────

#[test]
fn detached_head_commit_has_head_ref_type() {
    let repo = TestRepo::new().unwrap();
    repo.add_and_commit("a.txt", "v1", "first").unwrap();
    let sha = git_sha(repo.path());
    repo.add_and_commit("a.txt", "v2", "second").unwrap();
    git(&["checkout", "--detach", &sha], repo.path());

    let r = Repository::discover(repo.path()).unwrap();
    let ref_map = compute_ref_labels(&r).unwrap();
    let has_head = ref_map
        .get(&sha)
        .map(|labels| labels.iter().any(|l| l.ref_type == RefType::Head))
        .unwrap_or(false);
    assert!(has_head, "detached HEAD commit must have Head ref type");
}

// ── branches_containing_commit: basic ────────────────────────────────────────

#[test]
fn branches_containing_head_commit() {
    let repo = TestRepo::new().unwrap();
    repo.add_and_commit("a.txt", "v1", "initial").unwrap();
    let sha = git_sha(repo.path());
    git(&["branch", "feature-x"], repo.path());

    let r = Repository::discover(repo.path()).unwrap();
    let branches = branches_containing_commit(&r, &sha).unwrap();
    let names: Vec<&str> = branches.iter().map(|b| b.name.as_str()).collect();

    assert!(
        branches.len() >= 2,
        "both default and feature-x must contain commit, got: {:?}",
        names
    );
    assert!(
        names.contains(&"feature-x"),
        "feature-x must be in results: {:?}",
        names
    );
}

// ── branches_containing_commit: diverged branch excluded ─────────────────────

#[test]
fn diverged_branch_excluded_from_containing_branches() {
    let repo = TestRepo::new().unwrap();
    repo.add_and_commit("a.txt", "v1", "first").unwrap();
    git(&["branch", "feature"], repo.path());
    repo.add_and_commit("a.txt", "v2", "second").unwrap();
    let sha = git_sha(repo.path());

    let r = Repository::discover(repo.path()).unwrap();
    let branches = branches_containing_commit(&r, &sha).unwrap();
    let names: Vec<&str> = branches.iter().map(|b| b.name.as_str()).collect();
    assert!(
        !names.contains(&"feature"),
        "feature must NOT contain second commit, got: {:?}",
        names
    );
}

// ── branches_containing_commit: invalid OID returns error ────────────────────

#[test]
fn invalid_oid_returns_error() {
    let repo = TestRepo::new().unwrap();
    repo.add_and_commit("a.txt", "v1", "initial").unwrap();

    let r = Repository::discover(repo.path()).unwrap();
    assert!(branches_containing_commit(&r, "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef").is_err());
}
