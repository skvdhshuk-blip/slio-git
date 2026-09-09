//! Branch operations for git-core

#[cfg_attr(feature = "app-store", path = "branch/native.rs")]
#[cfg_attr(not(feature = "app-store"), path = "branch/desktop.rs")]
mod backend;

use crate::error::GitError;
use crate::repository::{Repository, SyncStatus, compact_branch_sync_hint, compact_relative_time};
use log::info;

/// Identifies what kind of ref was resolved during checkout_ref.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefKind {
    Branch,
    Tag,
    Commit,
}

/// Result returned by checkout_ref.
#[derive(Debug, Clone)]
pub struct CheckoutOutcome {
    pub ref_kind: RefKind,
    pub target_oid: String,
}

/// Checkout a branch name, tag name, or commit hash.
///
/// Security: input is trimmed and capped at 512 bytes. Dirty working tree is
/// refused before any HEAD modification.
pub fn checkout_ref(repo: &Repository, ref_str: &str) -> Result<CheckoutOutcome, GitError> {
    crate::native::reject_active(repo)?;
    let ref_str = ref_str.trim();

    if ref_str.is_empty() || ref_str.len() > 512 {
        return Err(GitError::InvalidInput {
            message: "ref must be 1–512 characters".to_string(),
        });
    }

    let repo_lock = repo.inner.read().unwrap();

    // Refuse dirty working tree before touching HEAD.
    let statuses = repo_lock
        .statuses(None)
        .map_err(|e| GitError::OperationFailed {
            operation: "checkout_ref".to_string(),
            details: e.to_string(),
        })?;
    let is_dirty = statuses.iter().any(|s| {
        let flags = s.status();
        flags != git2::Status::CURRENT && !flags.contains(git2::Status::IGNORED)
    });
    if is_dirty {
        return Err(GitError::DirtyWorkingTree);
    }

    let obj = repo_lock
        .revparse_single(ref_str)
        .map_err(|_| GitError::InvalidInput {
            message: format!("invalid ref: {ref_str}"),
        })?;

    let target_oid = obj.id().to_string();

    // Determine what the ref resolves to and checkout appropriately.
    let ref_kind = if let Ok(branch) = repo_lock.find_branch(ref_str, git2::BranchType::Local) {
        // It's a local branch — move HEAD symbolically.
        let refname = format!("refs/heads/{ref_str}");
        let commit = branch
            .get()
            .peel_to_commit()
            .map_err(|e| GitError::OperationFailed {
                operation: "checkout_ref".to_string(),
                details: e.to_string(),
            })?;
        repo_lock
            .checkout_tree(commit.as_object(), None)
            .map_err(|e| GitError::OperationFailed {
                operation: "checkout_ref".to_string(),
                details: e.to_string(),
            })?;
        repo_lock
            .set_head(&refname)
            .map_err(|e| GitError::OperationFailed {
                operation: "checkout_ref".to_string(),
                details: e.to_string(),
            })?;
        RefKind::Branch
    } else if let Ok(tag_obj) = repo_lock.find_reference(&format!("refs/tags/{ref_str}")) {
        // It's a tag — peel to commit, then detach HEAD.
        let commit = tag_obj
            .peel_to_commit()
            .map_err(|e| GitError::OperationFailed {
                operation: "checkout_ref".to_string(),
                details: e.to_string(),
            })?;
        repo_lock
            .checkout_tree(commit.as_object(), None)
            .map_err(|e| GitError::OperationFailed {
                operation: "checkout_ref".to_string(),
                details: e.to_string(),
            })?;
        repo_lock
            .set_head_detached(commit.id())
            .map_err(|e| GitError::OperationFailed {
                operation: "checkout_ref".to_string(),
                details: e.to_string(),
            })?;
        RefKind::Tag
    } else {
        // Treat as commit hash / other detachable ref.
        let commit = obj.peel_to_commit().map_err(|_| GitError::InvalidInput {
            message: format!("ref '{ref_str}' does not resolve to a commit"),
        })?;
        repo_lock
            .checkout_tree(commit.as_object(), None)
            .map_err(|e| GitError::OperationFailed {
                operation: "checkout_ref".to_string(),
                details: e.to_string(),
            })?;
        repo_lock
            .set_head_detached(commit.id())
            .map_err(|e| GitError::OperationFailed {
                operation: "checkout_ref".to_string(),
                details: e.to_string(),
            })?;
        RefKind::Commit
    };

    info!("checkout_ref '{}' → {:?} {}", ref_str, ref_kind, target_oid);
    Ok(CheckoutOutcome {
        ref_kind,
        target_oid,
    })
}

