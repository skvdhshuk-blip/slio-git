//! Integration tests for signature verification and SignatureCache.

use git_core::history::get_history;
use git_core::repository::Repository;
use git_core::signature::{SignatureCache, SignatureStatus, VerificationFailureReason};
use std::path::Path;
use tempfile::TempDir;

fn make_commit(repo_path: &Path, message: &str) {
    let status = std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", message])
        .current_dir(repo_path)
        .env("GIT_AUTHOR_NAME", "Test User")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test User")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .status()
        .expect("git commit failed");
    assert!(status.success(), "git commit exited with failure");
}

fn setup_repo() -> (Repository, TempDir) {
    let dir = TempDir::new().unwrap();
    let path = dir.path();

    let init = std::process::Command::new("git")
        .args(["init"])
        .current_dir(path)
        .status()
        .unwrap();
    assert!(init.success());

    std::process::Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(path)
        .status()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(path)
        .status()
        .unwrap();

    make_commit(path, "first commit");
    make_commit(path, "second commit");
    make_commit(path, "third commit");

    let repo = Repository::open(path).unwrap();
    (repo, dir)
}

#[test]
fn unsigned_commits_return_no_signature_via_cache_miss() {
    let (repo, _dir) = setup_repo();
    let cache = repo.signature_cache();

    // Fresh repo — cache is empty; all entries should have None signature_status
    let history = get_history(&repo, Some(3)).unwrap();
    assert_eq!(history.len(), 3, "expected 3 history entries");

    for entry in &history {
        assert!(
            entry.signature_status.is_none(),
            "unsigned commit should have None signature_status (cache miss), got {:?}",
            entry.signature_status
        );
    }

    // Manually pre-populate cache with NoSignature for all 3 commits
    for entry in &history {
        let oid = git2::Oid::from_str(&entry.id).unwrap();
        cache.insert(oid, SignatureStatus::NoSignature);
    }

    // Second pass: cache should return NoSignature for all
    let history2 = get_history(&repo, Some(3)).unwrap();
    for entry in &history2 {
        assert_eq!(
            entry.signature_status,
            Some(SignatureStatus::NoSignature),
            "expected NoSignature from cache, got {:?}",
            entry.signature_status
        );
    }
}

#[test]
fn cache_hit_rate_second_pass_all_hit() {
    let (repo, _dir) = setup_repo();
    let cache = repo.signature_cache();

    let history = get_history(&repo, Some(3)).unwrap();
    assert_eq!(history.len(), 3);

    // Pre-populate cache simulating backfill
    for entry in &history {
        let oid = git2::Oid::from_str(&entry.id).unwrap();
        cache.insert(oid, SignatureStatus::NoSignature);
    }

    // Second pass: all 3 should be cache hits
    let history2 = get_history(&repo, Some(3)).unwrap();
    let hit_count = history2
        .iter()
        .filter(|e| e.signature_status.is_some())
        .count();
    assert_eq!(hit_count, 3, "expected 3 cache hits on second pass");
}

#[test]
fn cache_stores_verified_and_bad_states() {
    let (repo, _dir) = setup_repo();
    let cache = repo.signature_cache();

    let history = get_history(&repo, Some(3)).unwrap();
    assert_eq!(history.len(), 3);

    let oids: Vec<git2::Oid> = history
        .iter()
        .map(|e| git2::Oid::from_str(&e.id).unwrap())
        .collect();

    // Simulate: first = Verified, second = NotVerified(MissingPublicKey), third = Bad
    cache.insert(
        oids[0],
        SignatureStatus::Verified {
            user: "Alice".to_string(),
            fingerprint: "AABBCCDD".to_string(),
        },
    );
    cache.insert(
        oids[1],
        SignatureStatus::NotVerified {
            reason: VerificationFailureReason::Unknown,
        },
    );
    cache.insert(oids[2], SignatureStatus::Bad);

    let history2 = get_history(&repo, Some(3)).unwrap();

    assert_eq!(
        history2[0].signature_status,
        Some(SignatureStatus::Verified {
            user: "Alice".to_string(),
            fingerprint: "AABBCCDD".to_string(),
        })
    );
    assert_eq!(
        history2[1].signature_status,
        Some(SignatureStatus::NotVerified {
            reason: VerificationFailureReason::Unknown,
        })
    );
    assert_eq!(history2[2].signature_status, Some(SignatureStatus::Bad));
}

#[test]
fn signature_cache_clear_invalidates_all_entries() {
    let (repo, _dir) = setup_repo();
    let cache = repo.signature_cache();

    let history = get_history(&repo, Some(3)).unwrap();
    for entry in &history {
        let oid = git2::Oid::from_str(&entry.id).unwrap();
        cache.insert(oid, SignatureStatus::NoSignature);
    }

    cache.clear();

    // After clear, all lookups should miss
    let history2 = get_history(&repo, Some(3)).unwrap();
    for entry in &history2 {
        assert!(
            entry.signature_status.is_none(),
            "after clear, signature_status must be None"
        );
    }
}
