//! Reflog-based recent checkout history (mirrors IDEA GitRecentCheckoutBranches).

use crate::{GitError, Repository};
use std::collections::HashSet;

/// Returns recently checked-out branch names in reverse-checkout order, deduped.
///
/// Reads HEAD reflog for "checkout: moving from <X> to <Y>" entries,
/// collects destination branch names most-recent first, skipping duplicates.
pub fn recent_checkout_branches(repo: &Repository, limit: usize) -> Result<Vec<String>, GitError> {
    let inner = repo.inner.read().map_err(|_| GitError::OperationFailed {
        operation: "reflog".to_string(),
        details: "lock poisoned".to_string(),
    })?;
    let reflog = match inner.reflog("HEAD") {
        Ok(r) => r,
        Err(_) => return Ok(Vec::new()),
    };

    let mut seen = HashSet::new();
    let mut result = Vec::new();

    // git2 reflog index 0 = most recent entry; iterate forward for most-recent-first order
    for entry in reflog.iter() {
        if result.len() >= limit {
            break;
        }
        let message = match entry.message() {
            Some(m) => m,
            None => continue,
        };
        let Some(rest) = message.strip_prefix("checkout: moving from ") else {
            continue;
        };
        let Some(to_idx) = rest.find(" to ") else {
            continue;
        };
        let to_branch = rest[to_idx + 4..].trim();
        if to_branch.is_empty() {
            continue;
        }
        if seen.insert(to_branch.to_string()) {
            result.push(to_branch.to_string());
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Repository;
    use tempfile::TempDir;

    fn make_commit(inner: &git2::Repository) {
        let sig = git2::Signature::now("test", "test@example.com").unwrap();
        let tree_id = inner.index().unwrap().write_tree().unwrap();
        let tree = inner.find_tree(tree_id).unwrap();
        let head_commit = inner.head().ok().and_then(|h| h.peel_to_commit().ok());
        if let Some(ref parent) = head_commit {
            inner
                .commit(Some("HEAD"), &sig, &sig, "commit", &tree, &[parent])
                .unwrap();
        } else {
            inner
                .commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
                .unwrap();
        }
    }

    fn create_branch_and_checkout(inner: &git2::Repository, name: &str) {
        let head = inner.head().unwrap().peel_to_commit().unwrap();
        let branch = inner.branch(name, &head, false).unwrap();
        inner.set_head(branch.get().name().unwrap()).unwrap();
        let obj = inner.revparse_single("HEAD").unwrap();
        inner.checkout_tree(&obj, None).unwrap();
    }

    #[test]
    fn recent_checkout_branches_returns_deduped_reverse_order() {
        let tmp = TempDir::new().unwrap();
        let inner_repo = git2::Repository::init(tmp.path()).unwrap();
        make_commit(&inner_repo);

        create_branch_and_checkout(&inner_repo, "branch-a");
        create_branch_and_checkout(&inner_repo, "branch-b");
        create_branch_and_checkout(&inner_repo, "branch-c");
        // Check out branch-a again to trigger dedup
        inner_repo.set_head("refs/heads/branch-a").unwrap();
        let obj = inner_repo.revparse_single("HEAD").unwrap();
        inner_repo.checkout_tree(&obj, None).unwrap();

        let repo = Repository::open(tmp.path()).unwrap();
        let result = recent_checkout_branches(&repo, 10).unwrap();

        // branch-a appears only once (deduped)
        assert_eq!(result.iter().filter(|s| *s == "branch-a").count(), 1);

        let a_pos = result.iter().position(|s| s == "branch-a").unwrap();
        let c_pos = result.iter().position(|s| s == "branch-c").unwrap();
        let b_pos = result.iter().position(|s| s == "branch-b").unwrap();
        // Most-recent first: A, then C, then B
        assert!(a_pos < c_pos, "branch-a should precede branch-c");
        assert!(c_pos < b_pos, "branch-c should precede branch-b");
    }

    #[test]
    fn recent_checkout_branches_empty_reflog_returns_empty() {
        let tmp = TempDir::new().unwrap();
        // A freshly init'd repo with no checkouts has no checkout reflog entries.
        let _inner_repo = git2::Repository::init(tmp.path()).unwrap();
        let repo = Repository::open(tmp.path()).unwrap();
        let result = recent_checkout_branches(&repo, 10).unwrap();
        assert!(result.is_empty());
    }
}
