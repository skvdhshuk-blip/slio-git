//! Branch operations for git-core

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
        info!("Finding branches merged into '{}'", target);

        let repo_lock = self.inner.read().unwrap();
        let target_oid = repo_lock
            .revparse_single(target)
            .and_then(|obj| obj.peel_to_commit())
            .map(|commit| commit.id())
            .map_err(|e| GitError::OperationFailed {
                operation: "find_merged_branches".to_string(),
                details: e.to_string(),
            })?;
        let iter = repo_lock
            .branches(Some(git2::BranchType::Local))
            .map_err(|e| GitError::OperationFailed {
                operation: "find_merged_branches".to_string(),
                details: e.to_string(),
            })?;
        let mut branches = Vec::new();
        for entry in iter {
            let (branch, _) = entry.map_err(|e| GitError::OperationFailed {
                operation: "find_merged_branches".to_string(),
                details: e.to_string(),
            })?;
            let Some(name) = branch.name().ok().flatten().map(str::to_string) else {
                continue;
            };
            let Some(oid) = branch.get().target() else {
                continue;
            };
            if repo_lock.merge_base(oid, target_oid).ok() == Some(oid) {
                branches.push(name);
            }
        }

        info!(
            "Found {} branches merged into '{}'",
            branches.len(),
            target
        );
        Ok(branches)
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
        info!("Creating branch '{}' from '{}'", name, start_point);

        let repo_lock = self.inner.write().unwrap();
        let commit = repo_lock
            .revparse_single(start_point)
            .and_then(|obj| obj.peel_to_commit())
            .map_err(|e| GitError::OperationFailed {
                operation: "create_branch".to_string(),
                details: e.to_string(),
            })?;
        let oid = commit.id().to_string();
        repo_lock
            .branch(name, &commit, false)
            .map_err(|e| GitError::OperationFailed {
                operation: "create_branch".to_string(),
                details: e.to_string(),
            })?;

        Ok(Branch {
            name: name.to_string(),
            oid,
            is_remote: false,
            is_head: false,
            upstream: None,
            tracking_status: None,
            sync_hint: None,
            recency_hint: None,
            last_commit_timestamp: None,
            group_path: None,
        })
    }

    /// Delete a fully merged local branch. Unmerged branches require
    /// [`Self::force_delete_branch`] after the user confirms the warning.
    pub fn delete_branch(&self, name: &str) -> Result<(), GitError> {
        self.delete_local_branch(name, false)
    }

    /// Delete a local branch even when it is not fully merged.
    pub fn force_delete_branch(&self, name: &str) -> Result<(), GitError> {
        self.delete_local_branch(name, true)
    }

    fn delete_local_branch(&self, name: &str, force: bool) -> Result<(), GitError> {
        info!("Deleting branch '{name}' (force={force})");
        if !force && !self.is_branch_merged(name).unwrap_or(false) {
            return Err(GitError::OperationFailed {
                operation: "delete_branch".to_string(),
                details: format!("branch '{name}' is not fully merged"),
            });
        }
        let repo_lock = self.inner.write().unwrap();
        let mut branch = repo_lock
            .find_branch(name, git2::BranchType::Local)
            .map_err(|_| GitError::BranchNotFound {
                name: name.to_string(),
            })?;
        if branch.is_head() {
            return Err(GitError::OperationFailed {
                operation: "delete_branch".to_string(),
                details: format!("cannot delete the checked-out branch '{name}'"),
            });
        }
        branch.delete().map_err(|e| GitError::OperationFailed {
            operation: "delete_branch".to_string(),
            details: e.to_string(),
        })
    }

    /// Rename a branch
    pub fn rename_branch(&self, old_name: &str, new_name: &str) -> Result<Branch, GitError> {
        info!("Renaming branch '{}' to '{}'", old_name, new_name);
        let repo_lock = self.inner.write().unwrap();
        let mut branch = repo_lock
            .find_branch(old_name, git2::BranchType::Local)
            .map_err(|_| GitError::BranchNotFound {
                name: old_name.to_string(),
            })?;
        branch
            .rename(new_name, false)
            .map_err(|e| GitError::OperationFailed {
                operation: "rename_branch".to_string(),
                details: e.to_string(),
            })?;

        Ok(Branch {
            name: new_name.to_string(),
            oid: String::new(),
            is_remote: false,
            is_head: false,
            upstream: None,
            tracking_status: None,
            sync_hint: None,
            recency_hint: None,
            last_commit_timestamp: None,
            group_path: None,
        })
    }

    /// Configure the local branch to track an upstream branch.
    pub fn set_branch_upstream(&self, branch_name: &str, upstream: &str) -> Result<(), GitError> {
        info!(
            "Setting upstream of branch '{}' to '{}'",
            branch_name, upstream
        );
        let repo_lock = self.inner.write().unwrap();
        let mut branch = repo_lock
            .find_branch(branch_name, git2::BranchType::Local)
            .map_err(|_| GitError::BranchNotFound {
                name: branch_name.to_string(),
            })?;
        branch
            .set_upstream(Some(upstream))
            .map_err(|e| GitError::OperationFailed {
                operation: "set_branch_upstream".to_string(),
                details: e.to_string(),
            })
    }

    /// List uncommitted changed file paths (staged + unstaged).
    pub fn list_uncommitted_files(&self) -> Vec<String> {
        let Ok(repo_lock) = self.inner.read() else {
            return Vec::new();
        };
        let mut opts = git2::StatusOptions::new();
        opts.include_untracked(true);
        let Ok(statuses) = repo_lock.statuses(Some(&mut opts)) else {
            return Vec::new();
        };
        statuses
            .iter()
            .filter(|entry| {
                let flags = entry.status();
                flags != git2::Status::CURRENT && !flags.contains(git2::Status::IGNORED)
            })
            .filter_map(|entry| entry.path().map(str::to_string))
            .collect()
    }

    /// Check if the working tree has uncommitted changes (staged or unstaged).
    pub fn has_uncommitted_changes(&self) -> bool {
        !self.list_uncommitted_files().is_empty()
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
        info!("Force checking out branch '{}' (discarding changes)", name);
        checkout_local_branch(self, name, true)
    }

    /// Smart checkout — stash changes, checkout, then pop stash.
    pub fn smart_checkout_branch(&self, name: &str) -> Result<(), GitError> {
        info!(
            "Smart checking out branch '{}' (stash → checkout → unstash)",
            name
        );
        let stash_created = crate::stash::stash_save_with_options(
            self,
            Some("slio-git: smart checkout auto-stash"),
            true,
            false,
        )
        .is_ok();
        let checkout_result = self.checkout_branch(name);
        if stash_created {
            if let Err(error) = crate::stash::stash_pop(self, 0) {
                info!("Warning: stash pop failed after smart checkout: {error}");
            }
        }
        checkout_result
    }

    /// Checkout a branch
    pub fn checkout_branch(&self, name: &str) -> Result<(), GitError> {
        info!("Checking out branch '{}'", name);
        checkout_local_branch(self, name, false)
    }

    /// Checkout a remote branch by creating or reusing a local tracking branch.
    pub fn checkout_remote_branch(&self, remote_ref: &str) -> Result<String, GitError> {
        info!("Checking out remote branch '{}'", remote_ref);

        let Some((_, local_branch_name)) = remote_ref.split_once('/') else {
            return Err(GitError::OperationFailed {
                operation: "checkout_remote_branch".to_string(),
                details: format!("invalid remote branch ref: {remote_ref}"),
            });
        };

        let local_exists = {
            let repo_lock = self.inner.read().unwrap();
            repo_lock
                .find_branch(local_branch_name, git2::BranchType::Local)
                .is_ok()
        };
        if !local_exists {
            self.create_branch_from_start_point(local_branch_name, remote_ref)?;
            self.set_branch_upstream(local_branch_name, remote_ref)?;
        }
        self.checkout_branch(local_branch_name)?;
        Ok(local_branch_name.to_string())
    }

    /// Merge a branch into current branch
    pub fn merge_branch(&self, branch_name: &str) -> Result<(), GitError> {
        info!("Merging branch '{}' into current branch", branch_name);
        merge_named_ref(self, branch_name, "merge_branch")
    }

    /// Get list of all branches
    pub fn list_branches(&self) -> Result<Vec<Branch>, GitError> {
        info!("Listing all branches");

        let repo_lock = self.inner.read().unwrap();
        let current_branch = self.current_branch().ok().flatten().unwrap_or_default();
        let iter = repo_lock
            .branches(None)
            .map_err(|e| GitError::OperationFailed {
                operation: "list_branches".to_string(),
                details: e.to_string(),
            })?;
        let mut branches = Vec::new();
        for entry in iter {
            let (branch, branch_type) = entry.map_err(|e| GitError::OperationFailed {
                operation: "list_branches".to_string(),
                details: e.to_string(),
            })?;
            let Some(name) = branch.name().ok().flatten().map(str::to_string) else {
                continue;
            };
            if name.ends_with("/HEAD") {
                continue;
            }
            let is_remote = branch_type == git2::BranchType::Remote;
            let commit = branch.get().peel_to_commit().ok();
            let oid = commit
                .as_ref()
                .map(|commit| commit.id().to_string())
                .unwrap_or_default();
            let last_commit_timestamp = commit.as_ref().map(|commit| commit.time().seconds());
            let is_head = branch.is_head() || (!is_remote && name == current_branch);
            let upstream = branch.upstream().ok().and_then(|upstream| {
                upstream.name().ok().flatten().map(|name| {
                    name.strip_prefix("refs/remotes/")
                        .unwrap_or(name)
                        .to_string()
                })
            });
            let tracking_status = if is_head {
                current_branch_tracking(self)
            } else {
                None
            };
            let sync_hint =
                compact_branch_sync_hint(upstream.as_deref(), tracking_status.as_deref());
            let mut item = Branch {
                name,
                oid,
                is_remote,
                is_head,
                upstream,
                tracking_status,
                sync_hint,
                recency_hint: compact_relative_time(last_commit_timestamp),
                last_commit_timestamp,
                group_path: None,
            };
            item.compute_group_path();
            branches.push(item);
        }
        branches.sort_by(|a, b| b.last_commit_timestamp.cmp(&a.last_commit_timestamp));
        Ok(branches)
    }
}