/// Lightweight branch reference returned by branches_containing_commit.
#[derive(Debug, Clone, PartialEq)]
pub struct BranchRef {
    pub name: String,
    pub is_remote: bool,
}

/// Return all local and remote branches that contain the given commit OID.
///
/// Equivalent to `git branch --all --contains <oid>`.
pub fn branches_containing_commit(
    repo: &Repository,
    commit_oid: &str,
) -> Result<Vec<BranchRef>, GitError> {
    info!("Listing branches containing commit '{}'", commit_oid);

    let oid = git2::Oid::from_str(commit_oid).map_err(|_| GitError::CommitNotFound {
        id: commit_oid.to_string(),
    })?;

    let repo_lock = repo.inner.read().unwrap();

    // Verify the commit exists
    repo_lock
        .find_commit(oid)
        .map_err(|_| GitError::CommitNotFound {
            id: commit_oid.to_string(),
        })?;

    let mut results = Vec::new();

    // Local branches
    let local_branches = repo_lock
        .branches(Some(git2::BranchType::Local))
        .map_err(|e| GitError::OperationFailed {
            operation: "branches_containing_commit".to_string(),
            details: e.to_string(),
        })?;

    for branch_result in local_branches {
        let (branch, _) = branch_result.map_err(|e| GitError::OperationFailed {
            operation: "branches_containing_commit".to_string(),
            details: e.to_string(),
        })?;

        let branch_oid = match branch.get().peel_to_commit() {
            Ok(c) => c.id(),
            Err(_) => continue,
        };

        let is_ancestor = repo_lock
            .graph_descendant_of(branch_oid, oid)
            .unwrap_or(false)
            || branch_oid == oid;

        if is_ancestor && let Some(name) = branch.name().ok().flatten() {
            results.push(BranchRef {
                name: name.to_string(),
                is_remote: false,
            });
        }
    }

    // Remote branches
    let remote_branches = repo_lock
        .branches(Some(git2::BranchType::Remote))
        .map_err(|e| GitError::OperationFailed {
            operation: "branches_containing_commit".to_string(),
            details: e.to_string(),
        })?;

    for branch_result in remote_branches {
        let (branch, _) = branch_result.map_err(|e| GitError::OperationFailed {
            operation: "branches_containing_commit".to_string(),
            details: e.to_string(),
        })?;

        let branch_oid = match branch.get().peel_to_commit() {
            Ok(c) => c.id(),
            Err(_) => continue,
        };

        let is_ancestor = repo_lock
            .graph_descendant_of(branch_oid, oid)
            .unwrap_or(false)
            || branch_oid == oid;

        if is_ancestor && let Some(name) = branch.name().ok().flatten() {
            results.push(BranchRef {
                name: name.to_string(),
                is_remote: true,
            });
        }
    }

    info!(
        "Found {} branches containing commit '{}'",
        results.len(),
        commit_oid
    );
    Ok(results)
}

/// A Git branch
#[derive(Debug, Clone)]
pub struct Branch {
    pub name: String,
    pub oid: String,
    pub is_remote: bool,
    pub is_head: bool,
    pub upstream: Option<String>,
    pub tracking_status: Option<String>,
    pub sync_hint: Option<String>,
    pub recency_hint: Option<String>,
    pub last_commit_timestamp: Option<i64>,
    /// Hierarchical group path for tree display (e.g., ["feature", "auth"] for "feature/auth")
    pub group_path: Option<Vec<String>>,
}

impl Branch {
    /// Compute the group_path from the branch name by splitting on '/'
    pub fn compute_group_path(&mut self) {
        let display_name = if self.is_remote {
            // For remote branches like "origin/feature/auth", skip the remote name
            self.name.split_once('/').map(|x| x.1).unwrap_or(&self.name)
        } else {
            &self.name
        };

        let parts: Vec<&str> = display_name.split('/').collect();
        if parts.len() > 1 {
            // All but the last part form the group path
            self.group_path = Some(
                parts[..parts.len() - 1]
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            );
        } else {
            self.group_path = None;
        }
    }

    /// Get the leaf name (last segment after '/')
    pub fn leaf_name(&self) -> &str {
        self.name.rsplit('/').next().unwrap_or(&self.name)
    }
}

