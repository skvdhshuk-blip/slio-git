//! Native execution for branch.

use super::*;

pub(super) fn find_merged_branches(
    repo: &Repository,
    target: &str,
) -> Result<Vec<String>, GitError> {
    info!("Finding branches merged into '{}'", target);

    let repo_lock = repo.inner.read().unwrap();
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

    info!("Found {} branches merged into '{}'", branches.len(), target);
    Ok(branches)
}

pub(super) fn create_branch_from_start_point(
    repo: &Repository,
    name: &str,
    start_point: &str,
) -> Result<Branch, GitError> {
    info!("Creating branch '{}' from '{}'", name, start_point);

    let repo_lock = repo.inner.write().unwrap();
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

pub(super) fn delete_branch(repo: &Repository, name: &str, force: bool) -> Result<(), GitError> {
    info!("Deleting branch '{}'", name);
    if !force && !repo.is_branch_merged(name)? {
        return Err(GitError::OperationFailed {
            operation: "delete_branch".to_string(),
            details: format!("branch '{name}' is not fully merged"),
        });
    }
    let repo_lock = repo.inner.write().unwrap();
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

pub(super) fn rename_branch(
    repo: &Repository,
    old_name: &str,
    new_name: &str,
) -> Result<Branch, GitError> {
    info!("Renaming branch '{}' to '{}'", old_name, new_name);
    let repo_lock = repo.inner.write().unwrap();
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

pub(super) fn set_branch_upstream(
    repo: &Repository,
    branch_name: &str,
    upstream: &str,
) -> Result<(), GitError> {
    info!(
        "Setting upstream of branch '{}' to '{}'",
        branch_name, upstream
    );
    let repo_lock = repo.inner.write().unwrap();
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

pub(super) fn list_uncommitted_files(repo: &Repository) -> Vec<String> {
    let Ok(repo_lock) = repo.inner.read() else {
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

pub(super) fn has_uncommitted_changes(repo: &Repository) -> bool {
    !repo.list_uncommitted_files().is_empty()
}

pub(super) fn force_checkout_branch(repo: &Repository, name: &str) -> Result<(), GitError> {
    info!("Force checking out branch '{}' (discarding changes)", name);
    checkout_local_branch(repo, name, true)
}

pub(super) fn smart_checkout_branch(repo: &Repository, name: &str) -> Result<(), GitError> {
    info!(
        "Smart checking out branch '{}' (stash → checkout → unstash)",
        name
    );
    if !repo.has_uncommitted_changes() {
        return repo.checkout_branch(name);
    }
    crate::stash::stash_save_with_options(
        repo,
        Some("slio-git: smart checkout auto-stash"),
        true,
        false,
    )?;
    let checkout = repo.checkout_branch(name);
    let restore = crate::stash::stash_pop(repo, 0);
    checkout?;
    restore
}

pub(super) fn checkout_branch(repo: &Repository, name: &str) -> Result<(), GitError> {
    info!("Checking out branch '{}'", name);
    checkout_local_branch(repo, name, false)
}

pub(super) fn checkout_remote_branch(
    repo: &Repository,
    remote_ref: &str,
) -> Result<String, GitError> {
    info!("Checking out remote branch '{}'", remote_ref);

    let Some((_, local_branch_name)) = remote_ref.split_once('/') else {
        return Err(GitError::OperationFailed {
            operation: "checkout_remote_branch".to_string(),
            details: format!("invalid remote branch ref: {remote_ref}"),
        });
    };

    let local_exists = {
        let repo_lock = repo.inner.read().unwrap();
        repo_lock
            .find_branch(local_branch_name, git2::BranchType::Local)
            .is_ok()
    };
    if !local_exists {
        repo.create_branch_from_start_point(local_branch_name, remote_ref)?;
        repo.set_branch_upstream(local_branch_name, remote_ref)?;
    }
    repo.checkout_branch(local_branch_name)?;
    Ok(local_branch_name.to_string())
}

pub(super) fn merge_branch(repo: &Repository, branch_name: &str) -> Result<(), GitError> {
    info!("Merging branch '{}' into current branch", branch_name);
    crate::native::merge::start(repo, branch_name, false, false, false)
}

pub(super) fn list_branches(repo: &Repository) -> Result<Vec<Branch>, GitError> {
    info!("Listing all branches");

    let repo_lock = repo.inner.read().unwrap();
    let current_branch = repo.current_branch().ok().flatten().unwrap_or_default();
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
            current_branch_tracking(repo)
        } else {
            None
        };
        let sync_hint = compact_branch_sync_hint(upstream.as_deref(), tracking_status.as_deref());
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