fn checkout_local_branch(repo: &Repository, name: &str, force: bool) -> Result<(), GitError> {
    let repo_lock = repo.inner.write().unwrap();
    let branch = repo_lock
        .find_branch(name, git2::BranchType::Local)
        .map_err(|_| GitError::BranchNotFound {
            name: name.to_string(),
        })?;
    let commit = branch
        .get()
        .peel_to_commit()
        .map_err(|e| GitError::OperationFailed {
            operation: "checkout_branch".to_string(),
            details: e.to_string(),
        })?;
    let mut opts = git2::build::CheckoutBuilder::new();
    if force {
        opts.force();
    }
    repo_lock
        .checkout_tree(commit.as_object(), Some(&mut opts))
        .map_err(|e| GitError::OperationFailed {
            operation: "checkout_branch".to_string(),
            details: e.to_string(),
        })?;
    repo_lock
        .set_head(&format!("refs/heads/{name}"))
        .map_err(|e| GitError::OperationFailed {
            operation: "checkout_branch".to_string(),
            details: e.to_string(),
        })?;
    Ok(())
}

fn merge_named_ref(repo: &Repository, name: &str, operation: &str) -> Result<(), GitError> {
    let signature = crate::commit::get_default_signature(repo)?;
    let repo_lock = repo.inner.write().unwrap();
    let oid = repo_lock
        .revparse_single(name)
        .and_then(|obj| obj.peel_to_commit())
        .map(|commit| commit.id())
        .map_err(|e| GitError::OperationFailed {
            operation: operation.to_string(),
            details: e.to_string(),
        })?;
    let annotated = repo_lock
        .find_annotated_commit(oid)
        .map_err(|e| GitError::OperationFailed {
            operation: operation.to_string(),
            details: e.to_string(),
        })?;
    finish_libgit2_merge(&repo_lock, &annotated, &signature, name, operation)
}