impl Repository {
    /// Create a new branch
    /// Check if a local branch is fully merged into HEAD.
    pub fn is_branch_merged(&self, name: &str) -> Result<bool, GitError> {
        let repo_lock = self.inner.read().unwrap();
        let branch = repo_lock
            .find_branch(name, git2::BranchType::Local)
            .map_err(|_| GitError::BranchNotFound {
                name: name.to_string(),
            })?;
        let branch_oid = branch
            .get()
            .target()
            .ok_or_else(|| GitError::BranchNotFound {
                name: name.to_string(),
            })?;
        let head_oid = repo_lock
            .head()
            .ok()
            .and_then(|h| h.target())
            .ok_or_else(|| GitError::OperationFailed {
                operation: "is_branch_merged".to_string(),
                details: "No HEAD reference found".to_string(),
            })?;
        let merge_base =
            repo_lock
                .merge_base(branch_oid, head_oid)
                .map_err(|e| GitError::OperationFailed {
                    operation: "is_branch_merged".to_string(),
                    details: e.to_string(),
                })?;
        Ok(merge_base == branch_oid)
    }

    /// Find all local branches merged into the target branch.
    ///
    /// Uses `git branch --merged <target>` for a single batch query.
    /// Returns branch names only (no refs/heads/ prefix).
    pub fn find_merged_branches(&self, target: &str) -> Result<Vec<String>, GitError> {
        backend::find_merged_branches(self, target)
    }

    pub fn create_branch(&self, name: &str, oid: &str) -> Result<Branch, GitError> {
        self.create_branch_from_start_point(name, oid)
    }

    /// Create a new branch from a commit, ref, or other git revspec.
    pub fn create_branch_from_start_point(
        &self,
        name: &str,
        start_point: &str,
    ) -> Result<Branch, GitError> {
        crate::native::reject_active(self)?;
        backend::create_branch_from_start_point(self, name, start_point)
    }

    /// Delete a branch
    pub fn delete_branch(&self, name: &str) -> Result<(), GitError> {
        crate::native::reject_active(self)?;
        backend::delete_branch(self, name, false)
    }

    /// Delete an unmerged local branch after explicit confirmation.
    pub fn force_delete_branch(&self, name: &str) -> Result<(), GitError> {
        crate::native::reject_active(self)?;
        backend::delete_branch(self, name, true)
    }

    /// Rename a branch
    pub fn rename_branch(&self, old_name: &str, new_name: &str) -> Result<Branch, GitError> {
        crate::native::reject_active(self)?;
        backend::rename_branch(self, old_name, new_name)
    }

    /// Configure the local branch to track an upstream branch.
    pub fn set_branch_upstream(&self, branch_name: &str, upstream: &str) -> Result<(), GitError> {
        crate::native::reject_active(self)?;
        backend::set_branch_upstream(self, branch_name, upstream)
    }

    /// List uncommitted changed file paths (staged + unstaged).
    pub fn list_uncommitted_files(&self) -> Vec<String> {
        backend::list_uncommitted_files(self)
    }

    /// Check if the working tree has uncommitted changes (staged or unstaged).
    pub fn has_uncommitted_changes(&self) -> bool {
        backend::has_uncommitted_changes(self)
    }

    /// Check if a checkout error is due to uncommitted changes that would be overwritten.
    pub fn is_checkout_conflict_error(error: &GitError) -> bool {
        let msg = error.to_string().to_lowercase();
        msg.contains("would be overwritten")
            || msg.contains("please commit your changes or stash them")
            || msg.contains("local changes")
    }

    /// Force checkout — discards all local changes.
    pub fn force_checkout_branch(&self, name: &str) -> Result<(), GitError> {
        crate::native::reject_active(self)?;
        backend::force_checkout_branch(self, name)
    }

    /// Smart checkout — stash changes, checkout, then pop stash.
    pub fn smart_checkout_branch(&self, name: &str) -> Result<(), GitError> {
        crate::native::reject_active(self)?;
        backend::smart_checkout_branch(self, name)
    }

    /// Checkout a branch
    pub fn checkout_branch(&self, name: &str) -> Result<(), GitError> {
        crate::native::reject_active(self)?;
        backend::checkout_branch(self, name)
    }

    /// Checkout a remote branch by creating or reusing a local tracking branch.
    pub fn checkout_remote_branch(&self, remote_ref: &str) -> Result<String, GitError> {
        crate::native::reject_active(self)?;
        backend::checkout_remote_branch(self, remote_ref)
    }

    /// Merge a branch into current branch
    pub fn merge_branch(&self, branch_name: &str) -> Result<(), GitError> {
        crate::native::reject_active(self)?;
        backend::merge_branch(self, branch_name)
    }

    /// Get list of all branches
    pub fn list_branches(&self) -> Result<Vec<Branch>, GitError> {
        backend::list_branches(self)
    }
}