pub(crate) fn finish_libgit2_merge(
    repo: &git2::Repository,
    annotated: &git2::AnnotatedCommit<'_>,
    signature: &git2::Signature<'_>,
    name: &str,
    operation: &str,
) -> Result<(), GitError> {
    let (analysis, _) = repo
        .merge_analysis(&[annotated])
        .map_err(|e| GitError::OperationFailed {
            operation: operation.to_string(),
            details: e.to_string(),
        })?;
    if analysis.is_up_to_date() {
        return Ok(());
    }
    if analysis.is_fast_forward() {
        let mut reference = repo
            .head()
            .map_err(|e| GitError::OperationFailed {
                operation: operation.to_string(),
                details: e.to_string(),
            })?;
        reference
            .set_target(annotated.id(), "slio-git fast-forward merge")
            .map_err(|e| GitError::OperationFailed {
                operation: operation.to_string(),
                details: e.to_string(),
            })?;
        repo.set_head(reference.name().unwrap_or("HEAD"))
            .map_err(|e| GitError::OperationFailed {
                operation: operation.to_string(),
                details: e.to_string(),
            })?;
        repo.checkout_head(Some(git2::build::CheckoutBuilder::default().force()))
            .map_err(|e| GitError::OperationFailed {
                operation: operation.to_string(),
                details: e.to_string(),
            })?;
        return Ok(());
    }

    repo.merge(&[annotated], None, None)
        .map_err(|e| GitError::OperationFailed {
            operation: operation.to_string(),
            details: e.to_string(),
        })?;
    if repo.index().map(|index| index.has_conflicts()).unwrap_or(false) {
        return Err(GitError::MergeConflict);
    }
    let mut index = repo.index().map_err(|e| GitError::OperationFailed {
        operation: operation.to_string(),
        details: e.to_string(),
    })?;
    let tree_id = index.write_tree().map_err(|e| GitError::OperationFailed {
        operation: operation.to_string(),
        details: e.to_string(),
    })?;
    let tree = repo.find_tree(tree_id).map_err(|e| GitError::OperationFailed {
        operation: operation.to_string(),
        details: e.to_string(),
    })?;
    let head = repo
        .head()
        .and_then(|head| head.peel_to_commit())
        .map_err(|e| GitError::OperationFailed {
            operation: operation.to_string(),
            details: e.to_string(),
        })?;
    let theirs = repo
        .find_commit(annotated.id())
        .map_err(|e| GitError::OperationFailed {
            operation: operation.to_string(),
            details: e.to_string(),
        })?;
    repo.commit(
        Some("HEAD"),
        signature,
        signature,
        &format!("Merge {name}"),
        &tree,
        &[&head, &theirs],
    )
    .map_err(|e| GitError::OperationFailed {
        operation: operation.to_string(),
        details: e.to_string(),
    })?;
    repo.cleanup_state().map_err(|e| GitError::OperationFailed {
        operation: operation.to_string(),
        details: e.to_string(),
    })?;
    Ok(())
}

fn current_branch_tracking(repo: &Repository) -> Option<String> {
    match repo.sync_status() {
        SyncStatus::Ahead(count) => Some(format!("↑{count}")),
        SyncStatus::Behind(count) => Some(format!("↓{count}")),
        SyncStatus::Diverged { ahead, behind } => Some(format!("↕{ahead}/{behind}")),
        SyncStatus::Synced => Some("✓".to_string()),
        SyncStatus::NoUpstream => None,
        SyncStatus::Unknown => Some("?".to_string()),
    }
}

